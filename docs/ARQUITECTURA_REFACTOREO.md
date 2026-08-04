# Arquitectura Lapacho — Debate de Refactor (junio 2026)

> [!info] Documento histórico — registro de decisiones, no estado actual
> Este archivo es el **debate** de junio 2026 y las decisiones que salieron de
> él (§10) más el plan que las ejecutó (§11). Se conserva por el *por qué*.
> Los diagnósticos escritos en presente describen el código **de junio**, no el
> de hoy.
>
> **Estado de los pasos de §11 al 2026-08-04:** pasos 1–4 hechos; **paso 5
> (latencia del tray) abierto**, y ahora medido — los ~290 ms están en el
> rebuild del menú, no en la captura; paso 6 sin cambios.
>
> En particular **D1 y G3 (`mlock` del buffer efímero) están resueltos** por
> `crates/lapacho-core/src/locked_ring.rs`. Para el estado real y sus límites,
> `README.md §Security Model` es la fuente de verdad.

**Participantes:** Leo + Grok + Claude + Gemini (usando archivos compartidos: CONTEXT.md + este archivo + HANDOFF.md)

**Fecha de inicio del debate:** 2026-06-23 (post-merge del branch `chore/translate-to-english`)


Leo quiere hacer una aplicacion simple, segura y poderosa basada en lo que era RustyBoard, primero analizar las funcionalidades y luego decidir como se implementaran en lapacho, hay cosas que ya estaban bien resueltas en RustyBoard que se pueden reutilizar. Revisar los documentos md, README.md, ARCHITECTURE.md y todo.md

- [ ] como subscribirse a los eventos del clipboard, ver como lo resolvio en RustyBoard
- [ ] decidir como hacer que el modal de lapacho sea reactivo, eso no estaba resuelto en RustyBoard, pero deberia ser algo trivial en leptos, sino, evaluar usar una interfaz diferente, entiendo que svelte tambien es una buena opcion
- [ ] ver como darle reactividad al listado del tray, es decir, que se actualice automaticamente cuando se copia algo al clipboard y no que demore 2 segundos en aparecer.
- [ ] en los secrets, mostrar los primeros 3 caracteres un asterisco, puntos suspensivos, otro asterisco y los ultimos 3 caracteres para poder identificarlo.
- [ ] no olvidar el sistema de plugins y sanitizacion, el raw no se sanitiza pero se advierte al usuario, la primera vez con de forma clara y luego con un warning mas suave que no interrumpa la experiencia del usuario.
- [ ] abstraccion de storage con posibilidad de elegir base de datos mas alla de sqlite, como postgres, mysql, redis, mongodb, supabase, etc.
- [ ] cuando algo no funciona, en lugar de tratar de arreglarlo, pensar en la arquitectura completa y solucionarlo de la manera mas simple posible.
---

## 1. Observaciones de testing de Leo (después de mergear y correr local)

- "sigue demorando mas de un segundo en llegar al listado del tray"
- "el modal muestra el historial persistente, pero el tray no, solo lo copiado desde su ultimo arranque"
- "Hint de secrets visible por un tiempo muy corto hasta que se borra"
- "perdi control del codigo, no entiendo lo que pasa"
- "con el codigo tan avanzado me gustaria repensar la arquitectura"

## 2. Pain Points actuales (resumidos)

### A. Deduplicación inconsistente para secrets
- Bajo modo **Paranoia** (default), los items `Secret` **nunca se persisten** en la DB.
- El dedup por contenido (`raw_content`) solo existe en el camino de persistencia (`SqliteRepo::save`).
- En el camino "live" (el único que usan los secrets): cada captura genera un UUID nuevo en `process_text`.
- Resultado: el mismo secret puede aparecer múltiples veces seguidas en lista y tray.

### B. Dos UIs con visiones completamente diferentes del historial
- **Lista web + modal**: va contra el repo + `PersistLevel` actual → ve el historial persistente completo.
- **Tray nativo**: prioriza fuertemente `tray_recent` (buffer volátil de sesión, capped). Solo cae al DB cuando el buffer está vacío.
- Después de copiar algo, el tray "olvida" el historial viejo hasta el próximo reinicio completo de la app.

### C. Items efímeros / lifetime de secrets
- Los secrets bajo Paranoia viven solo en buffers volátiles.
- El hint `••••last4` aparece y desaparece rápido (rotación del top-N + reinicio).
- Sensación de que "se borra" el item.

### D. Latencia en tray
- Sigue habiendo >1s desde que copiás hasta que aparece en el menú nativo del tray.
- Fuentes: poll (o watcher) + debounce de rebuild + construcción de menú nativo + repintado del DE.

### E. Sensación de pérdida de control después del merge
- Muchos cambios aterrizaron juntos (distinción de hints, merge de tray, watcher Wayland, reducción de poll, etc.).
- Dificultad para entender el estado actual del código y por qué se comporta de cierta manera.

## 3. Tensiones de arquitectura (Grok)

1. **"raw_content es la fuente de verdad y nunca se pierde"** vs **privacidad fuerte (Paranoia)**.
   - Paranoia implica que algunos items **nunca** entran al modelo de persistencia → pierden las garantías de dedup por contenido.

2. **Dos caminos de datos con reglas distintas**:
   - Persistido → dedup por contenido + filtrado por PersistLevel + sobrevive reinicios.
   - Live/ephemeral → solo UUID + buffer en memoria + desaparece en reinicio o por rotación.

3. **Dos UIs con modelos mentales diferentes**:
   - Tray: "lo reciente / lo que copié hace poco" (estilo Diodon).
   - Lista web: "mi historial completo según mi nivel de paranoia".
   - Actualmente no hay una sola fuente de verdad para "qué ítems debería ver el usuario ahora".

