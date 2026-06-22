# Lapacho — Roadmap

Lapacho: secure clipboard manager (Tauri 2 + Rust). Libre core; classifies,
sanitizes and masks content before it reaches the UI. **The `raw_content` is
the source of truth and is never lost:** it is copied and sent to plugins
intact, *rendered* sanitized, and on export threats are reported without
altering it.

## Current Status (2026-06-22)
History search implemented; English translation of docs + key source strings complete.

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

### 🐛 Bugs & Reactivity (HIGH PRIORITY — do in a dedicated session)
- [x] **SVG classification + capture** (false positive Personal on coords, plus HTML clipboard extraction for browsers) — fixed via `classify_sensitivity_graphics` + `looks_like_phone` + `svg_from_html` in monitor. SVG renders in list (thumb) and modal via safe `<img data:image/svg+xml;base64>`. Needs final GUI sign-off on copies from editors/browsers (use test-payloads/good-svg-test.svg).
- [ ] **Reactivity / live updates still not fully resolved** ("todavia no resolvimos la reactividad") — UI list and/or native tray sometimes miss new clipboard items or lag after copy/delete. Code uses `spawn_local` + `tray_recent` volatile buffer. Verify live in current running GUI.
- [x] Distinguish sensitive/secret items: Credentials now render as `••••` + safe last chars hint (e.g. `••••3456`) so different tokens are identifiable in list/tray. Secrets stay fully redacted (`••••••••`). Updated in `mask_display` + tests. (done while GUI was live 2026-06-22)

### 🔐 Security

- [x] Replace the regex SVG sanitizer with a real parser (`ammonia`) — done in
  `security.rs`. The old regexes are gone; we now use ammonia's HTML parser with a
  tight allow-list of SVG tags/attrs + restricted URL schemes. Test
  `sanitize_svg_removes_xss_vectors` passes, and the B1–B4 malicious payloads from
  `test-payloads/` can be re-tested live.
- [ ] More detectors in `threats::REGISTRY` (SQL injection, advanced XSS) — the
  modular registry already supports this without touching `assess()`.
- [ ] Real-machine verification of keyring + mlock (not testable headless).

### 📦 Project

- [ ] `LICENSE-APACHE` (standard text copy) before publishing — the dual
  `MIT OR Apache-2.0` is already declared and `LICENSE-MIT` exists.
- [ ] Decide whether to version `apps/desktop/src-tauri/gen/` (generated capabilities).

**Recent decision:** Global shortcut changed from Ctrl+Shift+V (too common in terminals, editors, browsers) to **Ctrl+Shift+Alt+L** (Lapacho-exclusive). Updated in code, README, HANDOFF, ROADMAP and CONTEXT.

## We do not use

- **Nothing from Diodon** or other GTK/Vala managers: our own architecture (Rust +
  Tauri 2, UI-free core). The only conceptually comparable thing is "recent list
  in the tray", which is implemented using Tauri's tray API.
