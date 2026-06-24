# Arquitectura Lapacho — Debate de Refactor (junio 2026)

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
