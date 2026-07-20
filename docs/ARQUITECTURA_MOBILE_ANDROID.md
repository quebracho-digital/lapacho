# Lapacho Mobile — Android-first

**Status:** design (not implemented)  
**Date:** 2026-07-16  
**Scope:** monorepo layout, IME + companion app, **storage as source of truth**, predictive keyboard (local, private), **optional secure multi-client sync (per-item)**  
**Decision:** Android first. iOS later (keyboard extension sandbox is a separate design).  
**Does not replace:** desktop Tauri shell; mobile is another shell over `lapacho-core`.

---

## 1. Product intent

Lapacho on mobile is **not** “the desktop UI scaled down”. It is two cooperating pieces:

| Piece | Role | Process model |
|-------|------|----------------|
| **Companion app** | Onboarding, settings, full history UI, key lifecycle, wipe | Normal app process; can die anytime |
| **IME (system keyboard)** | Type + local prediction + quick paste from Lapacho history | `InputMethodService`; **killed aggressively** when not in use |

Both share:

- classification / sensitivity / threats / ingest (`lapacho-core`)
- encrypted history (`HistoryRepo` semantics)
- local prediction engine (no network for prediction)
- optional **sync participation** (same crypto + policy as desktop; see §5)

They **do not** share:

- X11 / Wayland / tray / global shortcuts
- “keep secrets only in RAM forever” as the primary model (see §4)
- always-on cloud or automatic upload of every clip (sync is **opt-in**, see §5)

---

## 2. Same repo — layout

```
lapacho/
  crates/
    lapacho-core/           # existing — portable clipboard domain
    lapacho-predict/        # NEW — local n-gram / dict; no I/O policy
    lapacho-sync/           # NEW — E2E envelopes, policy, transport adapters (optional dep)
  apps/
    desktop/                # existing Tauri
    mobile/
      android/
        app/                # companion Application + activities
        ime/                # InputMethodService (or same module)
        # Rust via uniffi/jni: cdylib linked into both if needed
      README.md             # build / ADB / emulator notes
  docs/
    ARQUITECTURA_MOBILE_ANDROID.md   # this file
```

Cargo workspace (target end state):

- `members`: `lapacho-core`, `lapacho-predict`, `lapacho-sync`, (optional) `apps/mobile/android/rust-bridge`
- `apps/desktop/ui` stays excluded (wasm)
- Desktop `src-tauri` stays desktop-only dependencies (`arboard`, tray, …)
- Sync is a **feature flag / optional dependency** on shells (desktop + mobile companion). The IME process should **not** own network sync; companion (or desktop daemon) does.

**Rule:** anything that must run inside the IME process must be usable without a display, without Tauri, and with a cold start measured in tens of ms for the hot path (load top-N + open DB), not full history decrypt.

---

## 3. What we reuse from desktop

| Desktop concept | Mobile |
|-----------------|--------|
| `process_text` / ingest / detectors / threats | Same, on capture and on import |
| `ClipboardItem` / `UIClipboardItem` | Same types; UI projection never gets raw secrets in logs |
| `HistoryRepo` + `RetentionPolicy` + `PersistLevel` | Same trait semantics; path + keyring adapters change |
| AES-GCM at rest + content-id keyed | Same crypto story; key in **Android Keystore** |
| Paranoia / sensitive TTL | Same *policy names*; **ephemeral semantics change** (§4.3); **gate sync of secrets** (§5) |
| Plugins (stdin subprocess) | **Out of scope v1** (IME cannot spawn arbitrary tools safely) |
| Multi-device history | Optional, E2E, **per-item** opt-in (§5) — shared crate, all shells |

---

## 4. Storage: source of truth (answer to the RAM question)

### 4.1 Short answer

**Yes: durable encrypted storage is the source of truth. Do not rely on process memory for history that should survive.**

On Android the IME and the companion are killed without ceremony (LMK, swipe-away, OEM “battery optimizers”). Anything only in RAM is gone. That is not a desktop-style long-lived daemon.

**However:** “read everything from storage every keystroke” is the wrong extreme. The right model is:

1. **Disk (encrypted DB / files) = authority** for what exists after process death.  
2. **RAM = short working set** while the process is alive (top-N list, open DB handle, decrypted rows currently shown).  
3. **Load from storage on demand** when the IME/app becomes active or when the user opens the clipboard strip — not on every character unless the working set is missing.  
4. **Write-through on capture:** new clipboard item → classify → encrypt → `save` → then update working set. If the process dies mid-write, last successful transaction wins (SQLite WAL).

So: **not “memory-only”**, and **not “full reload per keypress”** — **authority on disk, cache in RAM for the current process lifetime**.

