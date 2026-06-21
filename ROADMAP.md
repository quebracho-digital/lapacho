# Lapacho — Roadmap

Lapacho: gestor de portapapeles seguro (Tauri 2 + Rust). Core libre; clasifica,
sanea y enmascara el contenido antes de que llegue a la UI. **El `raw_content` es
la fuente de verdad y no se pierde nunca:** se copia y se manda a los plugins
intacto, se *renderiza* saneado, y al exportar se avisa de amenazas sin alterarlo.

## Estado actual (2026-06-19)

### ✅ `lapacho-core` (lib, 45 tests entre unit + integración)

- `types` — `ClipboardItem` (raw) / `UIClipboardItem` (proyección segura) + enums.
- `detectors` — clasificación de tipo (texto/url/json/svg/mermaid/markdown).
- `security` — saneo (texto + SVG) y clasificación de sensibilidad (regex + entropía).
- `ingest` — pipeline que compone todo + enmascarado para display.
- `storage` — **abstracción `HistoryRepo`** (trait) sobre SQLite (WAL); niveles de
  persistencia, **TTL de sensibles configurable** (`RetentionPolicy`), cap de tamaño
  y **dedup por contenido** (recopiar mueve al tope, no duplica — sin hash en claro).
- `crypto` — **AES-256-GCM en reposo**; `SecretKey` (zeroize) + `Cipher` residente
  (boxed, zeroize, mlock opcional vía feature `mlock`).
- `threats` — **escáner modular** (trait `Detector` + `REGISTRY`): active-content,
  embedded-frame, trojan-source-bidi, zero-width, control-chars, sensitive-data,
  prompt-injection. Agregar un filtro = una struct + una línea.
- `plugins` — ejecución de comandos externos por stdin, con timeout.

### ✅ Backend de escritorio (`apps/desktop/src-tauri`)

- Monitor de portapapeles (`arboard`, 500 ms) → `process_text` → `HistoryRepo`.
- Clave de cifrado en **keyring del SO** (Secret Service / Keychain / Credential
  Manager) con fallback a archivo `0600` y migración automática.
- Endurecimiento en memoria: `mlock` de la clave + sin core dumps (Linux `prctl`).
- Comandos: `get_history`, `delete_item`, `clear_history`, `get/set_persist_level`,
  `get/set_sensitive_ttl`, `copy_item` (raw), `export_item` (raw + amenazas),
  `list_plugins`, `run_plugin` (opera sobre raw; la salida se guarda como ítem nuevo).
- Evento en vivo `clipboard-new` (refresco reactivo — el bug viejo del `://` está
  resuelto).

### ✅ Frontend (`apps/desktop/ui` — Leptos 0.7 / WASM, build verificado con Trunk)

- Crate **standalone** (excluido del workspace; Trunk lo compila a wasm32). No
  depende de `lapacho-core`: **tipos espejo** de la proyección segura.
- Bindings tipados sobre `window.__TAURI__` (`invoke` / `listen`).
- Lista en vivo, copiar/borrar/limpiar, selector de persistencia y de TTL.
- Acciones por ítem: **maximizar** (vista completa), **exportar** (muestra el raw
  + las amenazas detectadas antes de grabar), **enviar a plugin**.
- La UI vanilla original se conserva como referencia en `apps/desktop/legacy-ui/`.

## Pendientes

### 🎨 Frontend / UX

- [x] **Bandeja del sistema (menú nativo) + atajo global (Ctrl+Shift+V), lanzando
  solo a tray** (`src-tauri/src/tray.rs`). El listado es un menú nativo del
  indicador (fluido, no webview), reconstruido debounced en cada cambio. *Falta
  probar en GUI real.* Diferido: auto-paste (`enigo`), icono por ítem (con
  imágenes), y el popup-en-el-cursor (el menú nativo ya da la fluidez).
- [x] Render por `detected_type` en el modal de maximizar (toggle Raw/Vista):
  **SVG** (vía `<img data:>`, no innerHTML — más seguro), **Markdown**
  (`pulldown-cmark`), **JSON** (pretty), **Mermaid** (diagrama vivo con
  `mermaid.min.js` vendorizado, strict, degrada a código). Falta probar en GUI.
- [x] **Render rico en la lista** (parcial, Markdown): preview mini-renderizado de MD
  directamente en los ítems de la lista en vivo (usando truncate + render_markdown + inner_html
  en .md-mini). SVG/Mermaid siguen pendientes para lista (solo en modal). Ver `app.rs`.
- [ ] **Modal maximizar a pantalla completa**: diagramas/imágenes/SVG deben usar
  el espacio disponible del modal (hoy CSS limita a `40–50vh` y no es
  redimensionable). Ver `apps/desktop/ui/index.html` (`.mermaid-wrap`,
  `.image-view`, `.svg-preview`).
- [ ] Búsqueda / filtrado del historial.
- [x] Soporte de imágenes: captura `get_image()`, PNG data-URL + thumbnail 18×18
  (`src-tauri/src/images.rs`), `<img>` en lista/modal y **icono por ítem en el
  tray**, pegar de vuelta con `set_image`. Falta probar en GUI.
- [ ] Optimizar el wasm para release (`trunk build --release`; hoy ~2.9 MB sin
  optimizar tras sumar markdown/json/base64). Aparte: `mermaid.min.js` ~3.2 MB es
  un asset JS separado (no entra al wasm).

### 🔐 Seguridad

- [ ] Reemplazar el saneador SVG por regex por un parser real (ammonia) — TODO en
  `security.rs`.
- [ ] Más detectores en `threats::REGISTRY` (SQL injection, XSS avanzado) — el
  registro modular ya lo soporta sin tocar `assess()`.
- [ ] Verificación en máquina real de keyring + mlock (no testeable headless).

### 📦 Proyecto

- [ ] `LICENSE-APACHE` (copia del texto estándar) antes de publicar — el dual
  `MIT OR Apache-2.0` ya está declarado y `LICENSE-MIT` existe.
- [ ] Decidir si versionar `apps/desktop/src-tauri/gen/` (capabilities generadas).

## No usamos

- **Nada de Diodon** ni de otros gestores GTK/Vala: arquitectura propia (Rust +
  Tauri 2, core sin UI). Lo único conceptualmente comparable es "lista de recientes
  en la bandeja", que se implementará con la API de tray de Tauri.
