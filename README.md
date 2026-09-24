# Lapacho

A **secure** clipboard manager for the desktop and Android. It classifies, sanitizes, and masks
content before it reaches the UI: credentials and secrets are never shown in
plain text, and the original content is only released when you explicitly paste
it back.

Part of the **Quebracho Digital** ecosystem. Replaces the prototypes
`quebracho-client` and `RustyBoard`.

> **Status:** advanced development. 98 tests across the workspace. The desktop
> app (Tauri 2 + Leptos/WASM) has a complete backend, native tray, global
> shortcut Ctrl+Shift+Alt+L (Lapacho-exclusive), image support, rich rendering
> (MD/SVG/Mermaid), and a raw-first security model.
>
> Known gap: the tray menu rebuild costs ~290 ms per capture, which is where
> the end-to-end latency lives (capture and storage together are ~16 ms). See
> [Diagnostics](#diagnostics) for how to measure it yourself.
>
> **Android** is an early spike, usable day to day: a keyboard with a paste
> strip, encrypted history, word suggestions and spelling corrections from
> on-device dictionaries (Spanish bundled, others imported and mixed), and the
> same sensitivity classifier as desktop (`lapacho-core` compiled for Android).
> The app and the keyboard speak English and Spanish.
> Downloads: [web.fishman.work](https://web.fishman.work/) (English) ·
> [en español](https://web.fishman.work/es). See [Mobile](#mobile).

## Usage

- **Desktop:** [`docs/USAGE_DESKTOP.md`](docs/USAGE_DESKTOP.md) — shortcuts,
  tray, per-item actions, persistence modes, plugins.
- **Android:** [`docs/USAGE_MOBILE.md`](docs/USAGE_MOBILE.md) — install,
  enabling the keyboard, capture and paste, how passwords are handled,
  suggestions and corrections. Dictionaries — format, mixing, making your own:
  [`docs/DICTIONARIES.md`](docs/DICTIONARIES.md).

## Features

- **Automatic capture** of the clipboard via a background monitor.
- **Type classification:** text, URL, JSON, SVG, Mermaid, Markdown, image.
- **Sensitivity classification:** `None` / `Personal` / `Credential` / `Secret`,
  using regex + Shannon entropy (private keys, API tokens, credit cards, emails,
  etc.).
- **Masking:** credentials and secrets are shown redacted — `••••last4` for a
  secret, `first3…last4` for a credential — so two of them are still
  distinguishable in a list without revealing the value or its length. The
  `raw_content` is never sent to the UI layer. See
  [Security Model](#security-model) for what that trade does and does not buy.
- **Persistence policies:**
  - `Paranoia` (default): only stores non-sensitive content.
  - `Balanced`: stores everything except secrets, with TTL for credentials.
  - `All`: stores everything.
- **SVG sanitization:** removes XSS vectors (scripts, `on*` handlers,
  `javascript:`).
- **Advanced XSS detection:** 3 detectors for advanced attack vectors:
  - **DataUrlScript:** detects `data:text/html,<script>...` and base64-encoded SVG with `<script>`
  - **SvgEventHandler:** detects `<svg>` with `onload`, `onerror`, or other event handlers
  - **ImgOnError:** detects `<img>` with `onerror` handler (classic XSS vector)
- **Transformation plugins:** external commands that receive content via
  `stdin` (no command injection), with timeout, and whose output is sanitized
  before display.
- **SQLite history** (WAL mode) with size limit and TTL-based cleanup.

## Architecture

Cargo workspace (Rust 2024 edition):

```
lapacho/
├─ crates/lapacho-predict/ # Word completion over a dictionary (lib, no I/O)
├─ crates/lapacho-core/    # Pure logic, no UI (lib)
│  ├─ types        # ClipboardItem, UIClipboardItem (safe projection), enums
│  ├─ detectors    # content type classification
│  ├─ security     # sanitization + sensitivity classification
│  ├─ storage      # SQLite: history, persistence levels, TTL
│  ├─ plugins      # external plugin execution
│  └─ ingest       # pipeline that composes everything + masking
├─ apps/desktop/
│  ├─ src-tauri/   # Tauri 2 backend (clipboard monitor + commands)
│  ├─ ui/          # Leptos/WASM frontend (standalone crate, built with Trunk)
│  └─ legacy-ui/   # Original vanilla UI, kept as reference
└─ apps/mobile/android/
   ├─ app/          # companion app + keyboard (IME), Kotlin
   │  └─ assets/dict/   # bundled word lists (es)
   ├─ storage/      # encrypted SQLite history, Kotlin (moving to Rust)
   └─ rust-bridge/  # lapacho-core + lapacho-predict for Android via uniffi
```

`lapacho-core` does not depend on Tauri or any UI framework: it is reusable
from any frontend.

### Codebase map

A generated knowledge graph lives at `.ua/knowledge-graph.json`: 310 nodes and
558 edges across 9 architectural layers (security core, word prediction, mobile
Rust bridge, desktop backend, desktop UI, Android app, tests and payloads,
documentation, workspace config), plus an 18-step guided tour. The tour walks
the security flow end to end (capture → type classification → sensitivity
classification → masking → persistence policy → release on paste), then the
Android keyboard, the uniffi bridge and word prediction. It is meant as the
fastest way in for anyone new to the project.

Explore it with the [understand-anything](https://github.com/Egonex-AI/Understand-Anything)
plugin:

```bash
/understand-dashboard    # interactive graph viewer
/understand              # refresh after changes (incremental)
```

The dictionary word lists are left out of the analysis (`.ua/.understandignore`):
they are data, 50 000 lines each.

The graph is a derived artifact: if it ever contradicts the code, the code wins.

## Mobile

On Android, Lapacho is a keyboard (IME) rather than a background monitor:
Android only lets the active keyboard read the clipboard, so a clip is
captured when the keyboard opens and pasted by tapping it in the strip above
the keys. The app has no internet permission, and the history is encrypted
with a key in the Android Keystore and excluded from backups.

Secrets are never stored there: a clip is a secret if the password manager
flagged it or if `lapacho-core`'s classifier recognizes it. It can still be
pasted, from the clipboard itself, through a masked 🔑 chip. In password
fields and incognito tabs the history is hidden.

The keyboard itself is deliberately small: Spanish layout with a dead-key
acute, ñ, `@` and `¿ ? ¡ !` on long press, a numbers/symbols layer —
opened by itself in numeric fields (amounts, PINs, phone numbers, dates), kept
while you type in it, back to the letters in the next text field —
a fixed emoji layer — no swipe typing, and no autocorrect that rewrites a word
behind your back. From the second letter of a
word the strip offers up to three completions from a bundled dictionary
(`lapacho-predict` over 49 525 Spanish words, accent- and case-insensitive),
and gives the clips back when the word ends. When nothing completes the
word, it offers **corrections** instead (`graicas` → gracias, `thnaks` →
thanks) — offered in the strip, applied only if tapped. A suggestion comes
with its space, and a `, . ? ! : ;` typed right after takes that space's place
(`hola, ` rather than `hola ,`). Other languages and custom word
lists are imported from a file and **mixed** with Spanish, no switch key
([`docs/DICTIONARIES.md`](docs/DICTIONARIES.md)).

**It only learns what it is handed.** The dictionary is fixed and identical
for everyone who has it; nothing typed is stored, counted or modelled. The one
exception is deliberate: hold a word the dictionary does not know and a chip
offers to learn it, one word per press. What is kept is that word and nothing
around it, in an encrypted list the app shows in full and can forget entry by
entry. There is no background language model, and dictionaries for other
languages are imported from a file rather than downloaded — the keyboard has
no network permission and is not getting one. The reasoning for all three is
in [`docs/DECISIONS.md`](docs/DECISIONS.md).

**The interface is in English and Spanish**, separately from the dictionaries:
the app and the keyboard's labels follow the phone's language (Spanish if it is
any Spanish, English otherwise), and on Android 13+ Lapacho alone can be set in
Settings → Apps → Lapacho → Language. The texts live in
`res/values/strings.xml` and `res/values-es/strings.xml`; another language is
one more `values-xx/` folder and a line in `res/xml/locales_config.xml`.

Build notes and known gaps: [`apps/mobile/android/README.md`](apps/mobile/android/README.md).

## Security Model

- The `raw_content` (original content) lives **only in the backend**; the UI
  receives a `UIClipboardItem` that never includes it.
- Credentials and secrets are masked before display. The mask is **not** fully
  opaque, deliberately: a secret shows as `••••last4` and a credential as
  `first3…last4`, so you can tell two of them apart in a list of twenty. The
  full value and its length are never shown, but those few characters are. If
  that trade is wrong for your threat model, it is one function —
  `ingest::sensitive_display`.
- Plugins receive their input via `stdin` (not arguments → no injection) and run
  with a timeout; their output is sanitized before reaching the UI.

### Content at rest, and content in memory

These are different problems and it is worth being precise about which is
solved.

**At rest** — history is encrypted with AES-256-GCM in SQLite. The key lives in
the OS keyring (file fallback) and is `mlock`-ed so it cannot be swapped. The
encryption boundary is the storage layer: `save` encrypts, `load` decrypts, and
everything above that line works with plaintext.

**In memory** — the live session buffer necessarily holds plaintext. It has to:
the app searches it, renders it, and pastes it back. Encrypting it in RAM with
the key sitting next to it would be theatre. What can be done, and is:

- The payload is **scrubbed on eviction** (`zeroize`) across every field that
  carries it — not just `raw_content`, but `display_content` (which *is* the
  payload for non-sensitive items) and any user-set `title`.
- The payload is held in a **page-locked arena** (`LockedRing`) that the kernel
  may not swap: 25 slots of 32 KB, 800 KB locked once at startup and never
  grown. Scrubbing RAM accomplishes nothing if a copy reached the swap device
  first, and swap is very often not encrypted.

What that does **not** cover, stated plainly:

- **Images are not locked.** A 32 KB slot is sized for text (across a real
  history, the largest text item was 10.7 KB); images average ~1 MB and would
  require locking >100 MB permanently. Oversized items stay in the buffer
  unlocked rather than being dropped.
- **Copies handed to the UI are not locked.** Rendering a list or a tray menu
  builds ordinary `String`s. Those are short-lived; the copy the arena protects
  is the one that sits idle for hours, which is the one the kernel picks to
  swap.
- **The lock is best-effort.** If `RLIMIT_MEMLOCK` forbids it the app starts
  anyway and says so in the log — a clipboard manager that refuses to start is
  worse than one that reports it could not lock. Check the startup line rather
  than assuming.

Locking the *whole process* with `mlockall` was tried and reverted: under
WebKit it kills the app. The measurement and the conditions under which it
would be worth revisiting are in [`docs/DECISIONS.md`](docs/DECISIONS.md).

### Keyring path

The encryption key lives in the OS keyring:

- **Linux:** D-Bus Secret Service (`org.freedesktop.secrets`) in the `default`
  keyring, path `~/.local/share/keyrings/`. The service name is
  `digital.quebracho.lapacho`, entry name `history-encryption-key`.
- **macOS:** Keychain (Apple-native backend).
- **Windows:** Credential Manager (Windows-native backend).
- **Fallback:** If no keyring is available (headless, server, CI), the key is
  stored in `history.key` (0600 permissions) in the app data directory.

The keyring path is not testable headless; verify on a machine with a desktop
session. The key is never plaintext on disk (except in the fallback file, which
is 0600).

## Development

Requirements: Rust ≥ 1.85. For the desktop app, the Tauri 2 toolchain
(on Linux: `gtk3`, `webkit2gtk-4.1`, `libsoup-3.0`).

```bash
# Core tests
cargo test -p lapacho-core

# Build the entire workspace
cargo build --workspace

# Run the desktop app
cd apps/desktop/src-tauri
# trunk rejects NO_COLOR=1 (it wants true/false); if your shell exports it, drop it:
env -u NO_COLOR -u CARGO_TERM_COLOR TRUNK_COLOR=always CARGO_TERM_COLOR=never \
  cargo tauri dev
```

To install it for real (binary, icons and desktop entry under `~/.local`, no root):

```bash
./install.sh
```

### Diagnostics

Launched from autostart there is no terminal, so `install.sh` writes a desktop
entry that redirects output to a log. The previous run is kept, because the run
you need to read is usually the one that just died:

```
~/.local/state/lapacho/lapacho.log      # current run
~/.local/state/lapacho/lapacho.log.1    # previous run
```

The log is quiet by default: startup state (including whether the session
buffer actually got locked) and anything that went wrong. No clipboard content
is ever written to it — only errors, content types and timings. Keep it that
way if you add a diagnostic; the file is unencrypted, which is precisely what
the session buffer is not allowed to be.

Per-capture timings are off by default, since they fire on every copy. Turn
them on for a session:

```bash
LAPACHO_TRACE=1 lapacho
```

or add `LAPACHO_TRACE=1` to the `Exec` line in
`~/.local/share/applications/lapacho.desktop` to keep them across restarts.

## Roadmap

- [x] `lapacho-core`: classification, sanitization, storage, plugins, ingest
- [x] Desktop backend: monitor + Tauri commands + keyring + hardening
- [x] Reactive UI refresh on live capture
- [x] At-rest encryption (AES-256-GCM) + key in OS keyring + memory hardening
- [x] Session buffer plaintext held in page-locked memory (`LockedRing`, 800 KB)
      + scrubbed on eviction across every field that carries the payload
- [x] Diagnostics that survive autostart (log file + `LAPACHO_TRACE`)
- [ ] Tray rebuild latency (~290 ms/capture): `get_tray_items` decrypts 100 rows
      on every rebuild, and the native menu is rebuilt whole
- [x] Modular threat scanner + per-item actions (copy/export/plugin)
- [x] Leptos/WASM frontend + rich rendering (Markdown, safe SVG, JSON, Mermaid)
- [x] Native system tray + global shortcut (Ctrl+Shift+Alt+L, Lapacho-exclusive) + launch-to-tray + dynamic tray indicator icon (shows last image thumbnail)
- [x] Full image support (capture, tray thumbnails, metadata sanitization)
- [x] Android spike: paste-strip keyboard, encrypted shared history, desktop
      classifier via uniffi, secrets never stored (see [Mobile](#mobile))
- [x] Android: word suggestions from a bundled dictionary, with no user model
      (`crates/lapacho-predict`), and learning one word at a time by an
      explicit press, listed and reversible in the app
- [x] Android: dictionaries for other languages and custom word lists,
      imported from a file and mixed ([`docs/DICTIONARIES.md`](docs/DICTIONARIES.md))
- [x] Android: spell correction, offered in the strip and never applied by
      itself (`Predictor::correct`)
- [x] Android: interface in English and Spanish, following the phone or the
      per-app language setting
- [ ] Android: storage, keyed ids and persistence levels in Rust
      ([`docs/MIGRACION_MOBILE_RUST.md`](docs/MIGRACION_MOBILE_RUST.md))
- [x] Custom app icon (artistic design: Argentine blue halo + dark green hexagon + lapacho leaf as circuit with golden nodes; source in `icons/lapacho-source.svg`)

Full details and minor pending items: see [`ROADMAP.md`](ROADMAP.md). What was
evaluated and rejected, with the evidence: [`docs/DECISIONS.md`](docs/DECISIONS.md).

## License

The core (`lapacho-core`) and the app are released under **MIT OR Apache-2.0**.
Quebracho Digital's proprietary plugins and integrations are closed source.
See [`LICENSING.md`](LICENSING.md).