### 4.2 Why pure RAM fails here (even more than desktop Paranoia)

| Desktop | Android IME |
|---------|-------------|
| One long-lived process (tray app) | IME process often dies when keyboard hides |
| Secrets under Paranoia can live in `tray_recent` for a while | That buffer is empty after kill → user thinks Lapacho “lost” the clip |
| User restarts rarely | LMK is normal, not exceptional |

**Conclusion:** mobile cannot copy desktop’s “secrets only in RAM” as the default UX for “I copied a token 30s ago and switched apps”. Either:

- **A (recommended for mobile default):** allow **encrypted, short-TTL** persistence of secrets/credentials on disk (same as `RetentionPolicy.sensitive_ttl_secs`, aggressive default e.g. 15–30 min), still never upload, still wipeable; or  
- **B:** keep “never on disk” but then document honestly that **secrets vanish when Android kills the IME** — usually worse UX and looks like a bug.

Desktop can keep stricter Paranoia; mobile profile should default to **A** with a clear setting “Never store secrets (may disappear when keyboard closes)”.

### 4.3 Where files live (security)

All Lapacho data stays in **app-private storage** (same app ID for companion + IME):

| Asset | Location (conceptual) | Protection |
|-------|------------------------|------------|
| History DB | `context.filesDir` / `no_backup` preferred | Mode private; **not** shared storage / MediaStore |
| SQLCipher or app-level AES-GCM blobs | same tree | Ciphertext at rest; key never in prefs plaintext |
| Key material | **Android Keystore** (AES key or wrapping key) | Hardware-backed when available; no export |
| User prefs (persist level, TTL) | encrypted prefs or DB `preferences` table | Same as desktop `set_preference` |
| Prediction dict / user language model | app-private; optional separate file | No network; user-clearable |
| Logs | minimal; **never** raw clipboard | Debug builds only for sensitive paths |

**Do not use:**

- External storage / Downloads for history  
- Backup to Google Drive for the history DB unless user opts into an **encrypted** export format we control  
- `ClipboardManager` as long-term store (system clipboard is hostile: other apps, sensitive notifications)

**Multi-process:** IME and companion often run as **two processes** of the same UID. SQLite must be opened with:

- single-writer discipline (WAL + careful connection lifecycle), or  
- a small **in-app ContentProvider / bound service** that owns the DB and both call into it.

v1 recommendation: **one process owns writes** if contention appears; start with WAL + short-lived connections and measure. Document “no two writers blindly” as a hard rule.

### 4.4 When to read from storage

| Event | Action |
|-------|--------|
| IME `onCreate` / first `onStartInput` after cold start | Open DB, load **top-N** (e.g. 20) into working set, run `cleanup(policy)` |
| User opens “clipboard strip” / history panel on keyboard | If working set stale or empty → `load`/`search` from repo |
| Companion app opens history screen | Always load from repo (or refresh if resume after long pause) |
| New system clipboard capture (if permitted) | Ingest → `save` → bump working set; no need to reload full table |
| Prediction while typing | Use in-memory LM; **do not** scan full clipboard history per character. Optional: seed LM from **non-secret** recent phrases on IME show |
| Process death | Working set discarded; next show rebuilds from disk |

**Staleness:** if both processes can write, prefer version/timestamp in prefs or SQLite and refresh working set when strip opens (cheap `SELECT` top-N), not continuous polling.

### 4.5 What “read every time you need it” means in practice

Interpret the requirement as:

> **Never treat RAM as durable. Every user-visible history entry must be reconstructible from encrypted storage after a cold start (within PersistLevel + TTL rules).**

Not as:

> Re-open and decrypt the entire DB on every key event.

Hot path for typing: prediction only.  
Hot path for paste strip: one query top-N + decrypt those rows only.

### 4.6 Capture path (clipboard → storage)

Android clipboard access is restricted (background reads limited; notifications of changes vary by API level). Design for:

1. **Primary:** user copies while Lapacho IME or companion is active / has appropriate role → capture → ingest → **write-through to repo**.  
2. **Secondary:** explicit “import current clipboard” button on the strip (always works when keyboard is up).  
3. **Do not promise** 24/7 background clipboard sniping like a desktop tray monitor — OEMs will break it and it is a privacy minefield.

Images: same as desktop later; v1 can be text-only to cut attack surface and size.

---

## 5. Multi-client sync (optional, secure, per-item)

Cross-device Lapacho (desktop ↔ Android, later iOS) is a **first-class product goal**, implemented once in shared crates and **off by default**. Mobile design must not paint us into a “local-only forever” corner; local encrypted disk remains source of truth **on each device**, sync is a deliberate export of selected items into a shared, end-to-end encrypted channel.

