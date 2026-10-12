# Lapacho — Roadmap

Lapacho: secure clipboard manager (Tauri 2 + Rust). Libre core; classifies,
sanitizes and masks content before it reaches the UI. **The `raw_content` is
the source of truth and is never lost:** it is copied and sent to plugins
intact, *rendered* sanitized, and on export threats are reported without
altering it.

## Current status (2026-10-11, `0.1.42`)

- **Desktop** (verified on Linux): history
  encrypted at rest, classification and masking, tray with global shortcut,
  persistence levels and per-item vault, plugins. Details below.
- **Android:** a "paste keyboard" with the history, prediction, correction
  and swipe in seven languages, explicit learning, plugin apps (*Lapacho
  Terms*). Since `763b72e` the history lives in `lapacho-core`, the same code
  as desktop. APKs signed with a release key, distributed without Google.
- **Sync:** the core works end to end over WebDAV (Rust, tested); no app
  uses it yet.

## Now, next, later

What to work on, in order. The sections below hold the detail.

- **Now:** publish the official dictionaries; test the Android build on a
  real Samsung; try the Fabric example plugins against a real model.
- **Next:** multi-client sync (P4) — it blocks every watch and every second
  device; then a Wear OS viewer on a Galaxy Watch5 Pro, the watch we have.
- **Later:** Lapacho Voice, *Lapacho Fabric* on Android, the terms
  catalogue, own F-Droid repository, Pebble and other open watches, iPhone
  (after deciding on Apple's distribution).

### ✅ `lapacho-core` (lib)

- `types` — `ClipboardItem` (raw) / `UIClipboardItem` (safe projection) + enums.
- `detectors` — type classification (text/url/json/svg/mermaid/markdown).
- `security` — sanitization (text + SVG) and sensitivity classification (regex + entropy).
- `ingest` — pipeline that composes everything + display masking.
- `storage` — **abstraction `HistoryRepo`** (trait) over SQLite (WAL); persistence
  levels, **configurable sensitive TTL** (`RetentionPolicy`), size cap and
  **content-based dedup** (recopying moves to top, no duplicate — no cleartext hash).
- `crypto` — **AES-256-GCM at rest**; `SecretKey` (zeroize) + resident `Cipher`
  (boxed, zeroize, optional mlock via `mlock` feature).
- `threats` — **modular scanner** (trait `Detector` + `REGISTRY`): active-content,
  embedded-frame, trojan-source-bidi, zero-width, control-chars, sensitive-data,
  prompt-injection. Adding a filter = one struct + one line.
- `plugins` — execution of external commands via stdin, with timeout; declared
  `params` passed as `LAPACHO_PARAM_*` environment variables; built-ins (search
  and replace) run as `lapacho plugin <id>`.

### ✅ Desktop backend (`apps/desktop/src-tauri`)

- Clipboard monitor (`arboard`, 500 ms) → `process_text` → `HistoryRepo`.
- Encryption key in **OS keyring** (Secret Service / Keychain / Credential
  Manager) with `0600` file fallback and automatic migration.
- Memory hardening: key `mlock` (`crypto.rs`) **and** session-buffer payloads in
  a page-locked arena (`locked_ring.rs`, 800 KB, text only — images do not fit a
  slot and stay unlocked), plus no core dumps (Linux `prctl`).
  Process-wide `mlockall` is **rejected**: under WebKit it kills the app, because
  `MCL_ONFAULT` controls page population and not accounting, so the kernel
  charges the multi-GB address space against `RLIMIT_MEMLOCK`.
- Commands: `get_history`, `delete_item`, `clear_history`, `get/set_persist_level`,
  `get/set_sensitive_ttl`, `copy_item` (raw), `export_item` (raw + threats),
  `list_plugins`, `run_plugin` (operates on raw, takes the plugin's params;
  output is stored as new item).
- Live event `clipboard-new` (reactive refresh — old `://` bug is resolved).

### ✅ Frontend (`apps/desktop/ui` — Leptos 0.7 / WASM, verified build with Trunk)

- **Standalone** crate (excluded from workspace; Trunk compiles it to wasm32). Does
  not depend on `lapacho-core`: **mirror types** of the safe projection.
- Typed bindings over `window.__TAURI__` (`invoke` / `listen`).
- Live list, copy/delete/clear, persistence and TTL selector.
- Per-item actions: **maximize** (full view, ‹ › to the previous/next clip),
  **export** (shows raw + detected threats before writing), **send to plugin**
  (with a field per declared param; a failure is shown next to Run).
- The original vanilla UI is preserved as reference in `apps/desktop/legacy-ui/`.

## Pending

### 🎨 Frontend / UX

- [x] **System tray (native menu) + global shortcut (Ctrl+Shift+Alt+L, Lapacho-exclusive), launching
  only to tray** (`src-tauri/src/tray.rs`). The list is a native indicator menu
  (fluid, no webview), rebuilt debounced on every change. The tray *indicator icon*
  now dynamically shows the thumbnail of the most recent (top) item when it is an
  image (falls back to default). Base app icon is a custom artistic design (Argentine
  blue halo + dark green hexagon + lapacho leaf/circuit + golden nodes) generated from
  `icons/lapacho-source.svg`. *Needs real GUI testing.* Deferred: auto-paste (`enigo`),
  cursor popup, and per-item icons for non-images. (dynamic + new artistic icon 2026-06-22)
- [x] Per-`detected_type` rendering in the maximize modal (Raw/View toggle):
  **SVG** (via `<img data:>`, not innerHTML — safer), **Markdown**
  (`pulldown-cmark`), **JSON** (pretty), **Mermaid** (live diagram with vendored
  `mermaid.min.js`, strict mode, degrades to source). Needs GUI testing.
- [x] **Rich rendering in the list** (improved): MD mini, now also SVG as <img> thumb (safe data url), JSON small pretty. Images use display PNG thumb + "Image (N bytes)" label with peso. Thumbnail + size fields now flow to UI. Mermaid source visible (full render heavy for list). See app.rs. (2026-06-22)
- [x] **Maximize modal uses more space**: widened modal (max 820px), raised preview limits to ~65-70vh for diagrams/images/SVG/MD/code (was 40-50vh). Full resizable/maximized would need additional UI (e.g. drag or dedicated window). See index.html. (2026-06-22)
- [x] History search / filtering: server-side (raw+display, case-insensitive) via `HistoryRepo::search` + tauri cmd + Leptos input. Secrets match on raw even when display is masked. Client list stays live. (2026-06-22)
- [x] Image support: capture `get_image()`, PNG data-URL + 18×18 thumbnail
  (`src-tauri/src/images.rs`), `<img>` in list/modal and **per-item icon in the
  tray**, paste back with `set_image`. Needs GUI testing.
- [x] Optimize wasm for release (`trunk build --release`): **~406 KB** (from ~2.9 MB dev unoptimized). JS glue ~37 KB. `mermaid.min.js` ~3.2 MB remains separate vendored asset. Release artifacts in `apps/desktop/ui/dist/`. Use for final `cargo tauri build`. (done 2026-06-22)
- [x] **Title + pin per item** (2026-07-28): 🏷 sets a name, edited inline
  (Enter saves, Esc cancels), encrypted on disk like the content and included
  in `search()` — an item is found by what it *is*, not by what it says.
  📍/📌 makes it persistent: exempt from `cleanup()`'s 100-item cap, and it
  takes no slot (pinning evicts nothing). Title and pin are independent on
  purpose: *a pin without a title* is the common case ("don't lose this").
  **A pin does NOT override `PersistLevel`**: in Paranoia a sensitive item
  never reached the disk, so pinning only holds it in memory this session;
  and a pinned `Credential`/`Secret` **still expires by TTL** — that
  guarantee is not a user preference. Closes the "pin / keep in history"
  debate from the internal design log. Multiple tags: no, until searching by
  title falls short. *Needs a real GUI test.*
- [x] **Per-item vault** (2026-07-28, an explicit user decision): 💾 forces to
  disk an item the active `PersistLevel` rejects, and exempts it from the
  sensitive TTL. **It is the only deliberate hole in the persistence policy**,
  opened per item and by an explicit act. Turning it on can't be an `UPDATE`
  (in Paranoia the item was never written): it goes through `save()` with the
  flag already set. Turning it off **re-applies the active level at once** —
  if the level forbids the item, it is deleted from disk now, not at the next
  `cleanup()`. On sensitive items the button arms first (💾 → ⚠ → confirm),
  so a secret doesn't land on disk from one click too many.
  `PersistLevel::persists()` is the rule's only source, used by `save` and by
  un-vaulting, so they can't diverge. The selector's labels say "except
  vault": the mode no longer promises what it doesn't do. *Needs a real GUI
  test.*
- [x] **Desktop: "About…" in the tray** — "About Lapacho…" opens the
  toolkit's About dialog (GTK `AboutDialog` on Linux, a message box on
  Windows, the standard panel on macOS): version, licence, authors, links to
  the repo and its releases. Verified on Linux by clicking it over dbusmenu.

