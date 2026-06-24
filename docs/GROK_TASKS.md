# GROK_TASKS — Backlog masticado del refactor lapacho

> Decisiones cerradas en `ARQUITECTURA_REFACTOREO.md §10`. Acá están las tareas
> para ejecutar, **una por vez**. Grok hace una, Claude audita, sigue la próxima.

## Reglas para Grok (leer siempre)

1. **Una tarea por vez.** No empieces la siguiente hasta que Claude apruebe.
2. No toques nada fuera de los archivos listados en la tarea.
3. Cada tarea termina con **`cargo test --workspace` en verde**. Si no compila o
   un test falla, **pará y reportá** — no inventes arreglos en otros archivos.
4. No reescribas el core (`detectors`/`security`/`crypto`/`storage`): se extiende.
5. Si algo no está claro o el cambio te obliga a tocar más de lo escrito,
   **pará y preguntá**. No "mejores" de más.

Estado: `[ ]` pendiente · `[~]` en curso · `[x]` auditada y aprobada por Claude.

---

## [x] T1 — Helper de identidad por contenido (blake3 keyed)

> ✅ Auditada por Claude 2026-06-24: en scope, código correcto (dominio separado,
> no es hash en claro), 4 tests verdes, `cargo test --workspace` ok. blake3 1.8.5.

**Objetivo:** una función que dé un id estable por contenido, con clave (no un
hash en claro). Pura, sin integrar todavía.

**Archivos:** `Cargo.toml` (raíz), `crates/lapacho-core/Cargo.toml`,
`crates/lapacho-core/src/crypto.rs`.

**Hacer:**
1. En `Cargo.toml` raíz, dentro de `[workspace.dependencies]`, agregar:
   `blake3 = "1"`
2. En `crates/lapacho-core/Cargo.toml`, dentro de `[dependencies]`, agregar:
   `blake3 = { workspace = true }`
3. En `crypto.rs`, agregar estas dos funciones públicas:
```rust
/// Subclave de 32 bytes para identificar contenido, separada de la de cifrado
/// (dominio distinto). Misma clave maestra → misma subclave.
pub fn derive_content_key(master: &[u8; KEY_LEN]) -> [u8; 32] {
    blake3::derive_key("lapacho content-id v1", master)
}

/// Id estable por contenido (MAC con clave). Mismo `raw` y misma subclave → mismo
/// id (hex). Sin la subclave no se puede confirmar un valor adivinado.
pub fn content_id(content_key: &[u8; 32], raw: &str) -> String {
    blake3::keyed_hash(content_key, raw.as_bytes()).to_hex().to_string()
}
```

**NO tocar:** nada más de crypto.rs.

**Tests (agregar en el `mod tests` de crypto.rs):**
- mismo `raw` + misma subclave → mismo id.
- `raw` distinto → id distinto.
- subclave distinta (otra master) → id distinto para el mismo `raw`.
- el id es hex de 64 chars.

**Aceptación:** `cargo test --workspace` verde.

---

## [x] T2 — Exponer `content_id` en el repositorio

> ✅ Auditada por Claude 2026-06-24: en scope (solo storage.rs), correcto, test
> verde. **Hallazgo:** `content_key` quedó sin zeroize → ver T2b.

**Objetivo:** que el resto de la app pida el id de contenido sin conocer la clave.

**Archivos:** `crates/lapacho-core/src/storage.rs`.

**Hacer:**
1. En el `trait HistoryRepo`, agregar el método:
```rust
/// Id estable derivado del contenido (ver crypto::content_id). Mismo contenido
/// → mismo id, para deduplicar en disco y en los buffers vivos.
fn content_id(&self, raw: &str) -> String;
```
2. En `struct SqliteRepo`, agregar el campo `content_key: [u8; 32]`.
3. En `SqliteRepo::new`, derivar la subclave **antes** de construir el `Cipher`
   (que consume la key):