### 5.1 Goals

| Goal | Meaning |
|------|---------|
| **Optional** | Sync disabled until the user enables a vault / peer link. No network required for core clipboard + IME + prediction. |
| **Per-item** | Default for a new clip: **do not sync**. User (or explicit rule) marks items as syncable. No bulk silent upload of the whole history. |
| **Secure** | End-to-end encryption: transport and any relay see only ciphertext + minimal metadata. Keys never leave devices in the clear. |
| **Policy-gated secrets** | Whether a `Secret` / `Credential` **may** sync is decided by **PersistLevel / paranoia** (and per-item flag). If policy forbids, the item cannot enter the sync outbox even if the user taps “share”. |
| **Same semantics everywhere** | Desktop and mobile use the same `lapacho-sync` rules so a phone does not become a weaker link. |

### 5.2 What “optional at item level” means

Each `ClipboardItem` (or row in the repo) gains sync-related fields (names indicative):

| Field | Role |
|-------|------|
| `sync_eligible` | User/system intent: this item is allowed into the outbox if policy passes. Default `false`. |
| `sync_state` | `LocalOnly` \| `Pending` \| `Synced` \| `RejectedByPolicy` \| `Error` |
| `sync_id` / revision | Stable id for multi-device dedup (aligned with content-id / keyed id where possible) |

**UX (companion + desktop list; IME strip later):**

- Action per item: **“Sync to my devices”** / **“Stop syncing”** (or pin-to-vault).  
- Optional bulk: “sync last N non-sensitive” — never default for secrets.  
- Clear badge: local-only vs shared.  
- Incoming synced items land in local encrypted storage (write-through), then appear in IME top-N after reload/working-set refresh — same as local captures.

**Not per-item (global settings):**

- Which devices/peers are trusted  
- Transport (self-hosted relay vs LAN / QuebrachOS vs later third party)  
- Master switch: sync feature on/off  
- Default rules templates (e.g. “never auto-queue Secret”) — still overridden by explicit per-item action only when policy allows

### 5.3 Paranoia / PersistLevel as gate (including secrets)

Sync of sensitive material is **not** a separate ad-hoc switch that bypasses paranoia. It is a **second gate** after local persistence rules:

```
capture → ingest → may persist locally? (PersistLevel + sensitivity)
                 → user marks sync_eligible?
                 → may sync? (sync policy derived from same paranoia model)
                 → encrypt for peers → outbox
```

Indicative matrix (tune in implementation; document defaults in settings UI):

| PersistLevel (paranoia) | `None` text | Personal | Credential | Secret |
|-------------------------|-------------|----------|------------|--------|
| **None** (strict / classic “paranoia”) | Sync only if `sync_eligible` | Optional, eligible | **No** (unless we later add explicit unlock flow) | **No** |
| **Sensitive** | If eligible | If eligible | If eligible + short remote TTL | **No** by default |
| **All** | If eligible | If eligible | If eligible | **If eligible** (user must opt in per item) |

Rules of thumb:

1. **`sync_eligible` never overrides “policy says no”.** UI disables the action and explains why.  
2. Under levels that **do not keep secrets on disk**, those items also **cannot sync** (nothing durable to package, and sync would be a weaker side channel). Mobile’s short-TTL disk for secrets (§4.2) can allow sync **only while** the item is still within TTL and policy is `All` (or a future explicit “sync secrets” mode).  
3. Remote retention for sensitive payloads should honor **TTL ≤ local sensitive TTL** (or peer-enforced wipe); no infinite cloud retention of secrets.  
4. Revoking sync / delete-on-all-devices is a **v2+** concern; v1 can be “push ciphertext; peers apply local TTL on ingest”.

### 5.4 Security properties (non-negotiable)

| Property | Requirement |
|----------|-------------|
| E2E | Payload encrypted to **sync-chain / device keys**; relay cannot read `raw_content`. |
| Metadata minimization | Prefer content-id hashes that are **keyed** (same spirit as local storage), not cleartext previews on the server. Safe previews only after local decrypt. |
| AuthN of peers | **Device pairing** establishes who can decrypt (see §5.8). Identity providers (Authentik) may gate *access to the relay*, not the plaintext. |
| Transport | TLS if IP path; ciphertext still E2E underneath. |
| IME | **No sync network stack in the IME process.** Companion owns outbox/inbox; IME only reads local repo. |
| Logging | Same as local: never log raw or key material. |
| Compromise model | Stolen relay ≠ stolen clipboard. Stolen device still requires local keystore/lock screen; remote wipe later. |

---

### 5.5 Do we build our own sync engine?

**Yes for the product protocol; no for reinventing every crypto primitive.**