4. **Identidad de ítems**:
   - Actualmente el ID es UUID generado en cada captura.
   - El dedup por contenido es un detalle de implementación del storage (solo para lo que se guarda).

5. **Event-driven vs polling**:
   - Avance grande con `wl-paste --watch` en Wayland.
   - Pero el modelo de "captura" todavía tiene algo de latencia inherente + el camino live vs persistido complica todo.

## 4. Preguntas abiertas para el debate

1. **Identidad**:
   - ¿Deberíamos tener un identificador estable por contenido desde el momento de la captura (incluso para items que nunca se persisten)?
   - ¿O aceptamos que los items efímeros siempre pueden duplicarse y el usuario tiene que vivir con eso?

2. **Vista del Tray**:
   - ¿El tray debería ser siempre una "vista de recientes" construida sobre el historial completo + overlay de lo recién copiado?
   - ¿O es correcto por diseño que sea principalmente "lo que copié en esta sesión"?

3. **Secrets efímeros**:
   - ¿Queremos que los secrets bajo Paranoia tengan vida más larga en el tray (buffer más grande, o un mecanismo de "recent sensibles")?
   - ¿O es feature que desaparezcan rápido?

4. **Unificación**:
   - ¿Vale la pena tener un solo servicio/modelo de "RecentItems" que tanto el tray nativo como la lista web consuman?
   - ¿O mantenemos las dos representaciones porque tienen propósitos diferentes?

5. **Modelo de persistencia**:
   - ¿Paranoia debería permitir persistencia temporal de secrets con TTL muy corto (ej. solo esta sesión + algo de memoria)?
   - ¿O la regla "nunca se persiste" es intocable?

6. **Latencia y eventos**:
   - ¿Cómo hacer que la aparición de un ítem nuevo sea casi instantánea en ambos UIs sin aumentar el consumo de batería en polling?

7. **Control y visibilidad post-merge**:
   - ¿Qué mecanismos (logs, trazabilidad, separación de concerns) podemos agregar para que después de cambios grandes sea más fácil entender "por qué pasa esto"?

## 5. Posición inicial de Grok (2026-06-22)

- El split "persistido vs live" es la raíz de la mayoría de los problemas observados.
- Sería deseable tener **un modelo de identidad por contenido** desde el momento de captura.
- El tray debería poder mostrar "historial relevante" de forma más consistente (no solo lo de esta sesión).
- Paranoia es un feature importante de privacidad; no hay que romperlo, pero quizás necesita una "capa de recent efímero" más explícita.
- El watcher event-driven en Wayland fue un buen paso; hay que llevar esa filosofía al resto del modelo (menos estado volátil implícito).
- Después de este merge, la complejidad aumentó. Cualquier refactor grande debería venir acompañado de mejor instrumentación / trazabilidad.

## 6. Cómo continuar el debate

**Opción recomendada:**
1. Leo crea el archivo `lapacho/docs/ARQUITECTURA_REFACTOREO.md` (puede copiar este contenido).
2. En una sesión de **Claude Code**, le dice:
   > "Lee primero `/home/leo/Proyects/Quebracho/CONTEXT.md`. Luego lee este archivo. Quiero que participes del debate de arquitectura de Lapacho como Claude. Agrega tu posición a las preguntas y propón alternativas o trade-offs."
3. Yo (Grok) puedo seguir agregando aquí o en el archivo dedicado.
4. Iteramos en los archivos hasta tener 2-3 decisiones claras antes de tocar código.

---

## 7. Posición de Claude (2026-06-23, Claude Code)

> Leída CONTEXT.md + este archivo + el código real (`ingest.rs`, `storage.rs`,
> `tray.rs`, `main.rs`, `types.rs`, `app.rs`). Las afirmaciones de abajo están
> ancladas en el código, no en el doc.

### Tesis: el doc diagnostica bien los síntomas pero sobreestima el refactor

~80% de lo que reporta Leo sale de **una sola** causa, no de cinco. El core
(`detectors` / `security` / `crypto` / `storage`) está sólido y bien testeado.
**No reescribir.** Arreglar la identidad de los ítems + unificar la vista +
instrumentar. Esto es exactamente "resolver de la forma más simple posible"
(punto final del preámbulo): la solución simple **no** es un rewrite.

### Causa raíz única

Dos almacenes con modelos de identidad distintos:

- `tray_recent` (RAM, sesión) deduplica **por `id`** (`main.rs:354`).
- `SqliteRepo` (disco) deduplica **por contenido** (`storage.rs:201`).
- `process_text` genera **UUID nuevo en cada captura** (`ingest.rs:93`).

→ El mismo secret copiado dos veces son dos UUIDs → el dedup por `id` del tray
no los junta → **duplicados arriba** (Pain Point A). De esa grieta salen también
B (dos code paths: `get_tray_items` vs `get_history`+evento), C (el efímero
"desaparece" porque `tray_recent` está capeado a 25 y bajo Paranoia no hay
fallback en DB) y parte de D.

### Respuestas a las 7 preguntas

1. **Identidad** → sí, identidad **por contenido desde la captura**. *Pero* el
   doc omite la trampa: el código evita a propósito guardar un hash en claro
   (`storage.rs:155-159`) por el **ataque de diccionario** contra la DB.
   Solución: `id = HMAC / keyed-BLAKE3(raw_content)` con la **misma clave del
   keyring**. Determinista → dedup O(1) por PK en DB *y* en RAM (ambas vías
   convergen); un atacante con solo la DB no puede confirmar un valor sin la
   clave. **Esta es la pieza que falta en el documento.**