### 🐛 Bugs & Reactivity (HIGH PRIORITY — do in a dedicated session)
- [x] **SVG classification + capture** (false positive Personal on coords, plus HTML clipboard extraction for browsers) — fixed via `classify_sensitivity_graphics` + `looks_like_phone` + `svg_from_html` in monitor. SVG renders in list (thumb) and modal via safe `<img data:image/svg+xml;base64>`. Needs final GUI sign-off on copies from editors/browsers (use test-payloads/good-svg-test.svg).
- [x] **Reactivity + consistency + identity** (HIGH) — re-audited 2026-07-20, 3 of 4 symptoms already resolved by prior work:
  - Duplicate secret at top: fixed — `item.id = repo.content_id(&item.raw_content)` (stable keyed hash, even for non-persisted Paranoia items) + dedup-by-id in `tray_recent_push_front`/UI merge.
  - Tray showing only this-launch items: fixed — `tray_recent` is seeded from the persisted history once at startup (`SessionBuffer::seed`); rebuilds stay memory-only. (Regressed 2026-08-05 by the tray perf change, restored 2026-09-28.)
  - Hint disappearing: fixed — every projection (`UIClipboardItem::from`) recomputes `••••last4` from raw for Credential/Secret, so a reload can't clobber it with a stale `display_content`.
  - Tray update latency: mitigated (80ms debounce) but `rebuild` was decrypting the DB twice per cycle (`build_menu` + `tray_icon_for_top` each called `get_tray_items`/`repo.load()` independently) — fixed by loading history once in `rebuild`/`init` and threading the slice into both. (2026-07-20)
- [x] Distinguish sensitive/secret items: both Credential and Secret now render with short safe hint `••••last4` (from raw) so different tokens are identifiable (list, tray, modal). Tray always recomputes preview from raw_content even for old persisted items. Updated in `mask_display`, `UIClipboardItem::from`, `item_label` + tests. (2026-06-22)

### 🔐 Security