| Layer | Own / reuse | Notes |
|-------|-------------|--------|
| **Policy + domain** (`may_sync`, per-item flags, paranoia, TTL on ingest) | **Own** (`lapacho-sync` + `lapacho-core`) | Nobody else’s engine knows Lapacho sensitivity. |
| **Envelope format** (versioned blob: ciphertext, nonce, device_id, sync_id, rev, expires_at) | **Own** (small, stable, documented) | Keep it boring CBOR/MessagePack or length-prefixed binary. |
| **Outbox / inbox / ack / retry** | **Own** thin state machine | Not a general CRDT filesystem. Volume is tiny (selected clips), not multi-GB trees. |
| **Crypto primitives** | **Reuse** (same family as core: AEAD, X25519/Ed25519 or libsodium-class APIs via audited crates) | Do not invent ciphers. |
| **Full CRDT / Automerge / Yjs** | **Avoid for v1** | Overkill for append-mostly clipboard rows + explicit deletes later; harder threat model (merge of secrets). |
| **Syncthing / a file-sync service as clipboard store** | **Not as vault** | Fine as *optional* way to move opaque files someday; wrong UX and ACL for per-item secrets. a file-sync service on the self-hosted node stays file sync, not Lapacho history. |
| **Matrix E2EE as transport** | **Optional adapter later** | Attractive if we already live on a Matrix homeserver; still our envelopes inside room messages. Not required for v1. |

**Design stance:** Lapacho sync is a **small, purpose-built E2E message queue for clipboard envelopes**, not a general multi-master database. Each device’s SQLite remains the source of truth locally; the network only moves **explicitly eligible** envelopes.

Why not “just use Syncthing / git / drive folder”?

- No first-class **per-item policy** or sensitivity.  
- Hard to stop a secret from landing on a peer that should not have it.  
- Recovery and pairing UX for non-technical users is worse than a QR chain.  
- Relay operators (or folder ACLs) see more than we want unless we still E2E-encrypt — at which point we already have our envelope layer.

---

### 5.6 Topology: pure P2P vs relay (recommended hybrid)

**Pure direct P2P only is not enough** for phone + laptop in the wild (NAT, sleep, different networks). **Relay-only with no E2E** is unacceptable. Target:

```
                    ┌─────────────────────┐
   Device A ──────►│  Relay (optional)    │◄────── Device B
   (desktop)       │  ciphertext only     │        (Android)
         \         │  store-and-forward   │         /
          \        └─────────────────────┘        /
           \              ▲                      /
            \             │ TLS + authz          /
             \            │                     /
              └── optional direct path ────────┘
                  (LAN / WireGuard / mDNS)
```

| Mode | When | Role |
|------|------|------|
| **A. Store-and-forward relay (default for multi-site)** | Devices not online at the same time; phone on LTE, PC at home | Relay holds **encrypted** envelopes with short retention; devices pull/push when awake. |
| **B. Direct / LAN boost** | Same LAN or WireGuard (`the VPN subnet`) | Prefer direct HTTPS/QUIC or WebSocket between peers if discovery succeeds; lower latency, less load on the self-hosted node. |
| **C. Local-only** | Sync master switch off | No network; current product. |

**v1 recommendation (Quebracho):**

1. Ship **protocol + E2E + outbox** in `lapacho-sync`.  
2. First transport adapter: **self-hosted relay** on the self-hosted node (small service under QuebrachOS / Caddy, tunnel hostname e.g. `lapacho-sync.example.org` or LAN-only + WG).  
3. Second adapter: **direct over WireGuard/LAN** when both peers reachable (can be same API: “push to peer URL” with device certs).  
4. Do **not** depend on a vendor cloud. Optional later: “bring your own relay URL”.

**NAT / “true P2P” (ICE/STUN/TURN):** defer. TURN is just another relay with more moving parts. WireGuard already solves “my devices are on one virtual LAN” for Quebracho users; use that.

**Who runs the relay?**

- **Personal / PYME:** the self-hosted node (or any tiny VPS the user controls).  
- **No server:** pair on LAN only, or one device temporarily acts as “introducer” while both online (limited).  
- Relay is **untrusted for confidentiality**; it is trusted only for availability and (if Authentik is used) for *who may deposit blobs into a mailbox*.

---

### 5.7 Authentication: three layers (do not collapse them)

These solve different problems. Mixing them causes either “Authentik can read my clipboard” (false) or “QR pair but anyone can spam my relay” (bad).

