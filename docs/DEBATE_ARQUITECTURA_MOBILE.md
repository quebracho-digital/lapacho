# Debate de Arquitectura Mobile - Lapacho

**Fecha:** 2026-07-16  
**Objetivo:** Entender la propuesta mobile y decidir si es la correcta

## Para Leo (antes de leer)

Este debate reemplaza los documentos técnicos anteriores. Es más simple.  
**Lee esto primero**, luego pide a Claude que dé su opinión.

## Contexto breve

Lapacho es un administrador de portapapeles seguro. En el escritorio ya funciona bien.  
Ahora queremos llevarlo a Android.

**La pregunta clave:** ¿Cómo hacemos que funcione en Android sin perder seguridad?

## Los 3 problemas que resolvemos

### 1. El portapapeles "desaparece" en Android
En el escritorio, si copias un secreto y cierras la app, sigue ahí.  
En Android, el teclado virtual (IME) se mata cuando lo cierras. Si el secreto solo vive en memoria RAM, **desaparece para siempre**.

**¿Solución?** Guardarlo cifrado en el disco con una vida útil corta (ej. 15 minutos).  
**Dilema:** ¿Es menos seguro que "nunca en disco"? ¿O es más usable?

### 2. Dos interfaces, no una
- **Companion app:** La app completa, con historial, configuraciones, sincronización
- **IME (teclado virtual):** Solo para escribir y pegar rápido

**No son la misma app.** El IME debe ser liviano y matarse rápido. La companion app puede ser más pesada.

### 3. Sincronización entre dispositivos
¿Quieres que lo que copias en el celular aparezca en tu escritorio?  
**Sí, pero con reglas:**
- Opcional (no automático)
- Por item (elegís qué sincronizar)
- Cifrado extremo a extremo (ni siquiera el servidor puede leer)
- Los secretos bajo "Paranoia" no sincronizan

## Preguntas para Claude

Hola Claude. Leo y yo queremos tu opinión sobre la arquitectura mobile propuesta.

**Nuestras dudas principales:**

1. **Sobre persistencia de secretos:**  
   La propuesta dice que en mobile, los secretos bajo Paranoia **deberían** guardarse cifrados en disco con TTL corto (ej. 15 min), en lugar de "nunca en disco".  
   ¿Es esto más seguro o menos seguro que "nunca en disco"? ¿Qué riesgo real hay de que el contenido quede en claro en el disco?

2. **Sobre el modelo de dos procesos (IME + companion):**  
   Es correcto que el IME no tenga stack de red, y la companion app maneje la sincronización.  
   Pero, ¿cómo garantizamos que el IME pueda acceder al historial cifrado cuando el usuario abre el teclado? ¿No hay que abrir la base de datos constantemente?

3. **Sobre sincronización E2E por item:**  
   La arquitectura propone un sistema de "sync chain" tipo Brave Sync, con emparejamiento por QR.  
   ¿Es demasiado complejo para v1? ¿Podríamos empezar con algo más simple (ej. sincronización local LAN-only sin servidor)?

4. **Sobre el keyed-hash de contenido:**  
   En el escritorio, vamos a generar un ID basado en el hash del contenido (con clave secreta).  
   En mobile, ¿este ID debe calcularse en el IME o en la companion app? ¿No es un costo grande calcular hash sobre una imagen de 5MB en el hot path del IME?

5. **Sobre mlock para memoria:**  
   En el escritorio, proponen `mlock` para asegurar que secretos no se vayan a swap.  
   En Android, ¿es posible `mlock` en el IME? ¿Hay alternativas (ej. usar `Android Keystore` para guardar contenido sensible en RAM protegida)?

**Lo que ya acordamos (no debatir):**
- Android-first, iOS después
- Storage como fuente de verdad (no solo RAM)
- Sync opcional, E2E, por item
- No unificar tray y modal (son propósitos distintos)

**Por favor:** Sé crítico. No seas de acuerdo. Señala riesgos, suposiciones ocultas, y alternativas.

## Mis opiniones y sugerencias

### 1. Sobre persistencia de secretos

**Mi postura:** "Nunca en disco" es una promesa hermosa pero en Android es imposible de sostener sin `mlock` real.  
El kernel puede swappear RAM a disco sin cifrar. `PR_SET_DUMPABLE=0` no evita swap.

**Sugerencia:**  
- **Mobile default:** Cifrado en disco con TTL 15-30 min. Es más usable y probablemente igual de seguro si el dispositivo está protegido con contraseña/biometría.
- **Opción estricta:** "Nunca en disco" con `mlock` OBLIGATORIO. Pero documentar que esto puede causar lag o fallar en algunos dispositivos.

