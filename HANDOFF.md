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
- ~~Icono por ítem (thumbnail 18×18) en el menú~~ → **hecho** con imágenes (#3).
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

### 3. Soporte de imágenes — ✅ HECHO (2026-06-19)

Procesamiento en el backend (`apps/desktop/src-tauri/src/images.rs`); core sigue
puro (sin deps de imagen). Deps nuevas en src-tauri: `image` (feat `png`),
`base64`, `uuid`.

- **Captura**: el monitor prioriza texto; si no hay, `clipboard.get_image()` →
  `process_image(w, h, rgba)`. Doble gate: hash de los **bytes RGBA** (barato,
  evita re-encodear cada 500 ms) + hash del **PNG base64** (determinístico, evita
  recapturar nuestro propio paste). El dedup por contenido de `storage` es la red
  de seguridad si el round-trip difiere.
- **`raw_content`** = PNG base64 crudo; **`display_content`** =
  `"data:image/png;base64,…"` → el front lo muestra con `<img>` (lista: thumbnail
  CSS; modal: grande). **`thumbnail`** = RGBA 18×18 base64 → **icono por ítem en
  el menú del tray** (`IconMenuItem` + `tauri::image::Image::new_owned`).
- **Pegar de vuelta** (`copy_raw`): si `content_type == "image"`, decodifica
  `raw_content` → `image::load_from_memory` → `arboard::set_image`.
- `sensitivity = None`, `detected_type = Text` (la UI/tray ramifican por
  `content_type == "image"`). El front muestra "Imagen" como etiqueta de tipo.
- **Export**: para imágenes devuelve el data-URL sin correr `assess` (no es
  texto). **Plugins**: rechazados sobre imágenes (operan sobre texto).
- Verificado: 2 tests en `images.rs` (roundtrip + dimensiones inválidas),
  clippy backend + wasm limpios, `trunk build` OK. **El render real lo probás vos
  en GUI.**

Pendiente menor: el tamaño del PNG en `display_content` puede inflar el SQLite
con `max_items=100` (hoy se guarda raw + display); evaluar deduplicar el blob.

### 4. Render por `detected_type` (frontend) — ✅ HECHO (2026-06-19)

Render rico **en el modal de "maximizar"**, con toggle **Raw/Vista** (la lista se
mantiene compacta y fluida). Solo se renderiza si el ítem **no es sensible**
(los sensibles llegan redactados). En `apps/desktop/ui/src/app.rs`:

- **SVG** → `<img src="data:image/svg+xml;base64,…">`. **Decisión de seguridad:**
  NO se usa `innerHTML` (como RustyBoard) sino `<img>`, así un script dentro del
  SVG no puede ejecutarse ni tocar el bridge de Tauri. Más seguro que RustyBoard.
- **Markdown** → `pulldown-cmark` (Rust→wasm). Se escapa el HTML crudo del origen
  y se neutralizan links `javascript:`/`data:` antes de inyectar (único path con
  `inner_html`).
- **JSON** → `serde_json` pretty-print (fallback al raw si no parsea).
- **Mermaid** → **diagrama vivo** (decisión de Leo). `mermaid.min.js` vendorizado
  en `apps/desktop/ui/vendor/` (UMD, ~3.2MB, `mermaid@10`), copiado por Trunk
  (`copy-file`) e iniciado con `securityLevel: "strict"`. El render se dispara con
  `request_animation_frame` tras montar el contenedor; `window.renderMermaid`
  (index.html) hace `mermaid.render` → SVG. **Degradación:** si falta el bundle,
  el contenedor sigue mostrando el código fuente. `extract_mermaid_code` pela el
  fence ```` ```mermaid ````.
  - ⚠️ **`vendor/mermaid.min.js` NO está versionado aún** (untracked; `dist/` sí
    está en `.gitignore`, `vendor/` no). Decidir: commitear el blob (~3.2MB, build
    offline reproducible) o gitignorearlo + script de descarga. Sin ese archivo,
    el diagrama no renderiza (cae a vista de código).
- **URL/Text** → texto plano (igual que RustyBoard).

Deps UI nuevas: `pulldown-cmark` (feat `html`, sin `getopts`), `serde_json`,
`base64`. Verificado: `cargo clippy --target wasm32` limpio + `trunk build` OK.
**El wasm dev subió 1.9→2.9 MB** → ver paso #8 (`--release` lo achica mucho).
Pendiente render inline en la lista (thumbnails) — va con imágenes (#3).

### 5. Búsqueda / filtrado del historial.

### Menores

- `LICENSE-APACHE` (copia del texto estándar) antes de publicar — el dual ya está
  declarado y `LICENSE-MIT` existe.
- Reemplazar el saneador SVG por regex con un parser real (ammonia) — TODO en
  `security.rs`.
- Más detectores en `threats::REGISTRY` (SQL injection, XSS avanzado) — el registro
  modular ya lo soporta sin tocar `assess()`.
- `trunk build --release` para optimizar el wasm (hoy ~2.9 MB sin optimizar;
  `mermaid.min.js` ~3.2 MB es asset JS aparte, no entra al wasm).
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