```rust
let content_key = crypto::derive_content_key(key.expose());
let repo = Self { db_path: db_path.into(), cipher: crypto::Cipher::new(&key), content_key };
```
4. Implementar en `impl HistoryRepo for SqliteRepo`:
```rust
fn content_id(&self, raw: &str) -> String { crypto::content_id(&self.content_key, raw) }
```

**NO tocar:** `save`/`load`/`cleanup`/dedup (eso es T4).

**Tests:** `repo.content_id("x")` estable entre llamadas; accesible tras
`Box<dyn HistoryRepo>`.

**Aceptación:** `cargo test --workspace` verde.

---

## [x] T2b — Zeroizar la subclave de contenido (hallazgo de auditoría T2)

> ✅ Auditada por Claude 2026-06-24: `content_key` ahora es `Zeroizing<[u8;32]>`,
> en scope, 58 tests verdes. mlock pendiente para Claude (junto a T8).

**Objetivo:** `content_key` es secreto: con la DB (que guarda los `id` = hash de
contenido en claro) permite un ataque de diccionario. El master ya está
mlock+zeroize; la subclave debe al menos zeroizarse al dropear.

**Archivos:** `crates/lapacho-core/src/storage.rs`.

**Hacer:**
1. Cambiar el campo de `SqliteRepo`:
   `content_key: zeroize::Zeroizing<[u8; 32]>`
   (`zeroize` ya es dependencia; `Zeroizing` borra al dropear y derefa a `[u8;32]`).
2. En `SqliteRepo::new`:
   `content_key: zeroize::Zeroizing::new(crypto::derive_content_key(key.expose()))`
3. En `content_id`, pasar `&*self.content_key` (deref a `&[u8; 32]`).

**NO tocar:** nada más.

**Tests:** el test `content_id_stable_between_calls_and_via_trait_object` sigue
verde (no agregar nada).

**Aceptación:** `cargo test --workspace` verde.
*(mlock del `content_key` lo evalúa Claude junto con T8.)*

---

## [x] T3 — Asignar el id por contenido en cada captura

> ✅ Auditada por Claude 2026-06-24: los 2 call sites correctos, solo main.rs,
> tests verdes + binario compila. Fix real del duplicado de secrets (tray_recent
> ya deduplica por contenido). Smoke GUI pendiente (lo corre Claude).

**Objetivo:** que todo item capturado tenga `id = content_id(raw)` antes de
guardarse/emitirse. Esto mata los duplicados de secrets (mismo contenido → mismo
id en lista, tray y DB).

**Archivos:** `apps/desktop/src-tauri/src/main.rs`.

**Hacer:**
1. En la closure `persist_and_emit` (dentro de `run_monitor`), que hoy recibe
   `|item: ClipboardItem|`, hacer que la **primera** acción sea recomputar el id:
```rust
let persist_and_emit = |item: ClipboardItem| {
    let mut item = item;
    item.id = repo.content_id(&item.raw_content);
    // ... resto igual (save, cleanup, emit, tray_recent, schedule_rebuild)
```
2. En `run_plugin`, después de `let item = process_text(&resp.result_raw_content);`
   cambiarlo a:
```rust
let mut item = process_text(&resp.result_raw_content);
item.id = state.repo.content_id(&item.raw_content);
```

**NO tocar:** `process_text` en el core (sigue generando un UUID que acá se pisa;
está bien así, no lo cambies). Tampoco `images.rs`.

**Tests / aceptación (manual, lo corre Claude):** copiar el mismo texto dos veces
con algo distinto en el medio → aparece UNA sola vez en lista y tray. `cargo
test --workspace` sigue verde.

---

## [x] T4 — Dedup por clave primaria en `save` (sacar el decrypt-scan)

> ✅ Auditada por Claude 2026-06-24: `existing_id_for_content` eliminado, upsert
> por PK correcto, test de recopia ajustado, 58 verdes. **G1 completo** (identidad
> keyed-hash de punta a punta).

**Objetivo:** como ahora `id == contenido`, deduplicar por PK (O(1)) en vez de
descifrar toda la tabla.