### 2. Sobre el modelo de dos procesos

**Mi observación:**  
El IME y companion app comparten el mismo UID. Pueden acceder al mismo archivo SQLite.  
Pero hay riesgo de **deadlock** si ambos escriben al mismo tiempo.

**Sugerencia:**  
- Empezar con **WAL mode** + conexiones cortas.
- Si hay contención, crear un **ContentProvider** que sea el único escritor.
- Documentar: "No dos writers a la vez".

### 3. Sobre sincronización E2E

**Mi observación:**  
Brave Sync-style pairing es correcto, pero **la complejidad es alta** para v1.

**Sugerencia:**  
- **v1:** Local-only. Sin sync. Validar que el IME + companion funcionen bien.
- **v2:** Sync LAN-only (WireGuard) sin servidor externo.
- **v3:** Sync con relay propio en SER5.

No necesitamos un servidor externo para empezar. Dos dispositivos en la misma LAN pueden comunicarse directamente.

### 4. Sobre el keyed-hash

**Mi preocupación:**  
Calcular hash sobre una imagen de 5MB en el hot path del IME → latencia.

**Sugerencia:**  
- **v1:** Solo hash para texto. Imágenes sin hash (ID UUID aleatorio).
- **v2:** Optimizar hash con chunking (hash incremental).
- **v3:** Hardware acceleration (SHA-256 en hardware).

No todo necesita deduplicación por contenido desde el día 1.

### 5. Sobre mlock

**Mi observación:**  
En Android, `mlock` puede no funcionar como en Linux. El kernel puede mover páginas a memoria no protegida.

**Sugerencia:**  
- **Probar `mlock` en el IME.** Si falla, usar **Android Keystore** para guardar contenido sensible en RAM protegida.
- **Alternativa:** Usar `SecureMemory` de Rust (`zeroize` + `mlock` si disponible).

## Opinión de Claude (2026-07-16, sin filtro)

Leí este archivo y `ARQUITECTURA_MOBILE_ANDROID.md` completo. Veredicto general: **la arquitectura técnica está bien pensada (storage-as-truth, dos procesos, sync en fases). Los errores están en este debate, no en el documento técnico — y el riesgo más grande no es técnico, es de adopción.**

### 1. Persistencia de secretos — de acuerdo con la conclusión, en desacuerdo con el argumento

La conclusión (cifrado en disco + TTL como default mobile) es correcta. Pero el argumento del swap está mal trasladado:

- **Android no swappea a disco en la práctica.** Usa zram: swap comprimido *dentro de la RAM*. El escenario "el kernel escribe tu secreto en claro al disco" es de Linux desktop, no de Android. No lo uses para justificar la decisión — te van a refutar con razón.
- **Los riesgos reales del disco en Android son otros:**
  1. **El TTL no borra de verdad.** `DELETE` en SQLite no limpia páginas; con WAL el ciphertext viejo vive en el `-wal` hasta el checkpoint. Mitigación: es ciphertext, así que el borrado real es *destruir la clave*, no limpiar páginas. Conclusión práctica: una clave Keystore por clase de sensibilidad, y "wipe de secretos" = rotar esa clave. Barato y verificable.
  2. **Backups.** `allowBackup=false` + `no_backup dir` no es opcional, es día 1 (el doc técnico ya lo dice, bien).
  3. **Dispositivo desbloqueado = clave usable.** El cifrado con Keystore protege contra extracción del storage, no contra malware corriendo con el teléfono desbloqueado. Si querés subir la vara: `setUserAuthenticationRequired` con timeout para la clave de secretos (fuerza biometría para descifrar clase Secret). Eso sí es una mejora real, StrongBox si el hardware lo tiene.
- La "opción estricta nunca-en-disco" en mobile es teatro: el usuario la activa, Android mata el IME, el secreto desaparece, el usuario reporta un bug. Ofrécela pero enterrada y con el warning exacto que propone §4.2-B.

**Riesgo oculto que nadie mencionó:** el puente uniffi/JNI. En cuanto el plaintext cruza a Kotlin como `String`, vive en el heap de la JVM, inmutable, sin zeroize posible, hasta que el GC quiera. Todo el trabajo de `zeroize` en Rust es decorativo si el commit al campo de texto pasa por un `String` de Java (y va a pasar: `InputConnection.commitText` recibe `CharSequence`). Acepten esto explícitamente en el modelo de amenazas en vez de fingir que hay higiene de memoria end-to-end en Android.

### 2. Dos procesos — el "deadlock" no existe, pero el hot path sí

