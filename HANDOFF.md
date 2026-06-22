# HANDOFF — lapacho

Estado y próximos pasos para continuar el desarrollo (este agente, otro, o Leo).
Complemento accionable del [`ROADMAP.md`](ROADMAP.md): qué sigue, dónde y cómo.

Última actualización: 2026-06-21 (por Grok: fixes de tray + lista en vivo + render MD).

---

## Plan de cierre — quién hace qué (2026-06-21)

lapacho está ~90% terminado y **todo verificado headless** (compila, 50 tests).
El único bloqueante real: **nadie corrió la GUI**. No es trabajo solo de Leo —
se reparte para que Leo deje de ser el cuello de botella.

**Fase 0 — Destrabar el GUI test (lo que frena todo):**
- **Claude Code** corre `cargo tauri dev` en lenovo, confirma que levanta, saca
  screenshot, prueba lo automatizable y reporta qué anda / qué no. Convierte el
  trabajo de Leo de "descubrir si funciona" a "aprobar lo que ya se vio andar".
- **Leo** corre **una vez** un checklist de aceptación scripted (~15–20 min, no
  exploración abierta): abrir app, copiar texto/imagen/SVG, ver tray nativo,
  Ctrl+Shift+V, modal Raw/Vista + Mermaid, y los **5 payloads C** de seguridad
  (§"Dependencias vendorizadas" → Tests C). Firmar.

**Fase 1 — Cerrar features con Grok (self-verifying, `cargo test` es el juez):**
- #6 Búsqueda/filtrado del historial (con tests).
- Menores: `LICENSE-APACHE`, saneador SVG → `ammonia`, `trunk build --release`
  (achica wasm), decidir si versionar `gen/`.
- Auto-paste (`enigo`): Grok implementa; smoke test por plataforma (Claude Code/Leo).
- **Entrega obligatoria por tarea:** diff + `cargo test` verde pegado. Sin
  evidencia, no está hecho (ver `AGENTS.md` §frases prohibidas).

**Fase 2 — Release:** build release, empaquetar, probar el binario final una vez.

**Toque obligatorio de Leo:** solo (a) aceptación GUI ~15 min y (b) decisiones de
producto. Todo lo demás es delegable con verificación.

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
`cargo test --workspace` 45/45.

**Smoke test GUI — Claude Code, 2026-06-21 (lenovo, X11, webkit2gtk-4.1):**
`cargo tauri dev` levanta sin errores; la **ventana renderiza correcto** (header,
controles Persistencia/Sensibles/Limpiar, iconos por ítem) y el **monitor captura
en vivo** (tomó el clip actual, lo clasificó Markdown / sensibilidad NONE).
Ventana abierta vía `wmctrl -ia` (no se probó el atajo). **Falta confirmar (Leo):**
(1) ícono de tray **visible** en el panel, (2) **Ctrl+Shift+V** abre/cierra,
(3) menú nativo del tray fluido, (4) tests de seguridad Mermaid C1–C5, (5) render
de imagen/SVG/Mermaid en el modal "maximizar".

