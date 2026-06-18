# Lapacho — Roadmap

Lapacho: gestor de portapapeles seguro (Tauri 2 + Rust). Core libre; clasifica,
sanea y enmascara el contenido antes de que llegue a la UI. `raw_content` nunca
sale del backend salvo al copiar explícitamente.

## Estado actual (2026-06-18)

- ✅ **`lapacho-core`** (lib, 19 tests): `types`, `detectors` (tipo de contenido),
  `security` (saneo + clasificación de sensibilidad por entropía/regex),
  `storage` (SQLite + niveles de persistencia + TTL), `plugins` (ejecución
  externa por stdin), `ingest` (pipeline que compone todo + enmascarado).
- ✅ **Backend `apps/desktop/src-tauri`**: monitor de portapapeles (`arboard`,
  500 ms) → `process_text` → `storage`; 8 comandos Tauri; evento `clipboard://new`.
- ✅ **UI placeholder** (`dist/index.html`) funcional: lista, copiar/borrar,
  selector de persistencia.

## Pendientes

### 🐛 Bugs

- [ ] **La lista no se actualiza reactivamente.** Al copiar algo nuevo, el ítem
  se captura y **se persiste** (aparece al forzar un refresh — p. ej. cambiando
  el modo de persistencia, que llama a `get_history`), pero el evento en vivo
  `clipboard://new` no refresca la UI.
  Captura + persistencia funcionan; falla solo la entrega/manejo del evento.
  Investigar, en orden:
  1. Confirmar que el listener JS recibe el evento (`console.log` en el handler
     de `listen("clipboard://new", …)`).
  2. Verificar que `app.emit(...)` desde el hilo del monitor llega al webview
     (el `let _ =` silencia cualquier error — loguearlo temporalmente).
  3. Posible carrera: el listener se registra después de los primeros emits.
  4. Probar un nombre de evento sin `://` (p. ej. `clipboard-new`) por si la
     validación de nombres de Tauri 2 lo descarta.

### 🎨 Frontend

- [ ] Reemplazar el placeholder por **UI Leptos/WASM** (estructura `apps/desktop/`
  + `src/`, como RustyBoard). El crate WASM **no puede depender de `lapacho-core`**
  (arrastra `rusqlite`/C); definir structs espejo livianos de `UIClipboardItem`.
- [ ] Render por `detected_type`: SVG inline (saneado), Markdown, JSON con formato,
  preview de Mermaid.

### 🔐 Seguridad

- [ ] **Cifrado en reposo (AES-256-GCM)** para los ítems sensibles del SQLite;
  hoy `storage` guarda en claro. Crítico en modo "Balanceado" (persiste
  credenciales con TTL).
- [ ] Reemplazar el saneador SVG basado en regex por un parser real (ammonia) —
  ya marcado como TODO en `security.rs`.

### ⌨️ UX / sistema

- [ ] Bandeja del sistema + atajo global (Ctrl+Shift+V) + popup en la posición
  del cursor (lanzar solo a tray, sin ventana principal).
- [ ] Soporte de imágenes en el portapapeles (thumbnail RGBA 18×18 ya previsto
  en `ClipboardItem.thumbnail`).
- [ ] Búsqueda/filtrado en el historial.

### 📦 Proyecto

- [ ] Llenar `README.md` y `LICENSING.md` (hoy vacíos).
- [ ] Decidir si versionar `apps/desktop/src-tauri/gen/` (capabilities generadas
  por `tauri-build`) o gitignorearlo.