- WAL entre dos procesos del mismo UID no deadlockea: el segundo writer recibe `SQLITE_BUSY`. La solución es `busy_timeout` (250–1000ms) + transacciones cortas, no arquitectura nueva. Con el volumen de un clipboard (escrituras por minuto, no por segundo), la contención va a ser ~cero.
- **No hagan el ContentProvider preventivamente.** Mete IPC (binder) en el hot path del IME — exactamente lo que la regla de §2 ("cold start en decenas de ms") prohíbe. El plan del doc (WAL primero, medir, ContentProvider solo si hay evidencia) es correcto; este debate lo presenta como más simétrico de lo que es.
- La pregunta "¿no hay que abrir la base constantemente?" ya está respondida en §4.4-4.5 del doc técnico: conexión abierta durante la vida del proceso, top-N en RAM, reload solo en cold start / strip abierto. No es un problema abierto.

### 3. Sync — de acuerdo en diferir, en desacuerdo con la escalera v1/v2/v3 del debate

- v1 local-only: **sí, obvio.** El doc técnico ya lo dice (P0–P3 sin red).
- **El paso "v2 = LAN-only con WireGuard sin servidor" es un desvío, no un atajo.** WG te da la tubería pero no resuelve nada del problema real: pairing, envelopes, policy gate, outbox. Todo eso lo necesitás igual. Y "ambos dispositivos despiertos en la misma LAN a la vez" es la condición que *menos* se cumple con un teléfono. El relay store-and-forward en SER5 (§5.6) es *más simple de usar* que LAN-directo, y SER5 ya existe con Caddy+Authentik. La escalera correcta es la del doc: `lapacho-sync` puro con transporte en memoria (testeable sin Android) → relay HTTP en SER5 → direct/WG como optimización. No construyan un transporte descartable.
- **Sí háganlo ya:** las columnas placeholder (`sync_eligible`, `sync_state`, `sync_id`) en el schema de P0. Migrar schema dos veces con datos cifrados es dolor gratis.
- Riesgo aceptado a documentar: chain key simétrica compartida (S7) significa que revocar un dispositivo = resetear la cadena y re-parear todo. Para uso personal está bien; escríbanlo en el onboarding, no en un doc interno.

### 4. Keyed-hash — la premisa del debate es falsa

- SHA-256/BLAKE3 sobre 5MB toma **~10–30ms en cualquier SoC de los últimos años** (cientos de MB/s, y varios SoC tienen extensiones SHA de ARMv8). No es un costo de hot path si se hace fuera del hilo de UI — y la captura ya es asíncrona.
- **UUID-para-imágenes es la sugerencia más peligrosa de este archivo:** rompe la dedup contra desktop y rompe `sync_id`/`content_id` de §5.9, que es la base del dedup multi-dispositivo. Crearías dos esquemas de identidad que después hay que reconciliar en sync. No.
- Además es discusión vacía para v1: el doc ya dice texto-only (§4.6). Cuando lleguen imágenes: mismo keyed-hash, en background, listo. Las fases "chunking" y "hardware acceleration" del debate son soluciones a un problema que no va a existir.

### 5. mlock — la sugerencia del Keystore está técnicamente mal

- **Android Keystore no guarda contenido. Guarda claves y ejecuta operaciones criptográficas.** No podés meter "contenido sensible en RAM protegida" del Keystore; no existe esa API. El plaintext del clip siempre va a estar en la RAM normal del proceso (y peor, en el heap de la JVM, ver punto 1).
- `mlock` en Android funciona pero `RLIMIT_MEMLOCK` suele ser 64KB por proceso: alcanza para claves derivadas en el lado Rust, jamás para historial. Y es menos importante que en desktop porque no hay swap a disco (zram).
- Modelo correcto y honesto: **Keystore = claves (no exportables, hardware-backed). Rust = zeroize de buffers propios + mlock de material de clave si entra en 64KB. JVM = zona sin garantías, minimizar tiempo de vida del plaintext.** Punto. No prometan más que eso en la documentación de seguridad.

### El riesgo que este debate no menciona: nadie cambia de teclado

El mayor riesgo del proyecto mobile no está en ninguna de las 5 preguntas. Es este: **el IME como reemplazo de Gboard es una apuesta de adopción brutal.** La predicción es la razón por la que la gente no cambia de teclado, y un n-gram local en ES-AR va a sentirse notablemente peor que Gboard desde el minuto uno. Si Lapacho mobile requiere "abandoná tu teclado de siempre", muere ahí, con la mejor criptografía del mundo adentro.