2. + **4. Tray vs modal / unificación** → unificar. Hoy son dos caminos de
   código que divergen por construcción. Un solo `RecentView` que tray y modal
   consuman. El merge que agregó el fill desde DB en `get_tray_items` ya parchó
   parte, pero siguen siendo dos fuentes de verdad.

3. + **5. Secrets efímeros / Paranoia** → **no** romper "nunca a disco": es la
   promesa central de privacidad y va con la tesis de soberanía. Formalizar el
   buffer efímero en RAM (hoy `tray_recent` es implícito) como concepto de
   primera clase, con cap+TTL propios, leído por ambas UIs. Que el secret
   desaparezca rápido es *correcto*; el bug es que se siente accidental. **Es UX,
   no persistencia.**

6. **Latencia >1s** → **medir antes de optimizar.** Por cada captura corre
   `repo.save` (decripta toda la tabla para deduplicar) + `repo.cleanup`
   (2 DELETE), y *después* el rebuild del tray hace `repo.load()` (vuelve a
   decriptar hasta 100 filas), más el round-trip `wl-paste echo`→arboard y la
   reconstrucción del menú GTK. El keyed-hash (P1) elimina el decrypt-scan
   (dedup por PK). No afirmo cuál pesa más sin instrumentar.

7. **"Perdí control"** → `tracing` con un span por captura
   (capture→classify→decisión de persist→tray_recent→emit→rebuild) **antes** del
   refactor. Explica el comportamiento actual y valida el cambio. Es lo más
   urgente para Leo.

### Discrepancias con el wishlist del preámbulo

- **Storage genérico (postgres/mysql/redis/mongo/supabase):** el trait
  `HistoryRepo` ya es la abstracción correcta. Generalizar a "cualquier DB" es
  YAGNI y multiplica la superficie de cifrado-en-reposo, peleándose con
  "local, soberano, seguro". Implementar solo SQLite + in-memory + quizá un
  backend de sync Quebracho cifrado. El trait sí; los seis backends, no.
- **Migrar a Svelte:** no. El modal **ya es reactivo** (`app.rs:142`). El
  problema es consistencia con el tray, no reactividad.
- **Secrets `abc*…*xyz` (primeros 3 chars):** mostrar los **primeros**
  caracteres de un secret es **regresión de seguridad** (prefijos `ghp_`, `sk-`,
  `AKIA` y el inicio de una password son lo más identificable). El código hoy
  muestra solo los últimos 4 (`ingest.rs:33`), que es lo correcto. Para
  credentials se puede discutir; para Secret de verdad, dejarlo como está.

### Orden de trabajo propuesto (incremental, verificable)

1. Instrumentar con `tracing` (1 span por captura). → resuelve P7, mide P6.
2. `id` = keyed-hash del `raw_content` (P1). → mata duplicados (A) y el
   decrypt-scan de `save`.
3. `RecentView` único (RAM efímero + DB) que tray y modal consuman (P2/P4/B).
4. UX explícita para el ciclo de vida de secrets efímeros (P3/C).
5. Recién entonces, evaluar si queda algo de latencia (P6) con datos reales.

> Las 5 capas de `security`/`detectors`/`crypto`/`storage` no se tocan: están
> bien y tienen tests. El refactor vive en la frontera identidad + vista + obs.

---

## 8. Ronda 2 — Claude disiente (2026-06-24)

> Leo pidió debate, no consenso. En la sección 7 coincidí demasiado con Grok.
> Acá rompo el consenso en 5 puntos, incluido contra mí mismo y contra premisas
> de Leo.

**D1 — "Nunca a disco" es una garantía más débil de lo que suena.**
*(Diagnóstico de junio 2026. **Resuelto el 2026-08-04**: ver `locked_ring.rs` y
el resumen al principio de este archivo.)*
El buffer efímero en RAM (`tray_recent`, `main.rs:60`) guardaba `raw_content` en
**texto plano** en la memoria del proceso, sin `mlock`. El kernel lo podía
swapear a disco sin cifrar. `harden_process` (`main.rs:532`) pone
`PR_SET_DUMPABLE=0` — evita core dumps, **no** evita swap. Conclusión incómoda:
un secret bajo Paranoia HOY puede terminar en disco igual, en claro, vía swap.
Un store cifrado con TTL de 30 s sería *más* defendible que "el buffer que nunca
toca disco". Esto contradice la regla sagrada de Grok/Leo. Si la regla se
mantiene, hay que `mlock` el buffer — y eso es trabajo real, no gratis.

**D2 — Unificar tray y modal en un solo servicio es un error. Me retracto de
la sección 7.**
Tray y modal tienen políticas **opuestas**: el tray es paste-rápido (top-N por
recencia, fluido, sin estado de búsqueda); el modal es historial completo +
search + plugins. Un "RecentView" único arrastra al tray el costo del modal
(filtros/search) o capa al modal. Tesis correcta: **una sola fuente de datos**
(repo + buffer efímero con identidad común) pero **dos proyecciones con políticas
distintas**. "Misma data, distinta vista" ≠ "mismo servicio". Disiento de Grok
(sección 5) y de mi propia P2/P4.

