# Lapacho — Roadmap

Lapacho: secure clipboard manager (Tauri 2 + Rust). Libre core; classifies,
sanitizes and masks content before it reaches the UI. **The `raw_content` is
the source of truth and is never lost:** it is copied and sent to plugins
intact, *rendered* sanitized, and on export threats are reported without
altering it.

## Current Status (2026-07-20)
History search implemented; English translation complete. Sensitivity: long hex strings (>=32) now Secret; alphanumeric passwords without a special char now classify correctly. User-taught secrets (🔒 button, exact + prefix-pattern learning). Both Credential/Secret get short safe hints (`••••last4` from raw) for distinction (list/tray/modal); tray always derives preview from raw (even old DB items). Tray merges recent + persisted DB history; rebuild now loads history once per cycle instead of twice. Wayland: event-driven `wl-paste --watch` (no constant poll). Poll 250ms + 80ms debounce. Threat registry now includes SQL injection. 67 tests passing. ~93%.

### ✅ `lapacho-core` (lib, 45 unit + integration tests)

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
- [x] **Título + pin por item** (2026-07-28): 🏷 pone un nombre editable inline (Enter guarda,
  Esc cancela), cifrado en disco igual que el contenido, e incluido en `search()` — se encuentra
  el item por lo que *es*, no por lo que dice. 📍/📌 lo marca persistente: exento del cap de
  100 items de `cleanup()`, y sin consumir slot (pinnear no desaloja al resto). Título y pin son
  independientes a propósito: *pin sin título* es el caso común ("esto no lo pierdas").
  **El pin NO es un override de `PersistLevel`**: en modo Paranoia un item sensible nunca llegó
  al disco, así que pinnearlo solo lo sostiene en memoria esta sesión; y un `Credential`/`Secret`
  pinneado **igual expira por TTL** — esa garantía no es una preferencia del usuario.
  Cierra el debate de "pin / guardar en historial" del registro de diseño interno.
  Tags múltiples: no, hasta que buscar por título se quede corto. *Falta prueba GUI real.*
- [x] **Bóveda por item** (2026-07-28, decisión explícita del usuario): 💾 fuerza a disco un item
  que el `PersistLevel` activo rechaza, y lo exime del TTL de sensibles. **Es la única
  perforación deliberada de la política de persistencia**, y solo se abre por item y por acto
  explícito. Activar no puede ser un `UPDATE` (en Paranoia el item nunca se escribió): pasa por
  `save()` con el flag ya puesto. Desactivar **re-aplica el nivel activo en el acto** — si el
  nivel lo prohíbe, se borra del disco ya, no en el próximo `cleanup()`. En items sensibles el
  botón se arma primero (💾 → ⚠ → confirma) para que un secreto no quede en disco por un click
  de más. `PersistLevel::persists()` es la única fuente de la regla, usada por `save` y por el
  des-vaulteo, para que no diverjan. Las etiquetas del selector dicen "salvo bóveda": el modo ya
  no promete lo que no cumple. *Falta prueba GUI real.*
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

- [x] **(ALTA) Bajar el nivel de persistencia no purga lo que el nivel nuevo prohíbe.** El nivel
  solo gobierna escrituras *nuevas*: pasar a Paranoia no borra lo que Balanced/All ya escribió, y
  con `sensitive_ttl_secs=off` nada lo purga después — el TTL es el único mecanismo que lo haría.
  Resultado: credenciales y secretos en disco mientras la UI dice "Paranoia". No es fuga de texto
  plano (sigue cifrado con la master key); el problema es que la etiqueta te hace creer que ya no
  están. Encontrado 2026-07-29 en la DB real: 22 sensibles en disco (5 Secret, 1 Credential,
  16 Personal) con `persist_level=none`. Documentado en `CHANGELOG.md` con el SQL para revisar y
  limpiar. **Resuelto 2026-07-29**: `HistoryRepo::purge_forbidden` + llamada en `set_persist_level`.
  Se hace solo, no se ofrece: elegir Paranoia *es* la confirmación, y un diálogo que se puede
  cancelar deja el estado que el usuario cree haber descartado. Respeta `vaulted` (override
  explícito por item) pero **no** `pinned` — pinear exime del tope de tamaño, nunca del nivel ni
  del TTL. Volver a elegir el nivel actual re-aplica la purga, que es la vía de limpieza para
  quien ya tiene historial viejo.
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
- [ ] Decide whether to version `apps/desktop/src-tauri/gen/` (generated capabilities).
- [ ] **Mobile (Android-first)** — design in `docs/ARQUITECTURA_MOBILE_ANDROID.md` +
  the internal mobile design debate. Decided: **no fork** of an existing
  keyboard (FlorisBoard/HeliBoard ruled out); IME positioned as a
  "paste keyboard" (KeePassDX Magikeyboard pattern), HeliBoard read only as a
  lifecycle/accessibility reference. P0 spike scaffolded at
  `apps/mobile/android/` (Kotlin-only: `storage` module with SQLite +
  Keystore AES-GCM, one `:app` module with companion `MainActivity` + IME
  service in its own `:ime` process) — **written blind, not yet built or run**
  (no Android SDK/NDK in this environment; needs verification on a machine
  with Android Studio). P1 done: `rust-bridge` exposes `MobileCore` (uniffi).
- [ ] **Mobile: delete the Kotlin `storage` module** — `HistoryRepo.kt` + `Types.kt`
  (225 lines) duplicate `MobileCore` and have already diverged: no
  `title`/`pinned`/`vaulted`, no search, no `get_by_id`, and a plain SHA-256
  `contentId` instead of the keyed hash — so mobile and desktop compute
  *different ids for the same content*, which breaks sync dedup in P4.
  `LapachoCipher.kt` shrinks to Keystore key get/create + `wipeKey()`
  (a real Android API, the one thing Rust can't do); its encrypt/decrypt go to
  `lapacho-core`. Rule, per the internal mobile design debate and the
  same pattern Tauri's own mobile plugins use: **Kotlin only where an Android
  system API lives** (IME service, Activity, Keystore), everything else Rust.
  **Plan:** `docs/MIGRACION_MOBILE_RUST.md`. Requiere Android SDK/NDK. **Leo debe
  ejecutar** (no puedo probar sin SDK/NDK).
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
- [ ] **Multi-client sync (optional, E2E, per-item)** — design: `docs/ARQUITECTURA_MOBILE_ANDROID.md` §5
  (engine, hybrid topology, pairing, Authentik). Own thin `lapacho-sync` (not CRDT/Syncthing vault);
  hybrid self-hosted store-and-forward + LAN/VPN direct; Brave-like chain pair (QR/words) for decrypt keys;
  Authentik optional for relay authz only. Per-item `sync_eligible`; secrets iff paranoia allows.
  Companion/desktop network only. Not started (P4).

**Recent decision:** Global shortcut changed from Ctrl+Shift+V (too common in terminals, editors, browsers) to **Ctrl+Shift+Alt+L** (Lapacho-exclusive).

## We do not use

- **Nothing from Diodon** or other GTK/Vala managers: our own architecture (Rust +
  Tauri 2, UI-free core). The only conceptually comparable thing is "recent list
  in the tray", which is implemented using Tauri's tray API.