**Sugerencia práctica:** reposicionar el IME v1 como **teclado de pegado momentáneo**, el patrón de los password managers (Bitwarden, KeePassDX): el usuario cambia a Lapacho con el ícono del teclado del sistema, pega su clip, vuelve a Gboard. Consecuencias:

- `lapacho-predict` sale del camino crítico (P2 → post-P4 o nunca). Es el crate más caro en esfuerzo/valor de todo el diseño.
- P0–P1 se achican: strip de historial + commit de texto + settings. Eso es un producto usable.
- El cold start del IME importa aún más (el usuario cambia de teclado *para* pegar), lo cual refuerza top-N + conexión viva, ya decidido.
- Si algún día la predicción es buena, el upgrade a "teclado full-time" es opt-in, no requisito de entrada.

### Resumen de veredictos

| Pregunta | Veredicto |
|---|---|
| 1. Disco+TTL vs nunca-en-disco | Disco+TTL es correcto, pero por las razones de Android (kill del IME, no swap). Wipe = rotación de clave Keystore, no DELETE. |
| 2. Dos procesos | WAL + busy_timeout + transacciones cortas. Sin ContentProvider hasta tener números que lo pidan. No es problema abierto. |
| 3. Sync | Diferir sí; pero saltear el paso "LAN-only WG" del debate. Memoria → relay SER5 → direct. Columnas placeholder en P0. |
| 4. Keyed-hash | La premisa de costo es falsa. Mismo esquema de ID para todo, hash en background. Nada de UUID para imágenes. |
| 5. mlock/Keystore | Keystore no guarda contenido (error del debate). Keystore=claves, zeroize en Rust, JVM sin garantías — documentarlo así. |
| (no preguntado) Adopción | Riesgo #1 del proyecto. IME v1 = teclado de pegado momentáneo; matar `lapacho-predict` del camino crítico. |

## Segunda ronda (2026-07-16): competencia, reuso, credibilidad, iOS, relay hosted

### Competencia y posicionamiento

El espacio "clipboard sync E2E" ya está ocupado:

| Proyecto | Qué hace | Qué NO tiene |
|---|---|---|
| **UniClipboard** (Rust+Tauri 2, AGPL-3.0, activo 2026) | Sync tiempo real Mac/Win/Linux/iOS/Android, P2P local-first, E2E, relay propio opcional, historial cifrado | Ningún concepto de sensibilidad: sincroniza *todo*, ciego |
| **ClipCascade** (AGPL, Docker/JAR) | Sync automático self-hosted con E2E, 4 plataformas | Ídem: replica el clipboard entero |
| **FlorisBoard / HeliBoard** | Teclados open source privacy-first con clipboard integrado | Sin cifrado por clase, sin sync, sin política |
| **KDE Connect** | Clipboard sync LAN | Sin E2E per-item, sin clases |
| **Bitwarden / KeePassDX (Magikeyboard)** | Teclado de pegado de credenciales | Solo vault de passwords, no clipboard general |

**Conclusión:** el sync E2E de clipboard ya es commodity. **Lo que nadie hace es la clasificación de sensibilidad como política** (PersistLevel × per-item × clase → qué persiste, con qué TTL, qué puede entrar al outbox). Esa es la tesis defendible de Lapacho. No competir en transporte; competir en política.

### Sync: reusar iroh, no forkear UniClipboard

- UniClipboard es AGPL-3.0: vendorear su código obliga a Lapacho a ser AGPL (decisión de licencia a tomar consciente, no por accidente).
- Su transporte no es propio: usa **iroh** (n0-computer, MIT/Apache-2.0) — P2P, NAT traversal, QUIC, relay fallback, relay self-hosteable.
- **Propuesta que modifica S1/S2:** `lapacho-sync` conserva la capa propia (policy, envelopes, outbox — el diferencial) y usa **iroh como adaptador de transporte** en lugar de escribir `relay_http.rs` + `direct.rs` a mano. El relay en SER5 sigue posible (iroh relay self-hosted). Revisar en P4a: si iroh resulta pesado para el companion, el diseño de `transport/trait.rs` permite volver al HTTP simple sin tocar policy/envelope.
- iroh vive solo en el companion/desktop; el IME sigue sin red (sin cambio).

### IME: no existe Rust IME — cáscara Kotlin fina

- No hay ningún `InputMethodService` en Rust utilizable (lo único que existe, android-ime-rs, resuelve el problema inverso: apps Rust *recibiendo* texto de un IME).
- El teclado se escribe en Kotlin, punto. Con el reposicionamiento a "teclado de pegado", esa capa es chica.
- **Modelo a estudiar:** Magikeyboard de KeePassDX (teclado mínimo de pegado de credenciales). **Referencia de detalles** (lifecycle, campos password, accesibilidad): HeliBoard (Apache-2.0) — para leer, no para forkear.
- Regla: Kotlin = cáscara; toda lógica en Rust vía uniffi.

