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
- [x] **Título + pin por item** (2026-07-28): 🏷 pone un nombre editable inline (Enter guarda,
  Esc cancela), cifrado en disco igual que el contenido, e incluido en `search()` — se encuentra
  el item por lo que *es*, no por lo que dice. 📍/📌 lo marca persistente: exento del cap de
  100 items de `cleanup()`, y sin consumir slot (pinnear no desaloja al resto). Título y pin son
  independientes a propósito: *pin sin título* es el caso común ("esto no lo pierdas").
  **El pin NO es un override de `PersistLevel`**: en modo Paranoia un item sensible nunca llegó
  al disco, así que pinnearlo solo lo sostiene en memoria esta sesión; y un `Credential`/`Secret`
  pinneado **igual expira por TTL** — esa garantía no es una preferencia del usuario.
  Cierra el debate de "pin / guardar en historial" de `docs/ARQUITECTURA_REFACTOREO.md:542`.
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

### 🐛 Bugs & Reactivity (HIGH PRIORITY — do in a dedicated session)
- [x] **SVG classification + capture** (false positive Personal on coords, plus HTML clipboard extraction for browsers) — fixed via `classify_sensitivity_graphics` + `looks_like_phone` + `svg_from_html` in monitor. SVG renders in list (thumb) and modal via safe `<img data:image/svg+xml;base64>`. Needs final GUI sign-off on copies from editors/browsers (use test-payloads/good-svg-test.svg).
- [x] **Reactivity + consistency + identity** (HIGH) — re-audited 2026-07-20, 3 of 4 symptoms already resolved by prior work:
  - Duplicate secret at top: fixed — `item.id = repo.content_id(&item.raw_content)` (stable keyed hash, even for non-persisted Paranoia items) + dedup-by-id in `tray_recent_push_front`/UI merge.
  - Tray showing only this-launch items: fixed — `get_tray_items` merges `tray_recent` + persisted DB history.
  - Hint disappearing: fixed — every projection (`UIClipboardItem::from`) recomputes `••••last4` from raw for Credential/Secret, so a reload can't clobber it with a stale `display_content`.
  - Tray update latency: mitigated (80ms debounce) but `rebuild` was decrypting the DB twice per cycle (`build_menu` + `tray_icon_for_top` each called `get_tray_items`/`repo.load()` independently) — fixed by loading history once in `rebuild`/`init` and threading the slice into both. (2026-07-20)
- [x] Distinguish sensitive/secret items: both Credential and Secret now render with short safe hint `••••last4` (from raw) so different tokens are identifiable (list, tray, modal). Tray always recomputes preview from raw_content even for old persisted items. Updated in `mask_display`, `UIClipboardItem::from`, `item_label` + tests. (2026-06-22)

### 🔐 Security

- [ ] **(ALTA) Bajar el nivel de persistencia no purga lo que el nivel nuevo prohíbe.** El nivel
  solo gobierna escrituras *nuevas*: pasar a Paranoia no borra lo que Balanced/All ya escribió, y
  con `sensitive_ttl_secs=off` nada lo purga después — el TTL es el único mecanismo que lo haría.
  Resultado: credenciales y secretos en disco mientras la UI dice "Paranoia". No es fuga de texto
  plano (sigue cifrado con la master key); el problema es que la etiqueta te hace creer que ya no
  están. Encontrado 2026-07-29 en la DB real: 22 sensibles en disco (5 Secret, 1 Credential,
  16 Personal) con `persist_level=none`. Documentado en `CHANGELOG.md` con el SQL para revisar y
  limpiar. **Fix**: al bajar de nivel, ofrecer purgar lo que el nivel nuevo prohíbe (respetando
  pinned/vaulted, que son deliberados). Decidir si se ofrece o se hace solo.
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
- [ ] More detectors (advanced XSS beyond `<script>`/handlers/`javascript:`) —
  the modular registry already supports this without touching `assess()`.
- [ ] Real-machine verification of keyring + mlock (not testable headless).

### 📦 Project

- [x] `LICENSE-APACHE` — full standard text present with copyright line filled in.
- [ ] Decide whether to version `apps/desktop/src-tauri/gen/` (generated capabilities).
- [ ] **Mobile (Android-first)** — design in `docs/ARQUITECTURA_MOBILE_ANDROID.md` +
  `docs/DEBATE_ARQUITECTURA_MOBILE.md`. Decided: **no fork** of an existing
  keyboard (FlorisBoard/HeliBoard ruled out); IME positioned as a
  "paste keyboard" (KeePassDX Magikeyboard pattern), HeliBoard read only as a
  lifecycle/accessibility reference. P0 spike scaffolded at
  `apps/mobile/android/` (Kotlin-only: `storage` module with SQLite +
  Keystore AES-GCM, one `:app` module with companion `MainActivity` + IME
  service in its own `:ime` process) — **written blind, not yet built or run**
  (no Android SDK/NDK in this environment; needs verification on a machine
  with Android Studio). No Rust/uniffi bridge yet — that's P1.
- [ ] **Multi-client sync (optional, E2E, per-item)** — design: `docs/ARQUITECTURA_MOBILE_ANDROID.md` §5
  (engine, hybrid topology, pairing, Authentik). Own thin `lapacho-sync` (not CRDT/Syncthing vault);
  hybrid SER5 store-and-forward + LAN/WG direct; Brave-like chain pair (QR/words) for decrypt keys;
  Authentik optional for relay authz only. Per-item `sync_eligible`; secrets iff paranoia allows.
  Companion/desktop network only. Not started (P4).

**Recent decision:** Global shortcut changed from Ctrl+Shift+V (too common in terminals, editors, browsers) to **Ctrl+Shift+Alt+L** (Lapacho-exclusive). Updated in code, README, HANDOFF, ROADMAP and CONTEXT.

## We do not use

- **Nothing from Diodon** or other GTK/Vala managers: our own architecture (Rust +
  Tauri 2, UI-free core). The only conceptually comparable thing is "recent list
  in the tray", which is implemented using Tauri's tray API.