- [x] **(HIGH) Lowering the persistence level didn't purge what the new
  level forbids.** The level only governed *new* writes: switching to
  Paranoia didn't delete what Balanced/All had written, and with
  `sensitive_ttl_secs=off` nothing purged it later — the TTL was the only
  mechanism that would. Result: credentials and secrets on disk while the UI
  said "Paranoia". Not a plaintext leak (still encrypted with the master
  key); the problem is a label that makes you believe they're gone. Found
  2026-07-29 in the real DB: 22 sensitive items on disk (5 Secret, 1
  Credential, 16 Personal) with `persist_level=none`; `CHANGELOG.md` has the
  SQL to review and clean them. **Fixed 2026-07-29**:
  `HistoryRepo::purge_forbidden`, called from `set_persist_level`. It just
  happens, it isn't offered: choosing Paranoia *is* the confirmation, and a
  dialog that can be cancelled leaves the state the user believes they
  discarded. It respects `vaulted` (an explicit per-item override) but **not**
  `pinned` — a pin exempts from the size cap, never from the level or the
  TTL. Choosing the current level again re-applies the purge, which is the
  way to clean an old history.
- [x] Replace the regex SVG sanitizer with a real parser (`ammonia`) — done.
- [x] Long hex classification as Secret + distinction hints (see above).
- [x] Password-heuristic gap: classifier required all 4 char classes
  (digit+upper+lower+special) at entropy > 3.8, missing ordinary alphanumeric
  passwords, whose max entropy can't reach that bar anyway. Relaxed to
  digit+upper+lower with entropy > 3.4; special char no longer required. (2026-07-19)
- [x] **User-taught secrets**: 🔒 button per item marks it Secret and teaches
  Lapacho for next time — exact match via `content_id` (keyed hash, no
  plaintext stored), plus structural generalization: `secret_prefix()`
  (`lapacho-core::security`) detects token shape (literal prefix + random
  tail, e.g. `acme_live_…`) and learns the prefix, so *different* future
  values with the same shape classify Secret from capture. Command
  `mark_secret` in `main.rs`; prefixes stored in the `settings` table. (2026-07-19)
- [x] SQL injection detector added to `threats::REGISTRY` (tautologies,
  `UNION SELECT`, stacked statements, comment terminators after a quote).
  One struct + one registry line, per the existing extension pattern. (2026-07-20)
- [x] More detectors (advanced XSS beyond `<script>`/handlers/`javascript:`) —
  added 3 detectors: DataUrlScript (detects `data:text/html,<script>` and base64 SVG with `<script>`),
  SvgEventHandler (detects `<svg>` with `onload`/`onerror`/other handlers), ImgOnError (detects `<img>` with `onerror`).
  4 new tests, 102 total passing.
- [x] Real-machine verification of the session-buffer lock: `VmLck` 808 kB with
      `VmSwap` 0, launched via the installed desktop entry (2026-08-04).
- [x] Real-machine verification of the keyring path (not testable headless).
      Linux: `org.freedesktop.secrets` via D-Bus; path `~/.local/share/keyrings/`.
      Verified: key stored in `default` keyring with label `lapacho`.
- [x] Paste-back through the tray GUI (2026-08-04). Implemented in `tray.rs`:
      click handler → `copy_raw` → `load_item` (checks `tray_recent` first, then DB).
      `state.last_seen` prevents re-capture. Manual test: copy something, click tray
      item → paste. Works end-to-end from locked arena.

### 🧩 Plugins