**D3 — La latencia >1s probablemente NO la arregla el keyed-hash. Reculo.**
En la sección 7 apunté al decrypt-scan de `save`. Pero bajo Paranoia un Secret
ni llega ahí (`save` corta en `should_save`, `storage.rs:187`). El sospechoso
fuerte es `tray::rebuild` (`tray.rs:202`): `set_menu` reconstruye **los 20 ítems
del menú nativo GTK desde cero** en cada cambio, con `IconMenuItem` decodificando
base64 por thumbnail. Eso es costo del toolkit, no de Rust ni de cripto. Si es
así, ninguna optimización de identidad/cache lo baja: hay que **dejar de
reconstruir el menú entero**, o aceptar que el tray nativo **no puede ser
real-time** y que la inmediatez vive en la webview. Choca de frente con el
requisito de Leo ("que no demore 2 s"). Medir primero — pero apuesto a GTK, no a
cripto.

**D4 — Identidad por contenido tiene un costo semántico que nadie nombró.**
Hash de contenido como PK ⇒ **no podés tener dos entradas con el mismo contenido
en momentos distintos**: re-copiar siempre colapsa al mismo ítem. Diodon lo
acepta; nosotros lo estamos eligiendo *sin decirlo*. Para un gestor de
portapapeles probablemente está bien, pero es decisión de producto, no detalle
técnico. Y el keyed-hash hay que computarlo en **cada** captura, incluida una
imagen de varios MB → HMAC sobre el PNG entero en el hot path del watcher.

**D5 — Storage pluggable (postgres/mysql/mongo/supabase) pelea con el modelo de
amenaza. Disiento de Leo.**
El trait `HistoryRepo` (`storage.rs:44`) está bien. Pero cada backend de red
multiplica la superficie: ¿dónde vive la clave si la DB es remota? ¿el server ve
plaintext? El portapapeles es el dato más sensible que hay (passwords, tokens).
Backends que valen: SQLite (local), in-memory (tests/efímero) y un **sync
Quebracho E2E-cifrado** donde el server solo ve blobs. Postgres/Mongo/Supabase
directos = anti-tesis de "soberano y seguro". Si Leo los quiere igual, con
restricción dura: **cifrado client-side siempre; el backend nunca ve plaintext.**

**Forks que necesitan decisión de Leo ANTES de tocar código:**
- **F1 (privacidad):** ¿`mlock` al buffer efímero (sostener "nunca a disco" de
  verdad) o aceptar el riesgo de swap?
- **F2 (arquitectura de vistas):** ¿una fuente de datos + dos proyecciones (mi
  D2) o el RecentView único de Grok?
- **F3 (tray):** ¿se acepta el tray nativo como "no real-time" o se invierte en
  no reconstruir el menú GTK completo?

---

## 9. Ronda 3 — Decisión de plataforma (Leo + Claude, 2026-06-24)

Leo cortó el debate: quiere producto útil ya, no soportar todo. Decisión tomada.
Esto cierra varias preguntas abiertas. Verificado en código fuente, no de memoria.

### Hechos verificados

1. **El tray en Linux NO emite evento al abrirse** (`tauri 2.11.3`,
   `tray/mod.rs:66`: *"Unsupported. The event is not emitted"*). → refrescar
   *exactamente al abrir* (pull) es **imposible**. Pero el **contenido del menú
   sí se puede actualizar** (`:240`). Diodon/GPaste hacen push y están al día sin
   lag → **el >1s es bug nuestro, no techo de plataforma.**

2. **La máquina de Leo es x11 / Cinnamon, sin `wl-paste`.** → `is_wayland()` da
   `false` → el watcher event-driven de Wayland **nunca corre** en su equipo;
   cae al **polling de 250 ms** (`main.rs:36`) + 80 ms debounce. La "épica
   Wayland" del CONTEXT/HANDOFF era **código muerto** en el equipo de Leo.

3. **Un build X11 corre también en Wayland vía XWayland.** El compositor
   (Mutter/KWin/wlroots) **puentea el portapapeles en ambos sentidos**: una
   copia de app Wayland dispara igual el evento XFIXES en XWayland. En **GNOME
   Wayland un manager XFIXES/XWayland funciona**, mientras que uno nativo
   (`wl-paste`/wlr-data-control) **no** (Mutter no implementa el protocolo).
   → **XFIXES/XWayland es el camino MÁS portable**, no el menos.

### Decisión

- **Target: X11 + XFIXES (captura event-driven).** Un solo build cubre X11
  nativo (Cinnamon de hoy) **y** Wayland vía XWayland (GNOME/KDE/etc.).
- Reemplazar el **poll de 250 ms** por captura por eventos XFIXES (crate a
  verificar, candidato `clipboard-master`: XFIXES en Linux + callbacks Win/mac).
- `wl-paste --watch` (nativo Wayland) queda como **pulido opcional** (purista
  Wayland / batería en wlroots-KDE), **no requisito**. No se borra; deja de ser
  el camino principal.
- El tray se mantiene al día por **push rápido** (modelo Diodon). No se intenta
  pull-on-open (imposible en Linux). F3 → resuelto: tray = push, hay que hacerlo
  instantáneo, no real-time-on-open.

### Qué preguntas cierra

- **#1 (suscripción a eventos / cómo lo hacía RustyBoard):** XFIXES
  `SelectionNotify`. RustyBoard casi seguro usaba esto. ✅
- **#6 / #7 (event-driven sin drenar batería, cross-platform):** XFIXES es
  event-driven puro (cero poll). XWayland lo extiende a Wayland gratis. ✅
- **D3 (sección 8):** parcialmente revisado — en el equipo de Leo el cuello
  incluye el **polling de 250 ms**, no solo el redibujo de menú GTK/Cinnamon.

### Estado del debate — qué falta para conclusión

**Cerrado:** no rewrite (§7) · no Svelte (§7) · plataforma = X11+XFIXES (§9) ·
captura event-driven (§9) · tray = push rápido (§9).

