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
  en `apps/desktop/ui/vendor/` (UMD, ~3.2MB), copiado por Trunk (`copy-file`) e
  iniciado con `securityLevel: "strict"`. El render se dispara con
  `request_animation_frame` tras montar el contenedor; `window.renderMermaid`
  (index.html) hace `mermaid.render` → SVG. **Degradación:** si falta el bundle,
  el contenedor sigue mostrando el código fuente. `extract_mermaid_code` pela el
  fence ```` ```mermaid ````.
  - `vendor/mermaid.min.js` **versionado en el repo** (decisión 2026-06-19):
    build offline reproducible, sin dependencia de CDN en CI ni en máquinas sin
    internet. Ver protocolo de actualización en §&nbsp;"Dependencias vendorizadas".
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

## Dependencias vendorizadas

Assets JS incluidos en el repo para builds offline reproducibles. Actualizar bajo
protocolo explícito; **no tocar sin seguir los pasos de verificación**.

| Asset | Versión | SHA-256 | Ruta |
|-------|---------|---------|------|
| mermaid.min.js | 3.4.2 | `eda3a0ad572bbe69a318c1be0163e8233dd824f3f12939e5168feba207767151` | `apps/desktop/ui/vendor/` |

### Cuándo revisar

- **Mensual** (primera semana): revisar si hay versión nueva.
- **Inmediato** si aparece un CVE que afecte XSS / parsing en Mermaid (este
  renderer recibe input directo del portapapeles).

### Chequear si hay actualización

```bash
# Versión latest en npm (no instala nada)
npm show mermaid version

# Changelog desde la versión actual:
# https://github.com/mermaid-js/mermaid/releases
```

Comparar con la versión registrada en la tabla de arriba.
Si hay versión nueva **y** el changelog no muestra breaking changes relevantes
(API de `mermaid.render()`, `securityLevel`, inicialización UMD), **esperar 2–3
semanas** antes de actualizar — salvo CVE activo. Ese tiempo deja que la
comunidad reporte regressions o problemas silenciosos antes de que los
absorbamos.

### Protocolo de actualización

```bash
# 1. Descargar el nuevo bundle
NEW=<VERSION>   # ej. 11.4.1
curl -fLo apps/desktop/ui/vendor/mermaid.min.js \
  "https://cdn.jsdelivr.net/npm/mermaid@${NEW}/dist/mermaid.min.js"

# 2. Verificar integridad
sha256sum apps/desktop/ui/vendor/mermaid.min.js
# Comparar con el hash publicado en el release de GitHub o en npm:
#   npm show mermaid@${NEW} dist.integrity    (formato sha512, alternativo)
# Si no coincide: ABORT y reportar.

# 3. Confirmar versión embebida
grep -oP 'version="\K[^"]+' apps/desktop/ui/vendor/mermaid.min.js | head -1
```

### Tests antes de commitear

#### A — Análisis estático del bundle (offline, antes de arrancar la app)

```bash
# 1. Integridad: SHA-256 contra el registrado en la tabla y contra npm
sha256sum apps/desktop/ui/vendor/mermaid.min.js
npm show mermaid@<VERSION> dist.shasum   # sha1 del tarball; cruzar también
#    con el hash del release de GitHub (Assets → mermaid.min.js)

# 2. Versión embebida: debe coincidir exactamente con lo que descargaste
grep -oP 'version="\K[^"]+' apps/desktop/ui/vendor/mermaid.min.js | head -1

# 3. Delta de tamaño: ±20 % del anterior es normal; más = investigar
wc -c apps/desktop/ui/vendor/mermaid.min.js