| Layer | Question it answers | Mechanism |
|-------|---------------------|-----------|
| **L1 — Device / chain trust (E2E)** | Which devices can **decrypt** envelopes? | Brave-style **sync chain** (§5.8): first device creates chain key; later devices join via QR / codewords. |
| **L2 — Transport security** | Is the pipe private from network observers? | TLS 1.3 to relay; or WG encryption for direct path. |
| **L3 — Relay authorization (optional but useful)** | Who may **upload/download ciphertext** for this mailbox? | Authentik OIDC / access tokens, or mutual TLS device certs issued at pair time, or a long-lived relay API key bound to the chain id (not to plaintext). |

**Authentik alone is not enough** for clipboard E2E:

- SSO proves “this HTTP client is Leo’s account on QuebrachOS”.  
- It does **not** put encryption keys only on devices.  
- If the relay decrypts with a server-side key “for convenience”, that is **not** Lapacho (server sees secrets).  

**Authentik is enough (and desirable) for:**

- Logging into the **relay control plane** (create mailbox, list devices’ public ids, revoke a device’s *relay* access).  
- Multi-user deployments later (family / team): each Authentik user → isolated mailbox namespace.  
- Aligning with existing the self-hosted node SSO (same as Matrix, Kuma, etc.).

**Brave Sync–style pairing is the right model for L1** even when L3 uses Authentik:

- Offline-capable mental model: “scan this QR on the phone”.  
- Works without an account for pure LAN / friend-hosted relay.  
- Chain key never stored on Authentik.  
- Recovery codewords for adding a device when QR is hard (headless desktop ↔ phone).

**Recommended combo for Quebracho home lab:**

1. **Pairing (L1):** Brave-like chain — always.  
2. **Relay (L3):** Authentik forward-auth or OAuth device flow on the relay API — when the relay is exposed beyond pure WG.  
3. **On WireGuard-only LAN:** L3 can be “know the relay URL + present device signature from L1” without browser SSO every time (device key = client auth).

```
[User] --pairs--> [Device A] creates SyncChain (seed)
                      |
                      | QR / 24 words / short setup code (rate-limited)
                      v
                 [Device B] joins chain, stores chain secrets in OS keystore
                      |
                      | registers device public id with relay (optional L3)
                      v
                 [Relay] mailbox_id = hash(chain_id) or user-bound id
                      |
        envelopes encrypted to chain (or per-device wrappers)
```

---

### 5.8 Pairing flow (Brave Sync–inspired)

**First device (creates the chain)**

1. User enables Sync → “Create new sync chain”.  
2. Client generates:  
   - `chain_id` (public-ish identifier),  
   - `chain_secret` / root key material (high entropy),  
   - device keypair `(device_sk, device_pk)` for this install.  
3. Secrets go into OS keystore / Android Keystore / desktop keyring — **never** into the history SQLite as plaintext.  
4. UI shows **QR + codewords** (and optional short code for typing).  
5. Optional: register mailbox on relay with L3 credentials.

**Second device (joins)**

1. “Join existing chain” → scan QR or enter codewords.  
2. Derive same chain secrets; generate its own device keypair.  
3. Optionally publish `device_pk` + signed join announcement so peers can use pairwise wrapping later.  
4. Pull pending envelopes; decrypt; run local `may_persist` / TTL before save.

**Security properties of the pair ceremony**

| Property | How |
|----------|-----|
| QR / words are **bootstrap only** | After join, daily traffic uses device keys + chain; words not re-sent. |
| Short codes | High entropy or rate-limit + short TTL if human-typed; prefer QR. |
| No server sees words | Ceremony is device-to-device (visual) or typed by user; relay never gets the seed. |
| Revoke device | Remove `device_pk` from trusted set; rotate chain if device was compromised (v2: re-key). v1: “reset chain” + re-pair all. |

**Why not “login with Authentik on both devices and magically sync”?**

- Convenient, but then the **server or IdP path** becomes part of key distribution unless we still do a second factor (scan QR).  
- Hybrid that *is* OK: Authentik opens the relay UI → “show pairing QR for this account’s mailbox” still displays a **client-generated** QR whose secret never hit Authentik. Authentik only gates who can *create* a mailbox or download *ciphertext*.

---

### 5.9 Envelope, encryption, and conflict model

**Envelope (logical fields)**

- `v` protocol version  
- `sync_id` / `content_id` (keyed where applicable)  
- `rev` or `created_at` for ordering  
- `sensitivity` (or encrypted inside payload — prefer **inside** ciphertext so relay cannot filter-spy by class; policy re-checked on receiver)  
- `expires_at` (remote TTL hint)  
- `ciphertext` = AEAD(payload)  
- `sender_device_id`  
- auth tag / signature

**Payload (after decrypt):** raw + display hints + type + local policy snapshot needed for ingest.

**Who can decrypt?**

