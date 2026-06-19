# HANDOFF — lapacho

Estado y próximos pasos para continuar el desarrollo (este agente, otro, o Leo).
Complemento accionable del [`ROADMAP.md`](ROADMAP.md): qué sigue, dónde y cómo.

Última actualización: 2026-06-19.

---

## Estado actual

Core + backend **completos y testeados**; frontend Leptos **compila**. Rama de
trabajo: `feat/avance-autonomo`.

- **`lapacho-core`** (45 tests): clasificación, saneo, ingest, `HistoryRepo`
  (SQLite WAL + persistencia + TTL configurable), **cifrado AES-256-GCM en
  reposo**, **escáner de amenazas modular** (`threats::Detector` + `REGISTRY`),
  plugins por stdin.
- **Backend** (`apps/desktop/src-tauri`): monitor de portapapeles, clave en
  **keyring del SO** (+ fallback archivo 0600), endurecimiento en memoria
  (mlock + sin core dumps), comandos completos. **Modelo raw-first**: raw es la
  fuente de verdad (copiar/plugins → raw; render → saneado; exportar → raw +
  aviso de amenazas).
- **Frontend** (`apps/desktop/ui`, Leptos 0.7/WASM): lista en vivo,
  copiar/borrar/limpiar, persistencia + TTL, y por ítem maximizar / exportar
  (raw + amenazas) / enviar a plugin. UI vanilla original en `legacy-ui/`.

## Cómo verificar (importante)

- **Core + backend** (host): `cargo build --workspace` · `cargo test --workspace`
  · `cargo clippy --workspace --all-targets`.
- **Frontend** (wasm): `cd apps/desktop/ui && trunk build` (+ `cargo clippy
  --target wasm32-unknown-unknown`). El toolchain está instalado (wasm32, trunk
  0.21, tauri-cli 2.11) → **se verifica headless que compila**, pero la GUI no se
  ve acá; eso lo prueba Leo.
- **Correr la app** (en máquina con webkit2gtk): `cd apps/desktop/src-tauri &&
  cargo tauri dev`.
- RTK reescribe comandos vía hook (transparente). `CONTEXT.md` es **compartido**
  (Claude Code + Grok); para no pisarse, cada uno edita **solo su parte** con
  reemplazos quirúrgicos (Grok → quebrachos/SER5; acá → lapacho). **RustyBoard y
  quebracho-client son obsoletos**: solo referencia, no trabajar ahí.

---

## Próximos pasos (orden de prioridad)

### 1. Bandeja del sistema con menú NATIVO — ✅ HECHO (2026-06-19)

Implementado en `apps/desktop/src-tauri/src/tray.rs` (+ cableado en `main.rs`).
El listado del tray es un **menú nativo del indicador** (no webview) → fluido.

- `TrayIconBuilder` (id `lapacho-tray`) + `MenuBuilder`/`MenuItem`,
  **reconstruido (debounced 150 ms)** en cada `clipboard-new`, en
  `delete_item`/`clear_history`/`run_plugin`. Los últimos **12** ítems como
  entradas nativas; texto a una línea, truncado a 50; sensibles ya llegan
  `••••••••` desde `display_content` + sufijo `[credencial]`/`[secreto]`.
- Click en ítem (id = uuid) → `copy_raw` (raw al portapapeles; ceba `last_seen`).
- Entradas estáticas: "Abrir Lapacho…" (muestra/enfoca ventana) y "Salir".
- Atajo global **Ctrl+Shift+V** (`tauri-plugin-global-shortcut`) togglea la
  ventana. Registrado y manejado **solo en Rust** → no requiere capability.
- App **lanza a tray**: ventana `visible: false` y cerrar = ocultar
  (`on_window_event` CloseRequested → `hide()` + `prevent_close`). Abrir con el
  atajo o "Abrir Lapacho…".
- Cargo: `tauri` con feature `tray-icon` + dep `tauri-plugin-global-shortcut`.
- Rebuild corre en el **main thread** vía `run_on_main_thread`; el debounce usa
  un thread + sleep (sin runtime async).

Verificado headless: `cargo check/clippy -p lapacho-desktop` limpio,
`cargo test --workspace` 45/45. **Falta probar en máquina con GUI** (Leo): que el
indicador sea visible (depende del icono embebido + extensión de appindicator en
GNOME) y la fluidez real del menú.

Diferido (eran "opcionales" en el plan original):
- **Auto-paste** tras copiar (crate `enigo`, ya en cache local) — simula Ctrl+V;
  necesita prueba por-plataforma.
- **Icono por ítem** (thumbnail 18×18) en el menú — llega con el soporte de
  imágenes (paso 3).
- **Popup en el cursor** (ventana pre-creada y oculta) como alternativa al menú
  nativo — el menú nativo ya cubre la fluidez; revisar si Leo lo quiere.

### 2. Dedup por contenido (core) — ✅ HECHO (2026-06-19)

Identidad de ítem = su **contenido**: recopiar un clip viejo lo **mueve al tope**
(bump de `timestamp`, conserva el id) en vez de duplicar. Implementado en
`SqliteRepo::save` (`storage.rs`): antes de insertar, `existing_id_for_content`
busca un ítem con el mismo `raw_content`; si existe, `UPDATE timestamp`.