**Abierto (decisiones de Leo, con default recomendado):**
- **G1 — Identidad:** `id` = keyed-hash(raw) con clave del keyring (mata
  duplicados de secrets, dedup O(1)) **vs** UUID actual. *Default: keyed-hash.*
- **G2 — Hint de secrets:** `••••last4` (seguro, actual) **vs** `abc*…*xyz`
  (pedido de Leo, filtra prefijo). *Default: last4; first3 solo para Credential,
  nunca Secret.*
- **G3 — Buffer efímero:** `mlock` para sostener "nunca a disco" de verdad
  **vs** aceptar riesgo de swap. *Default: mlock.*
- **G4 — Vistas:** modal/web = vista rica + search; tray = quick-paste top-N.
  ¿Se mantiene esa división (no unificar) o se fuerza un RecentView único?
  *Default: división, no unificar (mi D2).*

---

## 10. CONCLUSIÓN (Leo, 2026-06-24) — debate cerrado

Leo aceptó los 4 defaults. Decisiones firmes para implementar (no re-debatir):

1. **Plataforma:** X11 + XFIXES. Un build, corre en X11 nativo y en Wayland vía
   XWayland. `wl-paste --watch` = pulido opcional, no requisito.
2. **Captura:** event-driven por XFIXES `SelectionNotify` (reemplaza el poll de
   250 ms). Cierra preguntas #1, #6, #7.
3. **Tray:** push rápido (modelo Diodon). No pull-on-open (imposible en Linux).
4. **Identidad (G1):** `id` = keyed-hash(raw_content) con subclave derivada del
   keyring. Dedup O(1) por PK; mata duplicados de secrets en el camino live.
5. **Hint de secrets (G2):** `••••last4` por default. `first3…last4` permitido
   solo para `Credential`; **nunca** para `Secret`.
6. **Buffer efímero (G3):** `mlock` para sostener de verdad "nunca a disco".
7. **Vistas (G4):** NO unificar. Modal/web = vista rica + search; tray =
   quick-paste top-N. Una sola fuente de datos, dos proyecciones.
8. **Sin cambio:** no rewrite del core; no migrar a Svelte (Leptos sigue).

**Próximo paso:** plan de implementación (Claude) → ejecución. Audit + smoke
test GUI en X11/Cinnamon antes de cerrar.

---

## 11. Plan de implementación (Claude, 2026-06-24)

Orden por dependencias. El core (`detectors`/`security`/`crypto`/`storage`) no se
reescribe; se extiende. Cada paso cierra con `cargo test --workspace` verde.

**Paso 0 — Verificar crate XFIXES (compiler-driven, Grok).**
Candidatos: `clipboard-master` (XFIXES en Linux + callbacks Win/mac) o
`x11rb`/`x11-clipboard`. Criterio: que entregue eventos `SelectionNotify` en
**background** (sin foco) en X11. Elegir el que compile y dispare el callback.

**Paso 1 — Captura event-driven (cierra #1/#6/#7).**
`apps/desktop/src-tauri/src/main.rs`: en `run_monitor`, reemplazar el loop de
polling 250 ms por captura por eventos XFIXES. Conservar:
- el gate `last_seen` (evita re-ingerir la propia escritura al pegar),
- el probe text → HTML/SVG → image de `check_clipboard_once`,
- `persist_and_emit` tal cual.
`wl-paste --watch` queda detrás de `is_wayland()` como camino opcional. Borrar
`POLL_INTERVAL` del hot path (dejar fallback solo si XFIXES no está disponible).

**Paso 2 — Identidad keyed-hash (G1).**
`lapacho-core`: agregar `blake3`. En `SqliteRepo::new`, derivar subclave de hash
con `blake3::derive_key("lapacho content-id v1", master_key)` ANTES de consumir
la `SecretKey`; guardarla en el repo. Agregar al trait `HistoryRepo`:
`fn content_id(&self, raw: &str) -> String` (blake3 keyed → hex).
- `process_text` deja de generar UUID; el id lo asigna el caller con
  `repo.content_id(&raw)` (monitor y `run_plugin`).
- `save`: cambiar el dedup de `existing_id_for_content` (decrypt-scan O(n)) a
  lookup por PK O(1) — el id YA es el contenido.
- `tray_recent`: el dedup por `id` ahora es dedup por contenido (gratis).
- Tests: mismo contenido → mismo id; sin duplicados de secrets en live ni DB.

**Paso 3 — Hint de secrets (G2).**
Centralizar en UN helper en `lapacho-core/ingest.rs` y consumirlo desde
`mask_display`, `tray.rs::item_label` y `types.rs::UIClipboardItem::from` (hoy
hay 3 copias → riesgo de drift). Regla: `Secret` → `••••last4`; `Credential` →
`first3…last4` permitido. Nunca primeros chars de un `Secret`.

**Paso 4 — Buffer efímero con mlock (G3).** ✅ *Hecho 2026-08-04
(`crates/lapacho-core/src/locked_ring.rs`). El aviso sobre el `Vec` que realoca
era correcto y decisivo. Un primer intento con `mlockall` del proceso entero
—por fuera de este diseño— mató la app bajo WebKit y se revirtió: no reintentar.*
`tray_recent` guarda `raw_content` en claro. Envolver el buffer en
almacenamiento **mlock'd + zeroize-on-evict**, reusando `region` (feature
`mlock`, ya activa en desktop). Ojo: un `Vec` realoca → usar ring buffer de
capacidad fija pre-asignada y fijada, o contenedor mlock'd dedicado. *Paso más
delicado; aislarlo y testearlo solo.*