- **v1 simplest:** single **chain symmetric key** (Brave-like): any device on the chain decrypts all chain envelopes. Easy pairing, easy mental model. Risk: any paired device reads all synced items (acceptable for personal multi-device; document it).  
- **v2:** per-device wrapping (encrypt to each `device_pk`) or sender keys subgroups (“work laptop only”). Defer until multi-user or selective share.

**Conflicts**

- Same `sync_id` twice: last-writer-wins by timestamp/rev, or “already have content_id → bump local rank” (same as local dedup).  
- No field-level CRDT merge of secret strings.  
- Delete/tombstone: later phase.

**Outbox behavior**

- User marks eligible → policy check → encrypt → queue.  
- Companion flushes queue when online; exponential backoff.  
- Ack from relay or peer → `sync_state = Synced`.  
- Failure does not delete local item.

---

### 5.10 Crate `lapacho-sync` (expanded sketch)

```
lapacho-sync/
  policy.rs       # may_sync(item, PersistLevel) → bool
  envelope.rs     # seal / open
  chain.rs        # create / join / export QR payload / codewords
  device.rs       # device keypair, trust list
  outbox.rs       # queue + state machine
  inbox.rs        # pull → open → hand off ClipboardItem to core ingest
  transport/
    trait.rs      # push(env), pull(since) → Vec<env>
    relay_http.rs # the self-hosted node adapter
    direct.rs     # optional LAN/WG peer
    memory.rs     # tests
```

- Depends on `lapacho-core`.  
- Shells (Tauri desktop, Android companion) depend on `lapacho-sync`; **IME does not**.  
- Unit tests: policy matrix, seal/open, join chain from words, reject wrong chain.  
- Integration tests: two in-process “devices” + memory transport.

---

### 5.11 Phasing relative to mobile

Sync is **cross-shell**: not “Android-only”. Scheduling:

| Phase | Sync work |
|-------|-----------|
| P0–P1 local mobile | Schema placeholders only (`sync_eligible`, …). |
| Parallel / early | Pure-Rust `lapacho-sync` + memory transport + chain join tests (no Android required). |
| P4a | Relay HTTP adapter + desktop ↔ desktop on LAN/WG. |
| P4b | Android companion push/pull; Authentik in front of relay if public hostname. |
| P4c | Direct path optimization; tombstones; device revoke UX. |

- Local mobile P0–P2 ships **without** network.  
- **Schema foresight** early so we do not migrate twice.

---

### 5.12 Explicit non-goals for sync v1

- Transparent full-history mirror of every copy.  
- Server-side search over plaintext.  
- Automatic sync of all secrets when PersistLevel is strict.  
- Running sync inside the IME process.  
- **Authentik (or any IdP) as the encryption root** for clipboard content.  
- Full ICE/STUN/TURN mesh as a dependency of v1.  
- Syncthing/a file-sync service as the clipboard database.  
- Multi-user team sharing with per-recipient ACLs (personal multi-device chain first).  
- Depending on Google Drive / iCloud as the E2E layer (optional export of **our** ciphertext later only).

---

### 5.13 Decision summary (sync architecture)

| # | Decision | Choice |
|---|----------|--------|
| S1 | Engine | **Own** thin E2E queue + policy; reuse standard crypto; no full CRDT v1 |
| S2 | Topology | **Hybrid:** self-hosted store-and-forward relay + optional LAN/WG direct |
| S3 | Pure internet P2P | **Not v1** (NAT pain); WG/LAN counts as “direct” |
| S4 | Device auth (decrypt) | **Brave-like chain** (QR / codewords), secrets only on devices |
| S5 | Relay authz | **Authentik optional** for who may use the mailbox API; never for decrypt |
| S6 | Without Authentik | Chain + device signatures still work on LAN/WG or private relay API key |
| S7 | Encrypt model v1 | Shared chain key (all my devices); pairwise later if needed |
| S8 | Relay trust | Untrusted for confidentiality; trusted for availability / quota only |

---

## 6. Predictive keyboard (private by design)

### 6.1 Goals

- Suggestions **on-device only** (no cloud API, no analytics of keystrokes).  
- Network permission: **not requested** by IME in v1 (and preferably never for prediction).  
  Network, if ever granted, is for **optional sync in the companion app only** (§5), never for keystroke prediction.  
- Learning: off by default; if on, stored only in app-private model file; wipe = delete file + optional factory dict restore.  
- **Secrets must not train the model.** After ingest, if `Sensitivity::Secret` or `Credential`, exclude raw from any “learn from clipboard / learn from typed” path. Prefer learning only from `Sensitivity::None` (and maybe Personal with explicit opt-in).  
- Synced-in items: same rules — do not train on Secret/Credential raw even if they arrived via sync.

