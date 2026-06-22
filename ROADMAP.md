# Lapacho — Roadmap

Lapacho: secure clipboard manager (Tauri 2 + Rust). Libre core; classifies,
sanitizes and masks content before it reaches the UI. **The `raw_content` is
the source of truth and is never lost:** it is copied and sent to plugins
intact, *rendered* sanitized, and on export threats are reported without
altering it.

## Current Status (2026-06-19)

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
- `plugins` — execution of external commands via stdin, with timeout.

### ✅ Desktop backend (`apps/desktop/src-tauri`)

- Clipboard monitor (`arboard`, 500 ms) → `process_text` → `HistoryRepo`.
- Encryption key in **OS keyring** (Secret Service / Keychain / Credential
  Manager) with `0600` file fallback and automatic migration.
- Memory hardening: key `mlock` + no core dumps (Linux `prctl`).
- Commands: `get_history`, `delete_item`, `clear_history`, `get/set_persist_level`,
  `get/set_sensitive_ttl`, `copy_item` (raw), `export_item` (raw + threats),
  `list_plugins`, `run_plugin` (operates on raw; output is stored as new item).
- Live event `clipboard-new` (reactive refresh — old `://` bug is resolved).

### ✅ Frontend (`apps/desktop/ui` — Leptos 0.7 / WASM, verified build with Trunk)

- **Standalone** crate (excluded from workspace; Trunk compiles it to wasm32). Does
  not depend on `lapacho-core`: **mirror types** of the safe projection.
- Typed bindings over `window.__TAURI__` (`invoke` / `listen`).
- Live list, copy/delete/clear, persistence and TTL selector.
- Per-item actions: **maximize** (full view), **export** (shows raw + detected
  threats before writing), **send to plugin**.
- The original vanilla UI is preserved as reference in `apps/desktop/legacy-ui/`.

## Pending

### 🎨 Frontend / UX

- [x] **System tray (native menu) + global shortcut (Ctrl+Shift+V), launching
  only to tray** (`src-tauri/src/tray.rs`). The list is a native indicator menu
  (fluid, no webview), rebuilt debounced on every change. *Needs real GUI testing.*
  Deferred: auto-paste (`enigo`), per-item icon (with images), and cursor popup
  (the native menu already provides the fluidity).
- [x] Per-`detected_type` rendering in the maximize modal (Raw/View toggle):
  **SVG** (via `<img data:>`, not innerHTML — safer), **Markdown**
  (`pulldown-cmark`), **JSON** (pretty), **Mermaid** (live diagram with vendored
  `mermaid.min.js`, strict mode, degrades to source). Needs GUI testing.
- [x] **Rich rendering in the list** (partial, Markdown): mini-rendered MD preview
  directly in live list items (using truncate + render_markdown + inner_html in
  .md-mini). SVG/Mermaid still pending for the list (only in modal). See `app.rs`.
- [ ] **Full-screen maximize modal**: diagrams/images/SVG should use the available
  modal space (current CSS limits to `40–50vh` and is not resizable). See
  `apps/desktop/ui/index.html` (`.mermaid-wrap`, `.image-view`, `.svg-preview`).
- [ ] History search / filtering.
- [x] Image support: capture `get_image()`, PNG data-URL + 18×18 thumbnail
  (`src-tauri/src/images.rs`), `<img>` in list/modal and **per-item icon in the
  tray**, paste back with `set_image`. Needs GUI testing.
- [ ] Optimize wasm for release (`trunk build --release`; currently ~2.9 MB
  unoptimized after adding markdown/json/base64). Note: `mermaid.min.js` ~3.2 MB
  is a separate JS asset (not inside the wasm).

### 🔐 Security

- [ ] Replace the regex SVG sanitizer with a real parser (`ammonia`) — TODO in
  `security.rs`.
- [ ] More detectors in `threats::REGISTRY` (SQL injection, advanced XSS) — the
  modular registry already supports this without touching `assess()`.
- [ ] Real-machine verification of keyring + mlock (not testable headless).

### 📦 Project

- [ ] `LICENSE-APACHE` (standard text copy) before publishing — the dual
  `MIT OR Apache-2.0` is already declared and `LICENSE-MIT` exists.
- [ ] Decide whether to version `apps/desktop/src-tauri/gen/` (generated capabilities).

## We do not use

- **Nothing from Diodon** or other GTK/Vala managers: our own architecture (Rust +
  Tauri 2, UI-free core). The only conceptually comparable thing is "recent list
  in the tray", which is implemented using Tauri's tray API.