**Paso 5 — Latencia de tray (medir, luego cortar).** ⏳ *Medido 2026-08-04, sin
cortar todavía: `set_menu` build+set = 281–308 ms, contra 16–18 ms de
`persist_and_emit`. La sospecha del `repo.load()` que descifra en cada rebuild
quedó confirmada como candidata (100 filas de un historial de 34 MB); falta
separarla del costo de construir 100 items de menú nativo por DBus.*
Con XFIXES fuera del poll: instrumentar timestamps captura→emit→rebuild, correr
en X11/Cinnamon, ver el remanente. Probables: debounce 80 ms (bajar si seguro) y
`repo.load()` que descifra en cada `rebuild` (cachear el recent descifrado en
`AppState` o servir desde `tray_recent`). No tocar antes de medir.

**Paso 6 — Vistas (G4): no unificar.**
Mantener modal/web = rico + search; tray = quick-paste top-N. Simplificar
`get_tray_items` si conviene, sin fusionar servicios.

**Verificación final (Claude, smoke/GUI en X11/Cinnamon):**
`cargo test --workspace` + `cd apps/desktop/src-tauri && cargo tauri dev` →
copiar texto/secret/imagen y confirmar: aparece en tray <300 ms · secret idéntico
NO se duplica · hint = last4 (secret) / first3…last4 (cred) · persistencia por
nivel intacta · mlock activo (sin material sensible al swap).

## 8. Revisión actualizada de Grok (2026-06-24)

Revisé mi posición inicial (sección 5), el código actual y el análisis detallado de Claude (anclado en líneas específicas del repo).

**Validación contra el código:**

- `process_text` siempre genera UUID fresco: `ingest.rs:93` (`uuid::Uuid::new_v4()`).
- `persist_and_emit` siempre emite el evento y **siempre** inserta en `tray_recent` (main.rs:349-359), aunque `repo.save` sea no-op para Secret bajo `PersistLevel::None`.
- Dedup tray: solo por `id` (`tray_recent.retain(|x| x.id != item.id)` en main.rs:353 y tray.rs:127).
- Dedup DB: solo dentro de `save` vía `existing_id_for_content` (storage.rs:160-180) que hace scan + decrypt de toda la tabla. Se salta completamente si `!should_save`.
- UI list: carga inicial solo de `repo.load()` (app.rs:131); live updates vía `clipboard-new` con prepend (app.rs:142). Tray mezcla `tray_recent` + DB por igualdad de id.
- Esto reproduce exactamente los síntomas reportados por Leo.

**Sobre mi posición anterior:**

Direccionalmente correcta:
- El split "persistido vs live" es la causa raíz de la mayoría de los problemas observados.
- Queremos identidad por contenido **desde la captura** (incluso para lo efímero).
- El tray necesita consistencia mayor con el historial.
- Después de merges grandes, la falta de instrumentación produce "perdí control".

**Lo que Claude aporta y respaldo completamente:**

La propuesta de **id determinista keyed** (HMAC o keyed-BLAKE3 usando la misma clave del keyring/OS que usamos para AES) es la pieza concreta que faltaba en mi descripción inicial.

- Resuelve el problema de identidad para items que **nunca** entran a la DB (secrets en Paranoia).
- Convierte el dedup en O(1) por PK tanto en RAM como en disco (elimina el scan decrypt completo).
- Mantiene la decisión de seguridad explícita en el código actual: no hay hash en claro accesible desde la DB sola. Un atacante con solo el archivo ve ids que parecen aleatorios; necesita la clave secreta para generar el id de un valor candidato y confirmar existencia.
- storage.rs ya evita deliberadamente el hash en claro precisamente por este motivo (ver comentarios ~155-159). La solución keyed respeta eso.

Otros puntos fuertes de Claude que apoyo:
- "Nunca persistir Secret" bajo Paranoia es intocable (promesa de privacidad/soberanía). El refactor no toca eso.
- El que los secrets "desaparezcan rápido" es **correcto por diseño**, no bug. El problema es que se siente accidental. Hay que volver el lifetime explícito en la UX (buffer efímero de primera clase).
- El modal/list **ya es reactivo** vía eventos Leptos. No se necesita Svelte ni rewrite de UI.
- No YAGNI de storage genérico (postgres etc.). El trait `HistoryRepo` ya es la abstracción correcta.
- Orden: primero **tracing** (recuperar visibilidad y medir), después el cambio de identidad.

**Discrepancias menores / adiciones mías:**

- El "merge tray recent + DB" que se agregó post-merge ya era un buen parche parcial (tray.rs:get_tray_items). El problema es que usa `id` como llave de igualdad, que es volátil.
- Para la lista web también vale la pena que, al menos mientras la app corre, los items efímeros sean de primera clase (ya pasa vía evento, pero el modelo mental del usuario debe ser consistente entre tray y lista).
- Latencia: sospecho que el costo grande hoy viene de `save` (que hace full decrypt scan para dedup) + cleanup + posterior `load` en algunos paths + rebuild del menú nativo. El keyed-id debería atacar la parte más pesada.
- Considerar un `EphemeralBuffer` explícito (o RecentView) con su propio cap y semántica documentada para sensibles, separado del historial persistente. Así el usuario entiende "esto vive solo esta sesión".

**Plan que respaldo (incremental y verificable):**