**Archivos:** `crates/lapacho-core/src/storage.rs`.

**Hacer:**
1. Borrar el método `existing_id_for_content` completo (y su doc).
2. En `fn save`, dejar el gate `should_save` igual, y reemplazar el bloque de
   dedup + INSERT por un upsert:
```rust
let enc_raw = self.cipher.encrypt(&item.raw_content)?;
let enc_display = self.cipher.encrypt(&item.display_content)?;
conn.execute(
    "INSERT INTO history
       (id, raw_content, display_content, content_type, sensitivity, detected_type, timestamp, thumbnail, size)
     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
     ON CONFLICT(id) DO UPDATE SET timestamp = excluded.timestamp",
    params![ item.id, enc_raw, enc_display, item.content_type,
        format!("{:?}", item.sensitivity), format!("{:?}", item.detected_type),
        item.timestamp, item.thumbnail, item.size.map(|s| s as i64) ],
).map_err(|e| e.to_string())?;
Ok(())
```
3. Arreglar el test `recopy_moves_to_top_instead_of_duplicating`: la recopia ahora
   llega con el **mismo id** (no "a-again"). Cambiar el item recopiado a:
```rust
// Mismo contenido ⇒ mismo id ("a"); timestamp nuevo.
let again = dummy("a", Sensitivity::None, 2000);
repo.save(&again, PersistLevel::All).unwrap();
```
   El resto del test (len==2, h[0].id=="a", timestamp 2000) queda igual.

**NO tocar:** los otros tests de storage (usan ids y contenidos distintos, siguen
válidos).

**Aceptación:** `cargo test --workspace` verde.

---

## [x] T5 — Centralizar el hint de sensibles (G2)

> ✅ Auditada por Claude 2026-06-24: helper correcto, 3 copias reemplazadas,
> Secret sin leak de prefijo, 58 verdes. **Hallazgo:** código muerto → T5b.

**Objetivo:** un solo helper para el preview de Credential/Secret (hoy hay 3
copias → drift). Regla: `Secret` → `••••last4`; `Credential` → `first3…last4`.
Nunca primeros chars de un `Secret`.

**Archivos:** `crates/lapacho-core/src/ingest.rs`,
`apps/desktop/src-tauri/src/tray.rs`, `crates/lapacho-core/src/types.rs`.

**Hacer:**
1. En `ingest.rs`, agregar:
```rust
/// Preview seguro para sensibles. Secret: solo ••••last4. Credential: first3…last4
/// (el prefijo del credential ayuda a distinguirlo; nunca se expone para Secret).
pub fn sensitive_display(raw: &str, sensitivity: Sensitivity) -> String {
    let last4: String = raw.chars().rev().filter(|c| !c.is_control())
        .take(4).collect::<Vec<_>>().into_iter().rev().collect();
    match sensitivity {
        Sensitivity::Credential => {
            let first3: String = raw.chars().filter(|c| !c.is_control()).take(3).collect();
            if first3.is_empty() && last4.is_empty() { "••••••••".into() }
            else { format!("{first3}…{last4}") }
        }
        _ => if last4.is_empty() { "••••••••".into() } else { format!("••••{last4}") },
    }
}
```
2. En `ingest.rs::mask_display`, la rama `Credential | Secret` ahora llama
   `sensitive_display(sanitized, sensitivity)`.
3. En `types.rs::UIClipboardItem::from`, donde calcula el display de sensibles,
   usar `crate::ingest::sensitive_display(&item.raw_content, item.sensitivity)`.
4. En `tray.rs::item_label`, para Credential/Secret, construir
   `sensitive_display(&item.raw_content, sensitivity)` y conservar el sufijo
   ` [secret]` / ` [credential]`.

**NO tocar:** la lógica de detección de sensibilidad (`security.rs`).

**Tests:** display de Credential contiene first3 y last4; display de Secret NO
contiene los primeros chars del secreto (solo `••••last4`); ninguno contiene el
medio.

**Aceptación:** `cargo test --workspace` verde.