### Credibilidad: la escalera real (no endorsements)

Descartar la vía "padrinos" (Stallman endorsa licencias, no seguridad; Torvalds no endorsa apps; Acton/Koum tampoco). Nadie serio endorsa sin track record. La escalera que sí funciona, en orden:

1. **Open source + builds reproducibles** — decisión de arquitectura de proyecto, tomarla AHORA (licencia, repo público, CI reproducible). Para un IME es existencial: Android muestra "esta app puede recopilar todo lo que escribís" al activarlo; sin código abierto verificable, nadie racional activa un teclado de seguridad.
2. **F-Droid** — compilan desde el fuente; su inclusión ES la verificación independiente que mira la audiencia target.
3. **Threat model publicado + SECURITY.md** — auditable empieza por documentado.
4. **Auditoría profesional** (Cure53 / Trail of Bits / Radically Open Security) — decenas de miles de USD, pero **NLnet/NGI financia exactamente este perfil** (privacidad, open source, self-hosted) y sus grants suelen cubrir la auditoría. Vía realista sin presupuesto. Es el camino que hicieron Signal, Tuta y Bitwarden.

### iOS: ni ahora ni never — barato desde ahora

Tres costos casi nulos hoy que evitan el "never":

1. **uniffi genera bindings Swift además de Kotlin** — el puente ya es inversión compartida.
2. **Medir RAM del core en P0** contra el techo ~60-70MB de las keyboard extensions de iOS. Si `core`+SQLite no entran, mejor saberlo ahora.
3. **Cero Android-ismos en `lapacho-core`/`lapacho-sync`** (regla ya existente; ahora con motivo extra).

Además: las keyboard extensions de iOS no tienen red por defecto → la regla más dura de Apple ya se cumple por diseño (IME sin red). Y sobre el usuario iOS: es el mercado que *paga* por privacidad — apps privacy-first cobran suscripción con menos fricción ahí. No es cliente Quebracho-infra, pero sí cliente Lapacho-producto. El modelo "teclado de pegado" funciona idéntico en iOS.

### Relay hosted: qué significa realmente (punto 5 expandido)

En cuanto un usuario sin servidor usa sync, alguien opera el relay. Aunque solo vea ciphertext:

- **Disponibilidad:** uptime que otros necesitan (deja de ser hobby).
- **Abuso:** un mailbox store-and-forward gratis = storage gratis para abusar → cuotas por chain, tamaño máximo de sobre, TTL server-side corto (diseñarlo en el protocolo desde P4, no después).
- **Metadata:** el relay ve IPs, timing, cantidad de dispositivos. "No podemos leer tu clipboard" es demostrable (protocolo publicado + builds reproducibles); "no sabemos nada de vos" es falso — no prometerlo.
- **Lo bueno:** sobres de KBs con retención de horas = costo de operación ínfimo → candidato natural a tier gratuito.

### Adelanto: QuebrachOS SaaS × Lapacho (debate aparte, opinión preliminar)

- La tesis "reducir costo de Google Workspace para PYMEs" es mercado real, pero **no competir en precio contra Google** (se pierde siempre). Wedge: soberanía + costo + soporte local en castellano.
- **No multi-tenant.** Multi-tenant = reescribir todo. **Single-tenant gestionado** (una instancia/VPS por cliente, provisionada y actualizada por el control plane `quebrachos` existente) convierte la infra actual en producto casi sin reescritura, y "tu empresa tiene SU servidor" es literalmente el pitch de soberanía.
- **Cruce con Lapacho:** cada instancia QuebrachOS incluye su relay de Lapacho gratis (costo ~cero). El SaaS resuelve la distribución de Lapacho para no-técnicos; Lapacho es amenity diferencial del SaaS. Ahí "más allá de Quebracho" y "SaaS Quebracho" se refuerzan en vez de competir.

## Próximos pasos

1. **Lee esto** (el archivo actual)
2. **Pide a Claude** que dé su opinión
3. **Decidir** si la arquitectura es correcta o necesita cambios

**Nota para Claude:** No es un debate teórico. Estamos a punto de implementar. Necesito tu opinión práctica, con riesgos concretos y recomendaciones de implementación.

---

*Este archivo reemplaza a `ARQUITECTURA_MOBILE_ANDROID.md` en el debate. La versión técnica completa está en ese archivo, pero esta es la versión accesible.*