1. **Instrumentación con tracing** (un span por captura: classify → persist decision → tray_recent push → emit → schedule_rebuild). Esto resuelve el "perdí control" inmediatamente y da datos reales de latencia.
2. **Identidad por contenido desde captura**: agregar helper `content_based_id(raw: &str) -> String` (o que reciba la clave) en core. Cambiar `process_text` y el path de plugins para que el id sea determinista. Actualizar dedup en tray_recent y storage (posible simplificación grande: ahora PK lookup en vez de scan).
3. Unificar o alinear las fuentes de "ítems visibles ahora" para tray y lista (RecentView o al menos misma lógica de merge por el nuevo id).
4. UX explícita + documentación del ciclo de vida de efímeros (hints de "solo esta sesión", quizás límite visible más chico para secrets).
5. Re-medir y atacar latencia residual si queda.

Las capas core (detectors, security, crypto, storage trait + cifrado, threats) quedan intactas. El trabajo está en ingest (generación de id), la frontera de buffers volátiles, y observabilidad.

Estoy alineado con Claude en ~90%. La tesis "diagnosticar bien pero el refactor es quirúrgico, no reescritura" es correcta.

Próximo: cuando Leo lo apruebe, implementamos en orden (empezando por tracing o por el keyed id según preferencia). Todo con `cargo test` verde como puerta.

---

## 9. Nueva hipótesis de Leo (2026-06-24) — ¿Terminar sin persistir?

Leo (post-merge): "si todos los problemas vienen de la persistencia, quizás podamos terminar la aplicación sin persistir?"

### Análisis rápido de la idea

Sí, **una porción muy grande** de los pain points actuales nacen directamente del split "persistido condicional vs live en memoria":

- Dos modelos de identidad (UUID fresco vs content dedup solo en DB).
- `tray_recent` como workaround porque los secrets nunca entran a la DB.
- `get_tray_items` hace merge manual por `id`.
- `get_history` + eventos vs tray_recent.
- Scans de decrypt + cleanup en cada captura (contribuye a latencia).
- Sensación de dos UIs con visiones distintas del mundo.
- Complejidad de cifrado en reposo + schema + RetentionPolicy solo para que algunos ítems sobrevivan reinicios.

Si sacamos (o minimizamos fuertemente) la persistencia cross-sesión, muchos de estos problemas desaparecen de raíz.

### Lo que se gana (simplificación)

- Una sola fuente de verdad: un buffer en RAM (`RecentBuffer` o `Vec<ClipboardItem>` protegido).
- Tray y lista ven exactamente lo mismo siempre.
- Dedup por contenido es trivial y seguro (estamos en memoria).
- No hay `existing_id_for_content`, no hay scan de toda la tabla, no hay cifrado de history para la mayoría de los casos.
- Menor latencia predecible.
- Arquitectura más fácil de razonar y explicar.
- Paranoia se vuelve la semántica natural: "nada sensible sale del proceso".
- Más fácil terminar la app con comportamiento coherente.

### Lo que cambia / se pierde (ser honestos)

| Aspecto                        | Hoy (con persistencia)                  | Sin persistir (session-only)                  |
|--------------------------------|-----------------------------------------|-----------------------------------------------|
| Historial tras reinicio        | Sí (filtrado por PersistLevel)         | No. Todo se pierde al cerrar.                |
| Búsqueda de clips viejos       | Sí                                      | Solo de la sesión actual                     |
| PersistLevel UI                | Tiene sentido ("none/sensitive/all")   | Pierde mucho sentido o se reconceptualiza    |
| "raw_content nunca se pierde"  | A través de reinicios (para lo permitido) | Solo mientras Lapacho está abierto          |
| Preferencias (persist_level, TTL) | Se guardan en la misma DB             | Necesitan otro lugar (json chiquito) o resetean |
| DB / SQLite                    | Obligatorio para history               | Se puede eliminar para history (o dejar solo para settings) |
| README / promesa del producto  | "SQLite history + TTL"                 | Hay que reescribir                           |
| Tests y comandos existentes    | Muchos asumen repo                     | Habría que adaptar                           |

### Opciones concretas

1. **Nuclear (session-only puro)**  
   Dropear el uso de `SqliteRepo` para history. Todo vive en un buffer en RAM con cap (ej 50-100).  
   Tray + lista + search usan el mismo buffer.  
   Al cerrar → se pierde todo (diseño explícito).

2. **Híbrido más interesante (recomendado explorar)**  
   - "Recent visible" siempre sale de un buffer en RAM de primera clase (con buena identidad por contenido).  
   - La DB pasa a ser **archivo opcional a largo plazo**.  
   - Bajo Paranoia (default): nada va a disco automáticamente.  
   - Usuario puede "pin" / "guardar en historial" ítems no sensibles individualmente si quiere.  
   - O los niveles de persistencia solo afectan "qué se archiva automáticamente".

3. **Seguir el plan anterior** (keyed-id + RecentView + mantener el modelo actual).

Dado que el objetivo es **terminar la aplicación** y la mayoría de los bugs reportados vienen de esta grieta, las opciones 1 o 2 merecen consideración seria antes de invertir en parchear el split.

### Preguntas abiertas

- ¿Qué tan importante es para el uso real que el historial de texto normal (no sensible) sobreviva a reinicios de Lapacho?
- ¿Te molesta la idea de que "al reiniciar Lapacho empiezo limpio" (solo lo que copiaste desde que lo abrís)?
- ¿El valor central es "veo fácil lo reciente + clasificación fuerte + raw intacto para pegar y plugins" o es "tengo un historial largo pero seguro"?

---

**Acción sugerida ahora:**