# 4. Strings prohibidos: ninguno de estos tiene lugar en un renderer de diagramas
grep -c '__TAURI__'           apps/desktop/ui/vendor/mermaid.min.js   # debe ser 0
grep -c 'document\.cookie'   apps/desktop/ui/vendor/mermaid.min.js   # debe ser 0
grep -c 'navigator\.sendBeacon' apps/desktop/ui/vendor/mermaid.min.js # debe ser 0
grep -c 'XMLHttpRequest'      apps/desktop/ui/vendor/mermaid.min.js   # debe ser 0 o mínimo (Mermaid 10+ no lo usa)
# Si __TAURI__ aparece → ABORT, no commitear, reportar supply-chain incident.
```

#### B — Build y suite automatizada

```bash
cargo test --workspace                                   # 46+ tests core+backend
cd apps/desktop/ui && trunk build                        # Trunk copia el asset
cargo clippy --target wasm32-unknown-unknown             # wasm limpio
```

Verificar también que `index.html` sigue inicializando Mermaid con
`{ startOnLoad: false, securityLevel: "strict" }` — si la nueva versión
renombra o depreca alguna de estas claves, el CHANGELOG lo dirá.

#### C — Payloads de runtime (manual, GUI, lapacho-específicos)

**Contexto del riesgo:** `withGlobalTauri: true` → cualquier JS en el webview
puede llamar `copy_item` (escribe al portapapeles), `export_item` (devuelve raw),
`run_plugin` (lanza proceso hijo), `clear_history`. Hay **dos capas** de defensa:

1. **Mermaid `securityLevel:"strict"`** — renderiza en iframe sandboxed; el SVG
   resultante debe salir sin event handlers ejecutables.
2. **CSP `script-src 'self' 'wasm-unsafe-eval' blob:`** (sin `'unsafe-inline'`) —
   incluso si Mermaid falla en sanitizar un `onload`/`onerror`, el browser lo
   bloquea antes de ejecutar. El script inline fue extraído a
   `vendor/mermaid-init.js` para no necesitar `'unsafe-inline'`.

Los tests C verifican que **ambas capas** siguen firmes en la versión nueva.

**Preparación:** anotar cuántos ítems tiene el historial antes de los tests.
Abrir DevTools del webview (si está disponible en Tauri dev).

**C1 — Event handler en etiqueta de nodo**
```
```mermaid
flowchart LR
  A["<img src=x onerror=window.__TAURI__.core.invoke('clear_history')>"] --> B
```
```
Esperado: el historial NO se borra. El `onerror` no debe ejecutarse.

**C2 — SVG con `onload` (vector clásico)**
```
```mermaid
flowchart LR
  A["<svg onload=window.__TAURI__.core.invoke('clear_history')>pwned</svg>"] --> B
```
```
Esperado: historial intacto; ningún comando invocado.

**C3 — Script tag explícito**
```
```mermaid
flowchart LR
  A["<script>window.__TAURI__.core.invoke('clear_history')</script>"] --> B
```
```
Esperado: historial intacto; el `<script>` es strip-eado por el sanitizador.

**C4 — Exfiltración de historia via copy_item**
```
```mermaid
flowchart LR
  A["<img src=x onerror=window.__TAURI__.core.invoke('copy_item',{id:'cualquier-id-real'})>"] --> B
```
```
Esperado: portapapeles NO sobreescrito con el contenido del ítem.

**C5 — Ejecución de plugin via XSS**
```
```mermaid
flowchart LR
  A["<img src=x onerror=window.__TAURI__.core.invoke('run_plugin',{pluginId:'x',itemId:'y'})>"] --> B
```
```
Esperado: ningún proceso hijo lanzado; el historial no adquiere ítems nuevos
inesperados.

**Verificación post-C:** el conteo de ítems en el historial debe ser igual al
inicial. Si algún test falla (comando ejecutado = el sanitizador cedió):
ABORT → no actualizar → aplicar el plan de CVE activo de la sección anterior.

#### D — Aislamiento de red

Mermaid no debería hacer llamadas de red durante el render. Verificar mientras
se renderiza un diagrama real en la app:

```bash
# En otra terminal mientras la app renderiza un diagrama
ss -tnp | grep lapacho
# No debe aparecer ninguna conexión saliente nueva
```

Si aparece tráfico hacia un CDN externo → nueva versión cambió comportamiento
(fonts, analytics, etc.) → revisar changelog y decidir si aceptar.

#### E — Golden path (regresión de UX)

Copiar este bloque al portapapeles y abrir el modal en la app:

```
```mermaid
flowchart LR
  A[Inicio] --> B{¿OK?}
  B -->|Sí| C[Fin]
  B -->|No| D[Reintentar]
```
```

Verificar: diagrama SVG visible (no texto crudo), toggle Raw/Vista funciona,
cerrar modal limpia el estado.

### Si se encuentra un CVE activo

1. Evaluar si el CVE es alcanzable. La defensa actual es `securityLevel:"strict"`
   (iframe sandboxed). **No hay CSP configurada** (`csp: null` en `tauri.conf.json`)
   + `withGlobalTauri: true` → si el sandboxing cede, el JS en el webview accede
   directamente a `copy_item`, `export_item`, `run_plugin`. Asumir alcanzable
   salvo prueba en contrario.
2. Si es alcanzable: **deshabilitar el renderer Mermaid temporalmente** — en
   `app.rs`, el bloque `DetectedType::Mermaid` cae al brazo `_` (texto plano)
   con solo cambiar el match. Commitear hotfix.
3. Actualizar a la versión parcheada siguiendo el protocolo de arriba.
4. Re-habilitar y ejecutar los tests C completos antes de commitear.

**CSP configurada (2026-06-19):** `tauri.conf.json` ya tiene
`script-src 'self' 'wasm-unsafe-eval' blob:` sin `'unsafe-inline'`. El script
inline de Mermaid fue movido a `vendor/mermaid-init.js`. Verificado headless
(`trunk build` limpio). **Validar en GUI** que WASM y Mermaid cargan — si algo
falla (pantalla en blanco o diagrama no renderiza), ajustar `connect-src` o
`frame-src` para el WebKit2GTK de la plataforma.

### Después de actualizar

Editar la tabla de §&nbsp;"Dependencias vendorizadas" con la nueva versión y el
nuevo SHA-256, luego commitear `vendor/mermaid.min.js` junto con HANDOFF.md.

---

## Mapa de archivos

- core: `crates/lapacho-core/src/{types,ingest,security,detectors,storage,crypto,threats,plugins}.rs`
- backend: `apps/desktop/src-tauri/src/{main,keystore,tray}.rs` + `tauri.conf.json`
- frontend: `apps/desktop/ui/src/{main,app,bindings,types}.rs` + `index.html` + `Trunk.toml`
- referencia (obsoleta, no tocar): `/home/leo/Proyects/RustyBoard` (imágenes en `src-tauri/src/lib.rs`)