### 6.2 Crate `lapacho-predict`

- Pure Rust: score candidates given prefix + optional language.  
- No Android types.  
- IME calls via uniffi/jni: `suggest(prefix, limit) -> Vec<String>`.  
- Dictionary assets shipped in APK; user lexicon separate file.

### 6.3 Clipboard × keyboard

Useful integrations (v1+):

- Strip above keys: last N **UI-safe** previews; tap inserts **raw** via IME commit (same security UX as desktop copy: user intentional).  
- Long-press secret: confirm or require companion unlock if we add biometrics later.  
- Prediction must **not** surface full secrets as ghost text.  
- Sync badges on strip are optional polish (P3+); paste path remains local repo only.

---

## 7. Security model (mobile-specific)

| Threat | Mitigation |
|--------|------------|
| Other apps read our DB file | App-private dir + encryption; no world-readable paths |
| Device backup exfiltrates history | `allowBackup=false` or exclude DB; prefer encrypted export only |
| Root / physical attacker | Keystore + strong lock screen; accept residual risk; short TTL for sensitive |
| IME process dump | Minimize plaintext lifetime; zeroize buffers after commit where feasible |
| Overlay / fake keyboard | System IME picker; user education in onboarding (“enable Lapacho keyboard”) |
| Clipboard race with other managers | We don’t own the system clipboard long-term; we own **our** history |
| Logging | No `Log` of raw; crash reports without clip content |
| Malicious or compromised relay | E2E only; relay cannot read items; pair only trusted devices (§5) |
| Accidental secret exfiltration via sync | Per-item opt-in + PersistLevel gate; no silent full-history sync |

Align with desktop: **raw is source of truth inside the vault; UI and logs only see safe projection.** Sync never weakens that: peers receive ciphertext envelopes, not cleartext over the wire.

---

## 8. Platform constraints (Android)

- **Min SDK:** decide at spike (recommend API 26+ or 29+ to drop legacy clipboard quirks).  
- **IME settings** entry for: PersistLevel, sensitive TTL, learn typing, clear history, export wipe.  
- **Companion-only** settings for: sync master switch, paired devices, per-item sync actions (not in IME if that complicates cold start).  
- **Foreground service:** avoid for v1 clipboard monitoring; prefer event-driven when visible + manual import. Sync workers run in companion (WorkManager / similar), not IME.  
- **Play policy:** keyboard apps must be careful with accessibility and privacy labels; declare no data collection for prediction; document optional self-hosted sync separately.  
- **Distribution:** sideload / F-Droid friendly first fits Quebracho; Play later if wanted.

---

## 9. Phased delivery

### P0 — Spike (validate kill + storage)

1. Empty companion + empty IME that types characters.  
2. Shared encrypted SQLite in app-private storage; write from companion, **read from IME after force-stop**.  
3. Prove: after “Force stop”, IME still shows last saved items.  
4. Measure cold-start time to show top-10.  
5. Schema foresight: columns/prefs placeholders for `sync_eligible` / `sync_state` (can be unused).

**Exit criteria:** storage-as-truth proven; multi-process read works.

### P1 — Core bridge

1. `lapacho-core` behind uniffi (or thin JNI).  
2. Ingest text → save with PersistLevel + TTL cleanup.  
3. Clipboard strip on IME (top-N, paste on tap).  
4. Settings: paranoia-equivalent + TTL.

### P2 — Prediction

1. `lapacho-predict` + bundled ES/EN dict.  
2. Suggestion bar; no network.  
3. Exclude sensitive from learning.

### P3 — Hardening & polish (local product)

1. Biometric gate for revealing secrets (optional).  
2. Image clips (if still desired).  
3. Export/wipe, battery and ANR audit.  
4. Document iOS delta (separate short design).

### P4 — Multi-client sync (optional E2E, per-item)

1. Crate `lapacho-sync`: envelope, `may_sync(item, PersistLevel)`, outbox/inbox, unit tests for policy matrix (including secrets when level authorizes).  
2. Peer pairing (QR / recovery) + preferred self-hosted / LAN transport adapter.  
3. Desktop + Android companion: per-item “Sync to my devices”, badges, master switch off by default.  
4. Ingest path on receive: decrypt → same `HistoryRepo` rules + TTL; IME sees items only via local storage.  
5. Explicit tests: Secret under strict paranoia **cannot** outbox; under `All` + `sync_eligible` **can**.

**Exit criteria:** two real clients exchange a non-secret item E2E; secret path proven only when policy allows; relay (if any) never sees plaintext.

---

## 10. Explicit non-goals (local v1 / P0–P3)