**Decisión de seguridad:** se compara plaintext **descifrado** (el historial está
capado por `max_items`, escanearlo es barato), **no** un hash en claro — un hash
guardado permitiría confirmar un secreto por diccionario a quien tenga el `.db`,
debilitando el cifrado en reposo. Sin columnas nuevas, sin deps, sin migración.

Nota: cubre la vía del **monitor** (re-copiar desde el origen) y la salida de
plugins. Copiar desde la UI/tray (`copy_item`) sigue cebando `last_seen` y el
monitor lo saltea (anti-feedback), así que ese camino no re-ordena — si se quiere
que también suba al tope, hace falta un bump explícito de timestamp ahí.

Verificado: test `recopy_moves_to_top_instead_of_duplicating` + suite 46/46,
clippy limpio. (El fixture `dummy` ahora usa contenido único por id, porque con
dedup reusar un string colapsaría ítems distintos.)

### 3. Soporte de imágenes — PORTAR de RustyBoard

El schema ya está listo: `ClipboardItem` tiene `content_type` y
`thumbnail: Option<String>` (heredados de RustyBoard a propósito).

Deps a sumar al workspace: `image = { version = "0.25", default-features =
false, features = ["png"] }` (arboard y base64 ya están).

Approach (con refs a `RustyBoard/src-tauri/src/lib.rs`):

- **Captura**: en el monitor, branch `clipboard.get_image()` →
  `arboard::ImageData { width, height, bytes (RGBA) }`; dedup por hash de bytes
  (`last_image_hash`). → `process_image` (lib.rs:513).
- **`display_content`** = `"data:image/png;base64,…"` (PNG completo como data-URL)
  → el frontend lo muestra con `<img src=… />`. Encode: `encode_image_to_png`
  (lib.rs:433): `ImageBuffer<Rgba<u8>>::from_raw` → `write_to(Png)` → base64.
  Render frontend: `RustyBoard/src/app.rs:440`.
- **`thumbnail`** = base64 de un RGBA **18×18 crudo** (no PNG; `18*18*4` bytes),
  vía `image::imageops::resize(…, 18, 18, Lanczos3)` → `generate_image_thumbnail`
  (lib.rs:450). Sirve de **icono del tray** (`tauri::image::Image::new_owned`).
- **Pegar de vuelta**: data-URL → bytes → `image::load_from_memory` →
  `to_rgba8()` → `arboard::ImageData` → `clipboard.set_image()` (lib.rs:84).

Específico de lapacho (decisiones a tomar):

- `sensitivity = None` (no hay clasificación de texto en imágenes).
- `detected_type`: hoy `DetectedType` no tiene variante `Image`; o se agrega, o se
  distingue solo por `content_type == "image"` (como RustyBoard).
- **Cifrado**: el data-URL y el thumbnail se cifran como strings (el `Cipher` ya
  opera sobre cualquier String). Cuidado con el **tamaño**: un PNG completo en
  `display_content` puede inflar el SQLite con `max_items=100`. Evaluar guardar
  los bytes una sola vez (no duplicar en raw + display).
- **Amenazas / export**: `threats::assess` corre sobre texto. Para imágenes,
  saltear el escaneo (un data-URL base64 no tiene amenazas de texto) o tratarlo
  aparte. El "export" de una imagen es el data-URL.
- `run_monitor` hoy solo hace `get_text()`; sumar la rama `get_image()`.

### 4. Render por `detected_type` (frontend)

SVG inline (ya saneado en backend), Markdown, JSON formateado, preview de Mermaid.
RustyBoard tiene render por `content_type` en `src/app.rs` como referencia.

### 5. Búsqueda / filtrado del historial.

### Menores

- `LICENSE-APACHE` (copia del texto estándar) antes de publicar — el dual ya está
  declarado y `LICENSE-MIT` existe.
- Reemplazar el saneador SVG por regex con un parser real (ammonia) — TODO en
  `security.rs`.
- Más detectores en `threats::REGISTRY` (SQL injection, XSS avanzado) — el registro
  modular ya lo soporta sin tocar `assess()`.
- `trunk build --release` para optimizar el wasm (hoy 1.9 MB sin optimizar).
- Decidir si versionar `apps/desktop/src-tauri/gen/`.

---

## Inspiración de Diodon (resumen)

Se toma la **UX del tray** (menú nativo + dedup por hash + auto-paste opcional),
**no** la persistencia: Diodon usa **Zeitgeist** (log de actividad en claro,
compartido por el sistema, sin mantenimiento) — incompatible con la tesis de
lapacho (cifrado en reposo, retención/TTL deliberada). Tampoco se busca
"historial infinito": lapacho expira sensibles a propósito.

## Mapa de archivos

- core: `crates/lapacho-core/src/{types,ingest,security,detectors,storage,crypto,threats,plugins}.rs`
- backend: `apps/desktop/src-tauri/src/{main,keystore,tray}.rs` + `tauri.conf.json`
- frontend: `apps/desktop/ui/src/{main,app,bindings,types}.rs` + `index.html` + `Trunk.toml`
- referencia (obsoleta, no tocar): `/home/leo/Proyects/RustyBoard` (imágenes en `src-tauri/src/lib.rs`)