---

## [x] T5b — Borrar código muerto que dejó T5

> ✅ Auditada por Claude 2026-06-24: borró solo `REDACTED` y `safe_preview`,
> `safe_credential_hint` intacta. Sus 2 warnings desaparecieron, 64 tests verdes.

**Objetivo:** T5 dejó sin uso `REDACTED` (ingest.rs) y `safe_preview` (tray.rs) →
2 warnings. Borrarlos.

**Archivos:** `crates/lapacho-core/src/ingest.rs`,
`apps/desktop/src-tauri/src/tray.rs`.

**Hacer:** borrar la constante `REDACTED` (ingest.rs) y la función `safe_preview`
(tray.rs). Ya nadie las usa.

**NO tocar:** `safe_credential_hint` (es `pub`, dejala). Nada más.

**Aceptación:** `cargo build --workspace` **sin** los warnings "constant
`REDACTED` is never used" ni "function `safe_preview` is never used"; `cargo test
--workspace` verde.

---

## [ ] T6 — Spike: captura XFIXES en background (verificar crate)

**Objetivo:** probar que podemos recibir eventos de clipboard en X11 **sin foco**.
Decidir el crate antes de integrar.

**Archivos:** un ejemplo aislado, p.ej. `apps/desktop/src-tauri/examples/xfixes_spike.rs`
(no tocar `main.rs` todavía).

**Hacer:** probar `clipboard-master` (primero) o `x11rb`/`x11-clipboard`. Escribir
un mini programa que imprima por stdout cada vez que cambia el portapapeles,
corriendo en segundo plano mientras copiás en OTRA app.

**Aceptación:** copiás texto en el navegador (sin que el spike tenga foco) y el
spike imprime el cambio al instante. Reportar a Claude qué crate quedó y su API.
**Si ningún crate da eventos en background, parar y avisar** (replanteamos).

---

## [ ] T7 — Integrar XFIXES en el monitor (reemplazar el poll)

> **Bloqueada hasta que T6 esté aprobada.** Claude da el detalle fino según el
> crate elegido. No empezar sin eso.

**Idea:** en `run_monitor`, donde hoy está el loop de polling (250 ms), poner la
captura por eventos XFIXES que en cada evento llame a `check_clipboard_once`
(reusar tal cual). Conservar el gate `last_seen` y el camino `wl-paste` detrás de
`is_wayland()` como fallback opcional.

---

## [ ] T8 — Zeroize del buffer efímero (primer paso de G3)

**Objetivo:** que el contenido sensible del buffer de sesión se borre de memoria
al ser evictado. (El `mlock` completo lo diseña/termina Claude — es delicado.)

**Archivos:** `apps/desktop/src-tauri/src/main.rs` (donde se hace
`rec.truncate(25)` y `rec.retain(...)` sobre `tray_recent`).

**Hacer:** antes de descartar un item del buffer (en truncate/retain/clear),
zeroizar su `raw_content` con el crate `zeroize` (ya es dependencia). Hacer un
helper chico que reciba el item a descartar y haga `item.raw_content.zeroize()`.

**NO tocar:** el modelo de mlock de la clave (`crypto.rs`). El mlock del buffer
en sí queda para Claude — no lo intentes.

**Aceptación:** `cargo test --workspace` verde; Claude audita que se zeroiza en
todos los caminos de descarte.

---

## [ ] T9 — Instrumentar latencia (con Claude)

**Objetivo:** medir dónde se va el tiempo captura→tray, en X11/Cinnamon.

**Archivos:** `apps/desktop/src-tauri/src/main.rs`, `.../tray.rs`.

**Hacer:** agregar `eprintln!` con `std::time::Instant` en: detección del cambio,
salida de `persist_and_emit`, entrada de `rebuild`, después de `set_menu`. Sin
cambiar lógica.

**Aceptación:** Claude corre `cargo tauri dev` en X11/Cinnamon, lee los números y
abre la tarea de optimización puntual (bajar debounce / cachear el load).