- Full desktop UI in WebView as the keyboard  
- **Automatic full-history cloud mirror** (replaced by optional per-item E2E sync in P4, off by default)  
- Background 24/7 clipboard vacuum  
- Plugins  
- iOS shipping  
- Sync inside the IME process  
- Replacing Gboard for everyone — Lapacho is a **security-conscious** keyboard + vault, not a feature clone of Gboard

---

## 11. Decisions log

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | Same monorepo | Already `core` + `apps/*`; mobile is another app |
| D2 | Android-first | IME model less hostile than iOS extensions for a private keyboard MVP |
| D3 | Encrypted disk = source of truth | OS kills processes; RAM-only history is a product bug on mobile |
| D4 | RAM = working set only | Performance + less decrypt thrash; reload on show / cold start |
| D5 | Mobile default: short-TTL disk for secrets | Desktop Paranoia RAM buffer does not survive IME death |
| D6 | No network for prediction | Privacy product promise |
| D7 | Tauri optional for companion only | IME is native `InputMethodService` |
| D8 | Capture primarily while engaged + manual import | Background clipboard APIs are fragile and privacy-sensitive |
| D9 | Optional multi-client sync, **per-item** opt-in | User control; no silent vault upload; works with paranoia |
| D10 | Secrets sync only if PersistLevel/paranoia **and** per-item eligibility allow | Policy is one model, not two contradictory switches |
| D11 | E2E + self-hostable transport; sync in companion/desktop only | Relay untrusted; IME stays offline-capable |
| D12 | Default new items: `sync_eligible = false` | Fail closed |
| D13 | Own thin sync engine (not Syncthing/CRDT as vault) | Domain policy + small volume; see §5.5 / S1 |
| D14 | Hybrid topology: the self-hosted node relay + optional LAN/WG direct | NAT reality; QuebrachOS-aligned; §5.6 / S2–S3 |
| D15 | Brave-like chain for E2E; Authentik only for relay authz | IdP ≠ encryption root; §5.7–5.8 / S4–S6 |

---

## 12. Open questions (resolve in P0/P1; sync in P4)

1. **SQLCipher vs app-level AES-GCM** (desktop today) on Android — stick to core crypto for one code path unless SQLCipher wins on multi-process.  
2. **Single process vs ContentProvider** for DB ownership after P0 measurements.  
3. **Biometrics** before paste of Credential/Secret — default on or off?  
4. **Language pack size** vs offline quality for ES-AR.  
5. Whether companion is pure native or thin Tauri Android (prefer **native/Kotlin thin** for IME cohabitation unless Tauri clearly helps).  
6. **Relay v1 surface:** public tunnel hostname + Authentik, or **WG-only** first (simpler L3)?  
7. Envelope encoding: CBOR vs JSON+b64 for debuggability.  
8. **Delete-everywhere / revoke device** — v1 “reset chain” only vs tombstones.  
9. Auto-rules later (“always sync URLs”) vs forever manual-only for sensitive classes.  
10. Whether a Matrix adapter is worth it after HTTP relay exists (a Matrix homeserver already on the self-hosted node).

---

## 13. Relation to desktop docs

- Desktop refactor debate: `docs/ARQUITECTURA_REFACTOREO.md`  
- Tasks backlog: `docs/GROK_TASKS.md`  
- Mobile does **not** block desktop merge; it depends on a **stable `lapacho-core` API** (`HistoryRepo`, ingest, crypto).  
- Sync is **desktop + mobile**; implement in `lapacho-sync` once, wire both shells.  
- When mobile/sync land code, update root `ROADMAP.md` + Quebracho `CONTEXT.md` project row.

---

## 14. One-paragraph summary

**Same repo, new `apps/mobile/android` + optional `lapacho-predict` + later `lapacho-sync`.** The system keyboard and the companion share an **encrypted on-disk history** as the only durable source of truth because Android will kill the IME; process memory is only a **working set**. Load top-N from storage when the keyboard or history UI needs it; write-through on every accepted clip. Do not re-decrypt the whole vault per keystroke. Default mobile policy should allow **short-TTL encrypted secrets on disk**, with an explicit opt-out that accepts data loss on process death. Prediction stays local and never learns from secrets. **Multi-client sync is optional, E2E, off by default, and opt-in per item**; secrets may sync only when paranoia/`PersistLevel` authorizes it and the user marks the item eligible — never as a silent full-history mirror. The engine is a **small own protocol** (outbox of envelopes), not Syncthing-as-vault; topology is **hybrid** (self-hosted store-and-forward + LAN/WG direct). **Pairing is Brave-like** (QR/codewords → chain keys on device only); **Authentik can authorize the relay API** but must never be the root of clipboard encryption.