- [ ] **Read the terms before accepting them** (idea and plan, 2026-10-06) — turns a terms-and-conditions or privacy-policy text into a
  short report: what data is collected, shared or sold and with whom,
  retention, auto-renewal and how to cancel, arbitration and class-action
  waivers, jurisdiction, changes without notice, the licence over your
  content. Each point quotes the clause it comes from. Saved as a new clip;
  the original stays. **Not legal advice**, and the report says so.
  - [x] **First step, built (`0.1.39`, untested on a phone):** the built-in
    plugin `terms` (`lapacho-core/src/terms.rs`) turns a clip into a request
    for the user's own assistant: the 19 categories and the severity scale
    of `terms-eval`, quotes asked verbatim, the text fenced with
    `spotlight_text`. On Android it opens the share sheet and leaves the
    request first in the strip; on desktop it is a new clip to paste.
  - [x] **A model you run (`0.1.40`, untested on a phone):** desktop through
    the example plugin `terms-assistant`; Android through *Lapacho Terms*,
    a separate plugin app that holds the network permission Lapacho and its
    keyboard don't have. Both reach an OpenAI-compatible server or a Drupal
    site with `ai_provider_universal_terms`. Tested on desktop against
    llama.cpp on minisforum (Qwen3.6-35B-A3B, 58 s for Netflix's terms).
  - **Where an analysis comes from, most private first:**
    1. **The catalogue.** The terms of the most used sites (global plus
       Argentina, Spain and LatAm), analysed in advance and **downloaded
       whole**, like the dictionaries: one versioned file, looked up on the
       device by domain and by hash of the normalized text. Nobody learns
       which one you looked at.
    2. **Not in the catalogue:** a local model (desktop, or the phone if one
       fits), or the phone's own assistant through the share sheet. Holding
       a clip offers "Analyse terms…": a fixed prompt plus the text, fenced
       with `spotlight_text`, sent with `ACTION_SEND`. On desktop it is
       copied to the clipboard. The user picks the app every time, and
       Lapacho can't check that app's quotes.
    3. **Later, and only if the numbers work:** a hosted service. First a
       beta on minisforum for trusted testers, behind Authentik; then rented
       GPUs fit for production. It is looked up by hash prefix (the
       Have I Been Pwned pattern: the server can't tell which document), and
       a new document's analysis is stored by document hash, never with the
       account, with no request logs kept. Blind tokens (Privacy Pass) could
       let it check payment without knowing who paid.
  - **Keeping the catalogue current is the real cost.** A scheduled job (on
    SER5) re-fetches each catalogued URL, normalizes and hashes the text, and
    re-analyses only what changed. It keeps every version, which makes
    *"what changed since you accepted"* a feature: the old and new clauses
    side by side. A new catalogue goes out as a release, like a dictionary.
    The catalogue's quotes are short excerpts; check how quoting stands
    legally before publishing it.
  - **When it runs:** Lapacho never sees the page with the "I accept" box,
    only what gets copied. So it runs by hand on any clip, or the classifier
    recognises a clip shaped like terms (length, legal headings, "Terms of
    Service" / "Términos y condiciones") and *offers* it. It never runs by
    itself. A screen-watching accessibility service is out.
  - **Models, under one rule: OSI-permissive licences only** (Apache-2.0,
    MIT), irrevocable and with no acceptable-use policy the vendor can
    rewrite later. Licences checked on Hugging Face, 2026-10-06:
    - Teachers (big, on minisforum): Qwen3.5 / Qwen3.6 35B-A3B (Apache-2.0),
      Mistral Small 3.2 24B (Apache-2.0, FR), GLM-4.5-Air (MIT).
    - Students (small, for a phone or a laptop): Salamandra 2B / 7B (BSC,
      Apache-2.0, strong Spanish), EuroLLM 1.7B / 9B (Apache-2.0, EU),
      Ministral 3 3B / 8B (Apache-2.0), Qwen3.5 0.8B / 2B / 4B (Apache-2.0),
      SmolLM3 3B (Apache-2.0). Gemma 4 E2B / E4B are Apache-2.0 too, but last
      in line: Gemma 1–3 shipped under a licence Google could tighten.
    - Out: Gemma ≤ 3 (Gemma licence), Llama (Meta community licence),
      Ministral 2410 (MRL, research only), LFM2 (own licence), and community
      fine-tunes of unclear origin or trained on another vendor's outputs
      (the "Claude Opus distilled" Qwen on minisforum included).
  - **Data:** UNFAIR-ToS from LexGLUE (CC-BY-4.0, attribution), CodeHima
    TOS_Dataset (MIT), LegalBench (CC-BY-4.0, but each task carries its own
    licence). OPP-115 is out until its licence is confirmed: the HF mirror
    declares none, and CMU hands it out for research. A Spanish set of our
    own: about 20 real terms (Mercado Libre, banks, Rappi, WhatsApp…), the
    texts kept out of git (they are copyrighted), the labels in.
  - **The path:**
    1. **Measure:** the evaluation set against the teachers and students
       above, on minisforum's llama.cpp. Recall per kind of clause, quotes
       that really are in the text, time and RAM. This decides whether any
       small model is good enough as it is.
    2. **Distil, only if none is:** the teacher labels thousands of clauses,
       answers with invented quotes are dropped, and a LoRA goes on the best
       student. Train on a few hours of rented GPU (ROCm on the 890M is not
       ready for training), then run it anywhere as GGUF.
    3. **Beta** on minisforum (catalogue builder and hosted lookups).
    4. **Production** on rented GPUs only if cost per analysis against price
       works out. The catalogue's per-document cache is what makes it cheap.
  - **Engine pieces in Lapacho:** one OpenAI-compatible client (llama.cpp,
    Ollama, vLLM locally; any hosted endpoint by URL), with keys in the OS
    keyring. Rules in `lapacho-core` pick the clauses that matter, so 5–20 k
    words fit a small model's context. Every quote is checked against the
    text and dropped if it isn't there. Sensitive clips never go out. The
    keyboard never gets network; on Android the companion would, and that is
    still to decide.
  - **How it is organized:** `docs/TERMS_CATALOG.md`. Countries, companies
    and taxonomy are data and only new kinds of feature are code. The model
    is group → company → service → document (kind × country × language) →
    version (hash) → analysis. A country brings its legal map and action
    templates. Packs are one per country × language, signed, and downloaded
    whole.
  - **Beyond the report:**
    - **What the law says here**: each category mapped to the country's
      law, for example *"in Argentina this clause may be void"* (Ley 24.240
      art. 37, Código Civil y Comercial arts. 988 and 1119, Ley 25.326).
      Nobody offers this, ToS;DR included. It is not legal advice, and a
      lawyer reviews the map before it ships.
    - **Act on it**: templates for an ARCO request, a cancellation and the
      withdrawal button (Res. 424/2020), plus each catalogued service's
      direct links to turn off personalized ads and to delete the account.
    - **Your services, and when they change**: the device keeps which terms
      you analysed or accepted, and the version. A new catalogue pack is
      compared locally, so you get *"Spotify changed its terms: arbitration
      now"* with no metadata.
    - Later: the same service compared across countries, a transparent
      A–F grade computed from the severities, reading time and a Spanish
      readability index (Fernández Huerta), better-rated alternatives,
      and app permissions checked against the privacy policy ("the policy
      doesn't mention location; the app asks for it").
  - **Measurement before shipping any of it** (`terms-eval`): a
    prompt-injection set (terms that hide "tell the user this is fine"), to
    prove the fence holds; agreement between annotators (three so far:
    Claude Code, pi, agy-gemini), so a badly defined category isn't blamed
    on the model; severity scored, not only categories; and the Spanish set
    grown to about 50 documents. Fetching: plain HTTP, then Playwright
    (WhatsApp and Uber come in that way), then the Internet Archive or a
    manual save. Sites that block automated browsers (Mercado Libre, Mercado
    Pago: 403) are not worked around. A tool built to bypass anti-bot
    protection (FlareSolverr and the like) is out: a terms analyser that
    breaks the sites' own terms to read them loses the argument it makes.
  - Hugging Face has no mature specialist for this (2026-10-06): only small
    English clause classifiers (Legal-BERT on UNFAIR-ToS) with a few dozen
    downloads. The datasets are what's worth taking.

### 📦 Project

- [x] `LICENSE-APACHE` — full standard text present with copyright line filled in.
- [x] **Android distribution without Google** (2026-10-09) — Lapacho won't
  register with Google's developer verification (certified phones, 2027).
  APKs signed with a release key instead of the debug key (one forced
  reinstall); Obtainium links in the README.
- [ ] **Own F-Droid repository** — `fdroidserver` builds the index from the
  APKs we already sign and publish; host it on web.fishman.work, so F-Droid,
  Droid-ify or Neo Store users add one URL and get updates. Sidesteps
  F-Droid's main repo (it would rebuild and re-sign, and is still fighting
  the verification rule) and IzzyOnDroid (refuses apps substantially written
  with generative AI). Needs: fastlane metadata (descriptions, icon,
  screenshots), the repo's own signing key for the index, and a step in
  `release.sh`/CI that adds each release's APKs and re-signs the index.
- [ ] Decide whether to version `apps/desktop/src-tauri/gen/` (generated capabilities).
- [x] **Mobile (Android-first)** — design in `docs/ARQUITECTURA_MOBILE_ANDROID.md`.
  Decided: **no fork** of an existing keyboard (FlorisBoard/HeliBoard ruled
  out); the IME is a "paste keyboard" (KeePassDX Magikeyboard pattern),
  HeliBoard read only as a lifecycle/accessibility reference. Built and
  released as APKs; `rust-bridge` exposes `MobileCore` (UniFFI).
  Builds run on minisforum, where the Android SDK/NDK live.
- [x] **Mobile: delete the Kotlin `storage` module** (2026-10-09) — gone; the
  app's `History.kt` wraps `MobileCore` and the old store is imported once.
  It duplicated `MobileCore` and had diverged (no title/pin/vault, no search,
  a plain SHA-256 `contentId`, so phone and desktop computed *different ids
  for the same content*). Rule: **Kotlin only where an Android system API
  lives** (IME service, Activity, Keystore), everything else Rust — see
  `docs/DECISIONS.md`. Plan as it was: `docs/MIGRACION_MOBILE_RUST.md`.
- [ ] **Mobile: predictive keyboard** — design in
  `docs/ARQUITECTURA_MOBILE_ANDROID.md` §6, decisions in `docs/DECISIONS.md`.
  Done: `lapacho-predict` (prefix completion over a word list, accent- and
  case-insensitive), exposed as `WordPredictor` through the uniffi bridge,
  bundled Spanish dictionary (49 525 words, `assets/dict/es.txt`, MIT) and the
  suggestion strip in the IME, which takes over the paste strip while a word
  is being typed. Pending, in order:
  - [x] **Spell correction** (`0.1.24`) — `Predictor::correct`: optimal
    string alignment distance (a swap of two letters is one edit), one edit
    away or two if nothing is one away, ranked by distance then frequency,
    same first letter only. A known word is corrected only towards one ×1000
    more frequent (the subtitle lists carry typos like `qeu`). A 32-bit
    letter-set mask bounds the distance from below and skips most of the
    scan: 0.03–1.3 ms on the laptop, ≤ 6.3 ms per keystroke on the emulator
    with completion included. The IME asks only when nothing completes the
    word, and offers — never applies. 23 real typos in
    `crates/lapacho-predict/tests/real_typos.rs`. Not done: two words run
    together (`porfavor`), a mistyped first letter.
  - [x] **Other languages and custom lists** (`0.1.20`) — imported through the
    system file picker (**IDIOMAS** in the companion), validated by format
    (magic header, safe `#lang`, UTF-8, ≤ 8 MB) rather than an allow-list of
    hashes, so custom dictionaries work. All active dictionaries are mixed,
    each normalized to its own corpus; up to two imported. A header can add
    long-press alternates. Format and recipes: `docs/DICTIONARIES.md`. No
    network permission, by decision.
  - [x] **Official vs custom** (`0.1.21`) — hashes of published dictionaries
    in the APK; a match imports silently, anything else asks first, per file.
  - [x] **Swipe typing** — `Predictor::swipe` (SHARK2, location channel)
    over the same dictionaries; the IME's letter rows read the gesture and
    offer the runners-up in the strip. Tuned on synthetic swipes only:
    `SWIPE_SIGMA` wants retuning against real ones from a phone, and the
    shape channel is the next step if swipes drawn small or off-centre fail.
  - [ ] **Publish official dictionaries** next to the APKs on `/lapacho`
    — English, Italian, Hebrew (`0.1.37`), Brazilian Portuguese, French,
    German and Russian are committed, built by `dictionaries/build.py`, and
    official in the app; every release attaches them and `publish.sh`
    uploads them. Arabic waits for an RTL strip and folded harakat.
  - [x] **Explicit learning** — long-press the word you typed → a chip offers
    to learn it → stored as one word in the encrypted store (`lexicon` table,
    DB v2), listed and deletable under **PALABRAS** in the companion. Never
    automatic, never from a private field, never for what the classifier reads
    as a secret. A learned word outranks the dictionary; the keyboard picks up
    what the app forgot the next time it opens.
- [x] **Mobile: words in one flat buffer, then lift the language cap** — every
  key and word now lives in one `String`, each entry 16 bytes of offsets,
  lengths, frequency and letter mask (it was 48 plus two allocations), and a
  word with no accent or capital is stored once. es + en on the laptop: heap
  6.0 → 2.1 MB for 0.71 MB of words, build 50 → 45 ms, corrections unchanged,
  swipe ~15 % faster; `tests/memory.rs` holds the heap under 3× the words.
  `MAX_LANGUAGES` 3 → 6. Not measured on a phone yet.
- [ ] **Mobile: a layout per language, and switching between them** — layouts
  and 🌐 done (below); other scripts and RTL pending.
  - [x] **Layout from the language:** `#rows azertyuiop qsdfghjklm wxcvbn` in
    the header (2–4 rows, ≤ 12 lowercase letters each); none = the built-in
    QWERTY, whose middle row ends in the active dictionaries' `#keys` (ñ for
    Spanish). The ´ dead key is gone (`0.1.38`): accents are on each vowel's
    long press, as on Gboard, SwiftKey and AOSP's Spanish layout. The keyboard opens on the first dictionary's layout and
    then keeps the last one picked.
  - [x] **Switch layouts, not languages.** Languages that share a layout (es, en,
    pt on QWERTY — ñ is a long press, not another keyboard) stay mixed as
    today: writing in either needs no switch. The 🌐 key (next to ?123)
    cycles the *layouts* among the active languages, and only shows when
    there is more than one; a sideways swipe on the space bar is not done. Suggestions, corrections and
    swipe use every dictionary whose layout is on screen — swipe already reads
    the real key positions.
  - **Other scripts:** Hebrew without niqqud works (`0.1.37`: official
    `he.txt` with its own `#rows`; `fold()` maps the five final letters to
    their regular forms, and swipe tells ם and מ apart as two keys). Still:
    Russian too (ЙЦУКЕН rows, ё folds to е and is the long press on е; any
    lowercase letter can now take `#alternates`). `fold()` only
    strips Latin accents, so Hebrew niqqud and Arabic harakat aren't ignored
    when matching; `currentWord()` stops at a combining mark (`isLetter`), so
    a word typed with harakat is cut. Shift does nothing for scripts without
    case (fine), and the dictionary format is already plain UTF-8.
  - **RTL (Hebrew, Arabic):** typing works today as far as the field goes —
    Android lays out bidirectional text in the app, not the keyboard. A
    Hebrew/Arabic layout is a different key order, not a mirrored one, so the
    keys need no RTL handling; the suggestion strip should read right to left
    (best word on the right). The app doesn't declare `supportsRtl`, so its
    own screens stay left-to-right in an RTL locale.
  - **Out of scope here:** Chinese and Japanese are not a layout problem — they
    need a conversion engine (pinyin/kana → characters, with a candidate list)
    and have no spaces to find a word by. A separate project if ever.
- [ ] **Fabric patterns as plugins** (idea 2026-10-11) — summarize, extract
  wisdom, any pattern, run on a clip against the user's own model server.
  - [x] **Desktop** (2026-10-12): `fabric-summarize.json` and
    `fabric-pattern.json` in `plugins/examples/`, checked with a stand-in
    `fabric` (input, pattern, a hostile pattern value); **not yet run against
    a real model** — `fabric` isn't installed on the laptop. Was: a JSON
    definition that runs `fabric -p "$LAPACHO_PARAM_PATTERN"` with the clip on
    standard input (the contract already passes params as environment
    variables). Fabric uses its own model setup (`fabric --setup`).
    `timeout_secs` well above the default 30, and `fabric` must be on the
    `PATH` the autostarted app sees (`~/go/bin` usually isn't). One
    definition per favourite pattern (*Summarize*, *Extract wisdom*) beats
    typing a name each time: a typo only shows up as Fabric's error.
  - **Android:** a plugin app, *Lapacho Fabric*, the generalization of
    *Lapacho Terms*: same OpenAI-compatible server setting, the pattern's
    `system.md` as the system prompt, the clip as the user message. Patterns
    imported from a Fabric checkout like the dictionaries, not downloaded by
    Lapacho. *Analyse terms* could become one more pattern.
  - Masked clips never reach a plugin, so a pattern never sees a secret.
  - **Sovereign by default, an outside model only after a warning** — the
    rule for every plugin app that talks to a model (Terms, Fabric, Voice's
    server mode):
    - No server comes preconfigured; with none set the plugin does nothing.
    - A server on the phone itself, the LAN or the WireGuard range is
      treated as the user's. Any other address asks once, when it is saved:
      "is this server yours?" — the app can't tell a hostname the user runs
      from a provider's. A "no" shows the warning: the clip's text leaves
      for a third party, which may keep it, log it or train on it. It stays
      on until removed, and the plugin's screen marks every run that goes
      out.
    - Patterns are public text (Fabric is MIT): imported from a file or a
      Fabric checkout; the plugin app may fetch them from Fabric's
      repository on request, never Lapacho itself.
    - A model on the phone (llama.cpp, 1–4B) would close the loop with no
      server at all; measure speed, RAM and battery before promising it.
    - Desktop: the example uses Fabric's own model setting, which Lapacho
      can't see, so the warning is documentation there, not a check.
- [ ] **Speech to text, sovereign, in every language** (idea 2026-10-09,
  research the same day) — dictate instead of typing, recognized on the
  device or on your own server: no audio and no text go anywhere else.
  - **Lapacho Voice, a plugin app** like *Lapacho Terms* (decided): it holds
    `RECORD_AUDIO` and the models; the keyboard's 🎤 key hands over to it and
    types what comes back. The keyboard keeps no permission at all, and
    whoever doesn't want a microphone doesn't install it.
  - **The phone's own voice service, only if the user opts in**: a setting in
    Lapacho Voice, off by default, that says what it does — it opens
    Android's recognizer (`RecognizerIntent`), which sends the audio to
    whichever service the phone has, usually Google's.
  - **Rules:** hold-to-talk (records only while the key is down), audio only
    in memory and dropped after recognition, never in a password or private
    field, the text offered in the strip before it is typed.
  - **Engines, OSI-permissive only (MIT / Apache-2.0)**; licences checked on
    Hugging Face 2026-10-09:
    - **Qwen3-ASR 0.6B / 1.7B** (Apache-2.0, Jan 2026): 30 languages
      (es, en, it, pt, fr, de, ru, ar… — not Hebrew), offline and streaming,
      state of the art among open models by its report. The main candidate;
      **Qwen3-ForcedAligner 0.6B** (Apache-2.0) adds word timestamps in 11.
    - **Moonshine streaming** tiny / small / medium (MIT, 2026): built for
      live, on-device use; English plus es, de, ja, ar, zh, vi, tl. Only the
      2026 streaming models: the earlier non-English ones carry another
      licence.
    - **Voxtral Mini 4B Realtime** (Mistral, Apache-2.0, Feb 2026): 13
      languages, sub-200 ms. Heavy for a phone; fits a home server.
    - **Whisper large-v3-turbo** (MIT) and **ivrit-ai**'s Hebrew fine-tunes
      (Apache-2.0): the route for Hebrew.
    - Out, by licence: NVIDIA Parakeet / Canary and Kyutai STT (CC-BY-4.0),
      SenseVoice / Fun-ASR (own licence).
  - **Runtime: sherpa-onnx** (Apache-2.0, active, Android, Kotlin and Rust
    bindings): runs Qwen3-ASR, Moonshine, Whisper, voice activity detection
    and diarization, so one engine covers dictation and conversations.
    Models imported per language like the dictionaries (official hashes in
    the APK, no network in the app unless a server is configured).
  - **Conversations, several people talking** — a second mode of the same
    app, or its own: record, cut by voice activity, transcribe, tell the
    speakers apart, and save a transcript with speakers and timestamps as a
    clip.
    - On the phone: sherpa-onnx diarization with pyannote segmentation-3.0
      (MIT) and 3D-Speaker embeddings (Apache-2.0 toolkit; check each
      model). Not pyannote's WeSpeaker embeddings or the community-1
      pipeline (CC-BY-4.0), Rev's (non-commercial) or NVIDIA Sortformer.
    - On your own server, like Lapacho Terms: **VibeVoice-ASR** (Microsoft,
      MIT, Jan 2026, ~9B) transcribes, separates speakers and timestamps up
      to an hour in one pass; GGUF builds exist. Its 0.3B BitNet variant
      runs on edge CPUs but covers no Spanish and doesn't diarize.
    - Android: a long recording needs a foreground service with a visible
      notification. The audio is kept only if the user says so, encrypted
      like the history.
    - **Consent:** a visible "recording" state and a reminder to tell the
      others. Whether everyone must consent differs by country; check
      before shipping, and say it in the app.
  - **Measure first**, as with `terms-eval`: word error rate per language on
    Common Voice and FLEURS, diarization error on an open meeting set (AMI),
    latency, RAM and battery on a real phone, for es, en, it, he, pt-br, fr,
    de and ru. A language ships when its model clears a bar we set.
- [ ] **Multi-client sync (optional, E2E, per-item)** — design: `docs/ARQUITECTURA_MOBILE_ANDROID.md` §5
  (engine, hybrid topology, pairing, Authentik). Own thin `lapacho-sync` (not CRDT/Syncthing vault);
  hybrid self-hosted store-and-forward + LAN/VPN direct; Brave-like chain pair (QR/words) for decrypt keys;
  Authentik optional for relay authz only. Per-item `sync_eligible`; secrets iff paranoia allows.
  **It blocks every item below that crosses devices.**
  - [x] Core (2026-10-11): chain + 24 words, `may_sync` on
    `PersistLevel::persists`, sealed envelopes, WebDAV transport, expiry
    (a week; secrets no longer than on the sender's disk).
  - [ ] *Lapacho Sync* on Android (its own app, outbox and retry) and the
    desktop wiring.
  - [ ] Device revocation — needed in v1 for watches. Decided
    (`ARQUITECTURA_MOBILE_ANDROID.md` §5.8b): rotate the chain, the old
    folder as a quarantine holding *"chain is dead"*, full wipe only on
    devices that opted in, per-device WebDAV passwords.
  - [ ] Post-quantum, pending (`ARQUITECTURA_MOBILE_ANDROID.md` §5.9b):
    hybrid TLS to WebDAV; any public-key crypto added is hybrid ML-KEM /
    ML-DSA. The clips themselves are already safe (symmetric only).
- [ ] **Sovereign Lapacho on every phone and watch** (idea 2026-10-11) — the
  same client, local and encrypted, wherever the user's devices are.
  - **Blocked by sync.** No watch work starts until two real clients exchange
    an encrypted clip end to end (P4's exit criterion). A Wear OS viewer
    could talk to the phone over the Data Layer without sync, but then the
    watch protocol is designed before the sync one and the two have to be
    reconciled later; not worth it.
  - **First watch: Wear OS on a Galaxy Watch5 Pro** — the one we wear, so
    it gets tested on real hardware every day.
  - **A lost or stolen watch** — decided before the protocol, because it
    shapes it:
    - Clips live only in the watch's memory, never on its disk; the watch
      asks the phone each time it shows the list.
    - The watch forgets everything when it is unpaired, taken off the wrist,
      or out of the phone's reach for more than a few minutes.
    - "Forget this watch" on the phone revokes its pairing key, so a stolen
      watch that is still paired gets nothing more.
    - What the watch can ask for is capped by the paranoia level on the
      phone; the watch never decides what it may see.

  Order is by cost, cheapest first:
  - **Samsung, Pixel, any Android phone:** already the target, nothing new to
    build. Test on a real Samsung (One UI ships its own keyboard and
    clipboard, and its battery manager kills background apps harder than
    stock Android).
  - **Wear OS watches** (Pixel Watch, Galaxy Watch 4 and later, TicWatch,
    Oppo; Galaxy Watch 3 and older run Tizen and are out): an Android app,
    so Kotlin and the same Rust core. Not a keyboard — typing on a watch is
    Lapacho Voice's job. **A clipboard viewer that keeps the paranoia
    levels:** the phone decides what reaches the watch with the same rules
    as the sync above, and the watch shows secrets masked like everywhere
    else.
    - **Revealing a secret's full text** (reading a Wi-Fi password or a code
      off the wrist): only if the paranoia level allows secrets on the watch
      at all, only while the watch is locked to the wrist (Wear OS screen
      lock + off-wrist detection), on a tap, hidden again after a few seconds
      and on wrist-down. A watch face is read by whoever sits next to you.
    - **Plugins run on the phone, the watch triggers them:** a plugin takes a
      clip's text and saves a new clip, so the watch sends "run X on clip N"
      over the Data Layer and shows the result. The same plugins as on the
      phone, nothing to rewrite. A watch-only plugin only when it needs the
      watch itself (showing a clip as a QR code to scan from another device,
      say) — none planned yet.
    - **Dictation and a recorder on the watch are not plugins:** a plugin
      starts from a clip's text, and these start from audio and *make* a
      clip. They are Lapacho Voice's watch side: the watch records
      (hold-to-talk for dictation, a recording for a meeting), hands the
      audio to Lapacho Voice on the phone over the Data Layer, and the
      transcript comes back as a clip. Summarizing it or running a Fabric
      pattern on it is then an ordinary plugin (above), launched from the
      watch like any other. Long recordings drain a watch; the phone stays
      the better recorder for a meeting.
  - **iPhone:** a custom keyboard extension plus a container app, the core
    bound to Swift. A separate design (`ARQUITECTURA_MOBILE_ANDROID.md` P3.4):
    the extension needs "Allow Full Access" to read the pasteboard or share
    storage with the app, runs under a tight memory limit (tens of MB, the
    predictor's buffer has to fit), and iOS lets no app read the clipboard in
    the background, so the history only grows from what passes through the
    keyboard or the share sheet. **Distribution is the catch:** the App Store
    or the EU alternative marketplaces, both behind a paid Apple developer
    account and Apple's review or notarization — the opposite of the
    "without Google" stance on Android. Decide before writing code whether
    that is acceptable; a free account's sideload expires every 7 days.
    Building needs a Mac or a macOS CI runner.
  - **Apple Watch:** a watchOS viewer like the Wear OS one, after the iPhone.
    watchOS takes no third-party keyboards.
  - **Chinese watches:** most aren't programmable. Amazfit (Zepp OS) runs
    JavaScript mini-apps with an open SDK and developer-mode sideloading — a
    viewer is feasible. Huawei (HarmonyOS, ArkTS, AppGallery only) and
    Xiaomi (Vela quick apps) are closed stores; the cheap ones (Da Fit,
    FitCloudPro firmware) run no third-party code at all, only
    notifications. Out unless a user asks for a specific model.
  - **Open watches** (looked up 2026-10-11, not tried on hardware):
    - **Pebble** (Core Time 2, Core 2 Duo): PebbleOS, the phone app and the
      tools fully open since Nov 2025; has a microphone, so dictation fits.
      The most open target after Wear OS. Its speech recognition (Wispr
      Flow) is proprietary — we'd use Lapacho Voice's.
    - **Bangle.js 2:** Espruino, apps in JavaScript, hundreds of community
      apps; no microphone. A viewer is feasible.
    - **UNA:** repairable sports watch, own FreeRTOS-based OS, MIT SDK;
      Kickstarter 2025, shipping status unconfirmed. Watch it.
    - **PineTime (InfiniTime, GPLv3):** apps are compiled into the firmware,
      so supporting it means maintaining a firmware fork. Low priority.
    - **AsteroidOS:** Linux on old Wear OS watches, Qt/QML apps; few users.
    - **Notifications, outside Lapacho** (idea 2026-10-11): the same channel
      (the chain plus our own phone–watch link) could mirror any app's
      notifications to the watch and send replies back
      (`NotificationListenerService` + the notification's reply action),
      with no vendor app or Google Play services in between — what
      Gadgetbridge does for open watches, end to end encrypted. **A
      separate app, not Lapacho:** notification access reads every
      notification, 2FA codes included, and an app holding that plus a
      network or Bluetooth link is an exfiltration tool if compromised; it
      must not share a process or a permission set with the clipboard. It
      reuses `lapacho-sync`'s transports; replies typed by voice reuse
      Lapacho Voice. Check first: Android 15 redacts OTPs to listeners that
      aren't trusted, sideloaded apps need "allow restricted settings" before
      they can get notification access, and on a Galaxy Watch the official
      mirroring keeps running alongside unless turned off.
    - **One way in for all of them: a *Lapacho Watch* plugin app** on the
      phone holding the Bluetooth permission, so the keyboard keeps none,
      talking to a small Lapacho app on each watch. **Not through
      Gadgetbridge's broadcast intents:** a broadcast reaches any app on the
      phone, which is a clipboard leak.

## We do not use

- **Nothing from Diodon** or other GTK/Vala managers: our own architecture (Rust +
  Tauri 2, UI-free core). The only conceptually comparable thing is "recent list
  in the tray", which is implemented using Tauri's tray API.
