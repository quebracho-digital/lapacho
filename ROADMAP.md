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
  persistencia, **TTL de sensibles configurable** (`RetentionPolicy`) y cap de tamaño.
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

- [ ] **Bandeja del sistema + atajo global (Ctrl+Shift+V) + popup en el cursor**,
  lanzando solo a tray. *Prioridad* — el listado del tray debe sentirse fluido
  (ventana pre-creada y oculta, no recrear en cada apertura; posicionar al cursor).
- [ ] Render por `detected_type`: SVG inline (saneado), Markdown, JSON formateado,
  preview de Mermaid.
- [ ] Búsqueda / filtrado del historial.
- [ ] Soporte de imágenes (thumbnail RGBA 18×18 ya previsto en `ClipboardItem`).
- [ ] Optimizar el wasm para release (`trunk build --release`; hoy 1.9 MB sin optimizar).

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