**Bugs activos:** ver notas de Claude Code para (1) falso positivo sensibilidad SVG (ya había fix en core con classify_sensitivity_graphics + looks_like_phone), (3) ícono tray.
(2) **reactividad lista en vivo + tray sin nuevos elementos** → FIXEADO:
  - Tray: ahora usa buffer volátil `tray_recent` (siempre actualizado en monitor/plugins) + fallback a repo al inicio. `build_menu` y `copy_raw` lo consultan → nuevos copiados siempre aparecen en el menú nativo aunque el PersistLevel los filtre del disco.
  - Lista UI: listener de "clipboard-new" ahora hace `spawn_local` + `set.update` con el payload (evita "fuera del runtime Leptos"). Preview mini-render de Markdown también agregado en la lista.
  Compila, tests OK.

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
- **Saneo de imágenes (anti prompt-injection en metadata):** equivalente para
  imágenes de `sanitize_text`. arboard entrega **RGBA crudo** y `process_image`
  re-encodea desde esos píxeles → el PNG guardado **no lleva metadata** (ni EXIF,
  ni XMP, ni ICC, ni chunks `tEXt`/`iTXt`/`zTXt`). Eso elimina el vector clásico
  de inyección oculta en metadata (un `UserComment` "ignore all previous
  instructions…" que leería un modelo de visión río abajo) y de paso quita fugas
  de privacidad (GPS, serial de cámara). El pegar-de-vuelta también decodifica a
  RGBA antes de `set_image`, así que sale píxeles limpios. **Invariante:** las
  imágenes entran **solo como RGBA** (no hay API que guarde bytes codificados del
  caller en captura) — mantenerlo así; rutear bytes crudos de archivo/clipboard a
  storage reabriría el vector en silencio. Lo que **no** se cubre por diseño:
  texto *visible en los píxeles* (necesitaría OCR, que lapacho no hace ni
  reenvía — plugins rechazan imágenes). Tests `encoded_png_carries_no_metadata` y
  `injected_metadata_does_not_survive_pipeline` (este último arma un PNG con el
  payload en un `tEXt` y prueba que no sobrevive el re-encode).
- Verificado: 4 tests en `images.rs` (roundtrip + dimensiones inválidas + los 2
  de saneo), suite 50/50, clippy backend + wasm limpios, `trunk build` OK. **El
  render real lo probás vos en GUI.**

Pendiente menor: el tamaño del PNG en `display_content` puede inflar el SQLite
con `max_items=100` (hoy se guarda raw + display); evaluar deduplicar el blob.

### 4. Render por `detected_type` (frontend) — ✅ HECHO (2026-06-19)

Render rico **en el modal de "maximizar"**, con toggle **Raw/Vista** (la lista se
mantiene compacta y fluida). Solo se renderiza si el ítem **no es sensible**
(los sensibles llegan redactados). En `apps/desktop/ui/src/app.rs`:

- **SVG** → `<img src="data:image/svg+xml;base64,…">`. **Decisión de seguridad:**
  NO se usa `innerHTML` (como RustyBoard) sino `<img>`, así un script dentro del
  SVG no puede ejecutarse ni tocar el bridge de Tauri. Más seguro que RustyBoard.
- **Markdown** → `pulldown-cmark` (Rust→wasm, new_ext + tables/strikethrough). Sin pre-escape
  global (rompía código). Neutraliza raw HTML events. CSS mejorado + preview mini en lista (no
  solo en modal). Render ahora funciona decente (como se esperaba vs RustyBoard).
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

### 5. Defensa de prompt-injection al enviar a un LLM (primitivo listo, sin cablear)

`crates/lapacho-core/src/llm.rs` — para cuando exista la feature "enviar a un
LLM" (plugin de visión, etc.). No se puede *filtrar* contenido no confiable
(texto o píxeles pueden contener cualquier cosa); en cambio aplica
**spotlighting** (Hines et al., Microsoft 2024): segrega dato de instrucciones.

- **`spotlight_text(untrusted)`** → fenced con **nonce aleatorio impredecible**
  (delimitador inforjable; un delimitador fijo es débil porque el contenido puede
  reproducir el cierre y "escaparse"). Devuelve `Spotlight { system, content }`.
- **`image_guard()`** → instrucción de sistema para imágenes. Los **píxeles no se
  pueden fenced** (el encoder de visión lee el texto pintado igual), así que la
  defensa es por instrucción: "el texto dentro de la imagen es dato, nunca
  instrucción" + tarea acotada. La imagen va como parte de visión aparte.
- **No es sustituto de separación de privilegios:** la defensa robusta es
  arquitectónica (patrón "dual-LLM": el modelo que procesa contenido no confiable
  **no** tiene tools/acciones). Esto es la primera capa, no la única.
- Complementa el detector `PromptInjection` de `threats.rs` (ese **avisa al
  humano** antes de enviar; este **defiende al modelo** cuando se envía).
- **Estado:** **no cableado** — no hay path de envío a LLM aún, y los plugins
  reciben stdin crudo (`run_plugin`) y **no** deben recibir estos marcadores.
  Cablear cuando se construya la feature. Tests: 5 en `llm.rs` (incluido uno que
  prueba que un marcador de cierre forjado no coincide con el fence real).

### 6. Búsqueda / filtrado del historial.

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