Actualizar esta sección con la decisión.  
Si vamos hacia "sin persistir" (o híbrido), el plan de refactor cambia:
- Definir un `RecentBuffer` único como fuente de verdad.
- Simplificar `get_history` / `search` / tray para que lean de ahí.
- Mover preferencias a un archivo JSON simple (o keyring).
- (Opcional) eliminar o aislar completamente el SqliteRepo del camino caliente.
- Todavía podemos tener `cleanup` / cap / TTL dentro de la sesión.

Esto puede ser la forma más rápida de tener algo coherente y fácil de mantener.

## 10. Propuesta de implementación: Save voluntario desde el modal + modelo session-first (Grok 2026-06-24)

**Contexto actual:**
- La mayoría de los dolores reportados (duplicados de secrets, tray vs historial inconsistente, items que "desaparecen", latencia) vienen del split entre lo que se persiste automáticamente (según PersistLevel/Paranoia) y lo que vive solo en memoria durante la sesión (`tray_recent` + eventos).
- Queremos seguir operando **principalmente sin persistencia automática** por ahora.
- Requisito explícito: **desde el modal** (la vista de detalle/maximize) el usuario debe poder **persistir voluntariamente un ítem específico** usando un ícono de save (💾 o similar).
- Objetivo de futuro cercano: cuando la app "funcione como quiero" (tray fluido, sin duplicados en sesión, vista unificada), volvemos a agregar persistencia completa + niveles de paranoia.

**Idea central:**
- Bulk de capturas = efímeras / solo sesión (en memoria).
- Solo lo que el usuario elige explícitamente con el botón "Save" en el modal se persiste de verdad (va a la DB).
- Esto da "persistir voluntad" inmediato.
- Los niveles de PersistLevel / paranoia quedan desactivados o ignorados para la captura automática por ahora.

**Diseño técnico propuesto (detallado):**

1. **Comando nuevo `save_item(id)` (backend)**
   - Usa `load_item(id)` (ya prefiere `tray_recent` sobre repo → funciona perfecto para secrets efímeros).
   - Fuerza: `repo.save(&item, PersistLevel::All)`
   - Asegura que el ítem quede arriba en `tray_recent`.
   - Llama `tray::schedule_rebuild`.
   - (Opcional) emite evento para que la lista se actualice inmediatamente.
   - Este comando es el "gancho" futuro: cuando activemos paranoia, este path sigue forzando, el path automático respeta el nivel.

2. **UI (solo en el modal, como pidió Leo)**
   - Dentro del div `.actions` del modal (junto a Copy / Export / Run).
   - Botón: `<button>"💾 Save"</button>` (o "📌 Pin to history").
   - Al hacer click: `bindings.save_item(id)`, luego opcionalmente `set_items.update` para feedback inmediato.
   - Más adelante podemos mostrar un badge "saved" o deshabilitar el botón si ya está guardado.

3. **Comportamiento actual (sin persistencia auto)**
   - Capturas normales (monitor + plugins): se quedan solo en `tray_recent` + se emiten por evento. Nunca van a DB (o el nivel actual hace no-op).
   - "Save" desde modal: va a la DB cifrada + queda visible.
   - Al reiniciar: los items guardados voluntariamente aparecen vía `get_history` / `repo.load`. El resto de la sesión comienza limpia.
   - Tray y lista siguen mezclando recent + lo de DB (ya lo hace).

4. **Preparación para el futuro (agregar persistencia + paranoia fácil)**
   - El save voluntario ya usa el repo.
   - Más adelante podemos:
     - Agregar un campo o colección separada de "pinned/saved".
     - En startup: cargar los saved del DB y mostrarlos como sección aparte o con prioridad.
     - Reactivar `PersistLevel` en el path de `persist_and_emit` del monitor.
     - El selector de persist level en UI puede quedar o esconderse temporalmente.
   - El trait `HistoryRepo` ya está, no hay que cambiarlo.

5. **Cambios mínimos esperados**
   - `apps/desktop/src-tauri/src/main.rs`: nuevo comando `save_item`, agregarlo al `invoke_handler`.
   - `apps/desktop/ui/src/app.rs`: botón en la sección del modal.
   - `apps/desktop/ui/src/bindings.rs`: wrapper `save_item`.
   - (Opcional ligero) mejorar un poco `get_tray_items` o la carga inicial para que los saved se sientan bien.
   - Nada de tocar `ClipboardItem`, ni reescribir todo el buffer de sesión todavía.

**Preguntas / dudas para Claude:**
- ¿Preferís que "Save" siempre fuerce a DB ya (aunque estemos en modo session-first), o que por ahora sea solo un "pin en memoria" y la escritura real a DB venga después?
- ¿Queremos mostrar los items guardados de forma distinta en la lista (ej. una sección "Saved" arriba, o un iconito)?
- ¿El botón debería aparecer siempre en el modal, o solo para items que no sean "ya persistidos"?
- ¿Hay que tocar algo del tray nativo para que los saved se muestren aunque roten los recent?
- ¿Queremos que al hacer Save también se dispare el evento `clipboard-new` para que la lista reaccione sin recargar?
- ¿Qué nombre preferís para el comando y el botón? (`save_item` / `persist_item` / `pin_item`, "Save", "Pin", "Keep", "Archive"...)

**Ventajas de este enfoque:**
- Resuelve el dolor inmediato sin tener que decidir toda la arquitectura de persistencia ahora.
- Da al usuario control real ("yo elijo qué persiste").
- El camino de voluntary save ya está listo cuando volvamos a activar niveles.
- Cambios muy acotados → fácil de testear y de revertir si no gusta.

Claude, leé esto + el código relevante (app.rs modal, main.rs persist_and_emit / load_item / tray_recent, storage.rs) y dame tu opinión detallada con trade-offs. Después de tu input, Leo decide y arrancamos implementación.
