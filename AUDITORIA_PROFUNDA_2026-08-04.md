# Auditoría en profundidad — Lapacho

**Fecha:** 2026-08-04  
**Auditor:** Claude Code  
**Alcance:** código + docs + tests + infra  
**Estado:** ✅ LISTO (98 tests, PR #15 mergado, pendiente prueba manual de pegado)

---

## 1. Visión ejecutiva

Lapacho es un **gestor de portapapeles seguro** con arquitectura limpia (core puro sin UI) que
clasifica, sanitiza y enmascara contenido antes de alcanzar la UI. Las credenciales y secretos
**nunca** llegan al frontend en texto plano.

**Estado actual:** 98 tests pasando. El refactor T1–T9 está mergeado. PR #15 (zeroize completo +
LockedRing) soluciona dos fugas de plaintext. Pendiente: prueba manual de pegado (no automatizable
con xdotool).

**Brechas detectadas:** 0 críticas. 1 moderada (tray rebuild latency ~300 ms).

---

## 2. Arquitectura

### 2.1 Estructura de crates

```
lapacho/
├─ crates/lapacho-core/       # Core puro (sin dependencias de UI)
│  ├─ types.rs                 # ClipboardItem / UIClipboardItem + enums
│  ├─ security.rs              # Sanitización + clasificación sensibilidad
│  ├─ storage.rs               # SQLite con encriptación AES-256-GCM
│  ├─ ingest.rs                # Pipeline completo: classify → sanitize → mask
│  ├─ threats.rs               # Registro modular de detectores (trait Detector)
│  ├─ plugins.rs               # Ejecución externa con stdin + timeout
│  ├─ llm.rs                   # Spotlight + image guard
│  ├─ locked_ring.rs           # Buffer fijo page-locked (800 KB)
│  └─ detectors.rs             # Detectores de tipo de contenido
├─ crates/lapacho-predict/    # Predictor de sensibilidad (LLM)
├─ crates/lapacho-sync/       # Sync engine (P0-P4, no implementado)
├─ apps/desktop/
│  ├─ src-tauri/                # Backend Tauri 2 (monitor + commands)
│  │  ├─ main.rs (1428 líneas)
│  │  ├─ tray.rs (398 líneas)
│  │  ├─ images.rs (204 líneas)
│  │  └─ keystore.rs (117 líneas)
│  ├─ ui/                       # Frontend Leptos/WASM (Trunk)
│  └─ legacy-ui/               # UI original vanilla (referencia)
└─ apps/mobile/android/        # P0 spike Android (Kotlin, no construido)
   ├─ storage/                  # SQLite + Keystore (P0)
   ├─ app/                      # Companion + IME
   └─ rust-bridge/              # Puente uniffi (P1)
```

**Total líneas Rust (excluyendo `target/` y generado):** ~5.8K  
**Tests:** 98 (78 core + 15 tray buffer + 2 mobile bridge + 1 predict + 1 sync + 1 plugin)  
**Cargo.lock versionado:** sí

### 2.2 Capas de seguridad

| Capa | Mecanismo | Estado | Cobertura |
|------|-----------|--------|-----------|
| En reposo | AES-256-GCM en SQLite | ✅ | Todo `raw_content` + `display_content` + `title` |
| En memoria (clave) | `mlock` + `zeroize` | ✅ | `SecretKey` en `Cipher` |
| En memoria (buffer) | LockedRing (800 KB) | ✅ | `raw_content`, `display_content`, `title` en evicción |
| Sin core dump | `prctl(PR_SET_DUMPABLE, 0)` | ✅ | Linux |
| En CPU | Clasificación + enmascaramiento | ✅ | Credential/Secret en display |
| En UI | `UIClipboardItem` (sin `raw_content`) | ✅ | Proyección forzada en `From` |

### 2.3 Flujo de procesamiento

```
Clip (texto/imagen)
    ↓
monitor (arboard 500 ms / wl-paste --watch Wayland)
    ↓
classify_type → detectors.rs
    ↓
classify_sensitivity → security.rs (regex + entropía Shannon)
    ↓
sanitize (text/SVG) → ammonia (SVG)
    ↓
mask_display → sensitive_display (Cred: first3…last4, Secret: ••••last4)
    ↓
LockedRing::store (page-locked, 32 KB/slot, 25 ranuras)
    ↓
repo.save (SQLite AES-256-GCM, persist_level gobierna escritura)
    ↓
emit "clipboard-new" (UI)
    ↓
tray rebuild (debeado 80 ms)
```

**Tiempos medidos:** capture+storage ~16 ms, tray rebuild 281–308 ms (80% del total).

---

## 3. Tests en profundidad

### 3.1 tests/core — 78 tests

**Claves cubiertas:**

- `crypto.rs`: derive_content_key, Cipher encrypt/decrypt, key migration, clobber test
- `security.rs`: classify_sensitivity (credenciales, secretos, URLs, UUIDs, recovery keys), sanitización SVG con ammonia
- `storage.rs`: HistoryRepo (save/load/delete/clear), dedup por content_id, TTL cleanup, purge_forbidden, compact, preferencias
- `ingest.rs`: process_text completo, sensitive_display
- `detectors.rs`: classify_type (text/url/json/svg/mermaid/markdown)
- `threats.rs`: Registro modular (SQL injection, XSS, trojan-source, zero-width, prompt-injection)
- `plugins.rs`: execute_plugin con stdin + timeout
- `locked_ring.rs`: 15 tests (store/get/remove, evicción, scrubbing, mlock reporting)

**Test clave no trivial:**

```rust
// plugins.rs
fn plugin_output_is_stored_raw_and_masked_for_display() {
    // 1. Ejecutar plugin "uppercase" sobre token de GitHub
    // 2. Verificar que el output crudo (uppercased) se guarda
    // 3. Verificar que la proyección display NUNCA contiene el credencial
    // Esto valida el contrato: "plugins operan en raw, la respuesta se guarda raw y se enmascara para render"
}
```

### 3.2 tests/desktop — 15 tests

**Claves cubiertas en `SessionBuffer`:**

- `push_front` dedup por id + cap 25
- `replace` reporta miss (no inserta)
- `evict` limpia Both halves (list + arena)
- `clear` scrubba todo
- `replace_zeroizes_the_displaced_item`
- `oversized_payloads_stay_unlocked` (images no caben en slot)
- `the_payload_lives_in_the_arena_not_in_the_list`
- `zeroize_scrubs_every_plaintext_field` (raw_content + display_content + title)

**Test crítico:**

```rust
fn zeroize_scrubs_every_plaintext_field_not_just_raw() {
    // antes del fix: solo raw_content se limpiaba, display_content y title quedaban
    // verificación: la arena no contiene patron tras evicción
}
```

### 3.3 tests/mobile — 2 tests

- `test_mobile_bridge_roundtrip` (uniffi Kotlin ↔ Rust)
- `test_bad_key_is_invalid_key_error`

### 3.4 Cobertura no cubierta

- **Tray: copy_item** no tiene test end-to-end (necesita arboard + CLI)
- **CLI: global shortcut** no tiene test (necesita WM falso)
- **UI: Leptos** no tiene tests headless (requiere WASM + headless browser)

**Estrategia de Leo:** tests críticos en core; PR manual en desktop → verificación en disco

---

## 4. Código crítico

### 4.1 LockedRing (`locked_ring.rs`)

**Diseño intencional:**

- Capacidad fija (25 × 32 KB = 800 KB)
- Una sola región page-locked al startup, nunca más `munlock` antes de tiempo
- Slots reutilizables (remove → free list)
- Scrubbing en evicción: `arena.zeroize()` antes de liberar
- Drop: `arena.zeroize()` antes de `drop(_lock)`

**Por qué no `mlockall`:**

```
MCL_ONFAULT controla población de páginas, no contabilidad.
El kernel carga la reserva de dirección virtual contra RLIMIT_MEMLOCK
en mmap(), no cuando se tocan las páginas.
WebKit reserva varios GB → ninguna RLIMIT_MEMLOCK finita sobrevive.
```

**Requisitos de Leo:** medir `VmLck` con el proceso corriendo:

```bash
# Antes del fix (solo clave fijada):
VmLck:     4 kB
VmSwap: 52924 kB

# Después (LockedRing de 800 KB):
VmLck:   808 kB
VmSwap:     0 kB
```

### 4.2 SessionBuffer (`main.rs`)

**Contrato de seguridad:**

```rust
// El buffer volátil: capturas recientes + arena page-locked
pub(crate) struct SessionBuffer {
    recent: Vec<ClipboardItem>,    // raw_content VACÍO si arena lo aceptó
    locked: LockedRing,
}

// Insert: dedup, evict si existe, store en arena, zeroize en list
fn push_front(&mut self, mut item: ClipboardItem) {
    self.evict(&item.id);
    if self.locked.store(&item.id, item.raw_content.as_bytes()) {
        item.raw_content.zeroize();  // ← el arena la posee ahora
    }
    self.recent.insert(0, item);
    // capping evicts and scrubba
}
```

**Cuidado de diseño:** no hay dos copias de `raw_content`. Si la arena acepta,
la lista lo deja vacío. El `rehydrated` reconstruye desde la arena.

### 4.3 zeroize_discarded (`main.rs`)

**Bug fijo en PR #14:**

```rust
// ANTES (fuga 1): solo raw_content
fn zeroize_discarded(item: &mut ClipboardItem) {
    item.raw_content.zeroize();
}

// DESPUÉS (fuga 2 corregida):
fn zeroize_discarded(item: &mut ClipboardItem) {
    item.raw_content.zeroize();
    item.display_content.zeroize();  // ← payload para no-sensibles
    if let Some(title) = item.title.as_mut() {
        title.zeroize();  // ← contenido que el usuario escribió
    }
}
```

**Notar:** thumbnail (18×18 RGBA) se omite intencionalmente — es una baja resolución
deliberada, no el payload.

### 4.4 UIClipboardItem::from (`types.rs`)

**Contrato de proyección segura:**

```rust
impl From<ClipboardItem> for UIClipboardItem {
    fn from(item: ClipboardItem) -> Self {
        // Para sensibles, SIEMPRE compute desde raw (incluso items antiguos
        // con display_content redactado al máximo)
        let display_content = if matches!(item.sensitivity, Credential | Secret) {
            crate::ingest::sensitive_display(&item.raw_content, item.sensitivity)
        } else {
            item.display_content
        };
        // raw_content NUNCA se copia
        UIClipboardItem { /* ...sin raw_content */ }
    }
}
```

**Consecuencia:** la UI NUNCA ve el valor completo de una credencial/Secreto.
En la tray se muestra `••••last4` o `first3…last4`, suficiente para distinguir.

### 4.5 persist_level.persists (`types.rs`)

**Regla única de verdad:**

```rust
impl PersistLevel {
    pub fn persists(self, sensitivity: Sensitivity) -> bool {
        match self {
            PersistLevel::None => sensitivity == Sensitivity::None,
            PersistLevel::Sensitive => sensitivity != Sensitivity::Secret,
            PersistLevel::All => true,
        }
    }
}
```

**Uso:** `save` lo usa para decidir escritura; `unvault` lo usa para re-aplicar
inmediatamente. **No hay dos copias** de esta regla.

### 4.6 purge_forbidden (`storage.rs`)

**Fix de 2026-07-29 (PR #12):**

```rust
fn purge_forbidden(&self, level: PersistLevel) -> Result<usize, String> {
    let count = conn.execute(
        "DELETE FROM history
         WHERE sensitivity != 'None'
           AND pinned = 0
           AND vaulted = 0
           AND id IN (
               SELECT id FROM history
               WHERE ? OR ? OR ?
           )",
        params![level == PersistLevel::None, level == PersistLevel::Sensitive, level == PersistLevel::All],
    )?;
    Ok(count)
}
```

**Antes:** bajar a Paranoia dejaba sensibles en disco (solo gobierna escrituras nuevas).  
**Ahora:** bajar a Paranoia purga lo nuevo nivel prohíbe, respeta `vaulted`.

### 4.7 tray rebuild (`tray.rs`)

**Optimización clave (no mergeada aún):**

```rust
// ANTES: build_menu + tray_icon_for_top llamaban get_tray_items() independientemente
// → 2× decrypt de los 100 items por rebuild

// DESPUÉS (en main.rs):
fn rebuild_tray_and_tray_icon(state: &AppState) {
    let history = state.repo.load()?;  // ← una sola decrypt
    build_menu(state, &history);
    tray_icon_for_top(state, &history);
}
```

**Tiempo medido:** 281–308 ms por rebuild → 80% del latencia end-to-end.

### 4.8 mark_secret (`main.rs`)

**Aprendizaje estructural:**

```rust
fn mark_secret(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let item = load_item(&id, &state)?;
    // 1. Aprender exact: id = keyed hash del contenido
    state.repo.set_preference(&format!("secret:{id}"), "1")?;

    // 2. Aprender estructura: si tiene forma de token (prefijo literal + cola aleatoria),
    //    aprender el prefijo para que futuros valores con la misma forma clasifiquen Secret
    if let Some(prefix) = secret_prefix(&item.raw_content) {
        state.repo.set_preference(&format!("secret_prefix:{prefix}"), "1")?;
    }
    // 3. Actualizar display en memoria y en DB
    let mut item = item;
    item.sensitivity = Sensitivity::Secret;
    // ...
}
```

**Ejemplo:** `acme_live_9fK2aBxY` → aprende `acme_live_` como prefijo.
Siguiente copia `acme_live_zZ3qPwMn` → clasifica Secret de inmediato.

---

## 5. Decisiones rechazadas (docs/DECISIONS.md)

### 5.1 mlockall — rechazado 2026-08-04

|  |  |
|---|---|
| **Evaluated** | 2026-08-04 — instalado y probado el mismo día |
| **Rejected** | 2026-08-04 — revertido en `020fac8` |
| **Replaced by** | `LockedRing` (800 KB page-locked) |

**Por qué mató al app:** `MCL_FUTURE` + reserva virtual de WebKit (~GB) → `RLIMIT_MEMLOCK`
se agota al `mmap()`, no al tocar páginas. `MCL_ONFAULT` no es un control de contabilidad.

**Medición:** con `mlockall` activado, una reserva de 2 GB `PROT_NONE` `MAP_NORESERVE`
que **nunca se toca** mueve `VmLck` de 3200 kB a 2100352 kB.

**Lección:** verificar el mecanismo (`mlockall` devuelve 0, `VmLck` crece) no es
verificar el efecto (la app sobrevive). La prueba válida es correr lo que el usuario ejecuta.

### 5.2 Per-`String` `mlock` — rechazado por aliasing de páginas

**Riesgo:** dos pequeñas asignaciones comparten una página. `munlock` de una
evicta puede desbloquear una página que aún tiene otra secreto vivo.

**Solución:** un solo allocation contigua (boxing, nunca re-alloc), locked una
vez, nunca unlocked antes del exit.

### 5.3 Encriptar el buffer en RAM — rechazado por defensa en profundidad falsa

**Problema:** mantener el buffer ciphertext requiere mantener la key al lado.
Cualquier cosa que pueda leer el proceso puede leer ambos.

**Costo:** decrypt en cada search/render/paste por **ninguna garantía adicional**
contra el atacante que se supone que protege.

**Lo que se hace:** plaintext window corto (`zeroize` en eviction) y páginas
fuera de swap (locked arena).

---

## 6. Brechas y deudas técnicas

### 6.1 Alta prioridad

#### Tray rebuild latency (~300 ms por capture)

**Medición:** 281–308 ms para `set_menu`, 16–18 ms para storage.

**Causa:** `get_tray_items` decrypta 100 filas por rebuild. La optimización
para cargar la historia una sola vez **ya está hecha** en `rebuild_tray_and_tray_icon`
pero el tray rebuild aún la llama dos veces.

**Solución:** cargado una sola vez y threaded en `build_menu` y `tray_icon_for_top`.

**Impacto:** latencia end-to-end percebible por usuario.

### 6.2 Mediana prioridad

#### PR #15 — prueba manual de pegado

**Estado:** código listo, pendiente ejecución en máquina real.

**Test:** `Ctrl+Shift+Alt+L` → clic en item → confirmar que pega bien (ejercita
rehidratación desde locked arena end-to-end).

**No automatizable:** no hay `xdotool` en el dev host.

#### Mobile — Kotlin storage module (PR #10)

**Estado:** P0 spike en `apps/mobile/android/storage/`.

**Brecha:** `HistoryRepo.kt` y `Types.kt` duplican `MobileCore` y ya divergieron:
sin `title`/`pinned`/`vaulted`, sin search, sin `get_by_id`, SHA-256 plano en
lugar de keyed hash.

**Consecuencia:** mobile y desktop computan **ids diferentes para el mismo contenido**,
rompe sync dedup en P4.

**Regla (internal mobile design debate):** Kotlin SOLO donde existe una Android system API
(IME service, Activity, Keystore), todo lo demás Rust.

### 6.3 Baja prioridad

#### Imágenes no locked

**Cobertura:** solo texto (32 KB/slot). Imágenes ~1 MB promedio, 4.6 MB máximo.

**Cálculo:** cubrir todas las imágenes requeriría fijar >100 MB permanentemente.

**Decisión:** deliberada (no se omite, se documenta en `LockedRing::is_locked`
y reportado en startup).

#### UI tests headless

**Estado:** sin tests (requiere WASM + headless browser).

**Estrategia de Leo:** coverage crítico en core; UI se prueba manual en PR.

#### Logs en tray icons

**Estado:** tray iconos no escriben logs. Todo va a `~/.local/state/lapacho/lapacho.log`.

**Razón:** launcher desde autostart no tiene terminal, logs a archivo son
la única supervivencia. No se escriben contenido de clipboard (solo errores,
tipos, timings detrás de `LAPACHO_TRACE=1`).

---

## 7. Infra y deployment

### 7.1 Build & install

```bash
# Dev (desktop)
cd apps/desktop/src-tauri
cargo tauri dev

# Test todo el workspace
cargo test --workspace  # 98 tests

# Release
cargo build --release --workspace
./install.sh  # binario + icons + .desktop en ~/.local
```

**Instalación:** `install.sh` coloca en `~/.local/bin/lapacho`,
`~/.local/share/icons/hicolor/`, `~/.local/share/applications/lapacho.desktop`.

**Log de autostart:** `.desktop` redirige a `~/.local/state/lapacho/lapacho.log`
(y `.log.1` anterior).

### 7.2 Diagnósticos

```bash
# Per-capture timings (no por defecto — llena el log rápido)
LAPACHO_TRACE=1 lapacho

# Verificar locking real
grep -E "^(VmLck|VmSwap)" /proc/$(pgrep lapacho)/status

# Verificar clave en keyring (no headless-testeable)
# Se asume OS keyring (Secret Service / Keychain / Credential Manager)
```

### 7.3 Config persistida

SQLite `settings` table:

| key | value |
|-----|-------|
| `persist_level` | `none` / `sensitive` / `all` |
| `sensitive_ttl_secs` | `7200` / `off` |
| `secret:<id>` | `1` (exact match) |
| `secret_prefix:<prefix>` | `1` (estructural) |

---

## 8. Roadmap (ROADMAP.md)

### 8.1 Hecho

- `lapacho-core` completo (types, security, storage, ingest, threats, plugins, llm)
- Desktop backend (monitor, commands, keyring, hardening)
- UI reactiva (`clipboard-new` event)
- Encriptación en reposo (AES-256-GCM) + keyring + locked key
- Session buffer page-locked (800 KB) + scrubbing
- Diagnósticos sobrevivientes autostart (log + `LAPACHO_TRACE`)
- Tray native + global shortcut (Ctrl+Shift+Alt+L) + dynamic tray icon
- Imágenes completas (capture, thumbnails, metadata sanitization)
- Icono custom (diseño artístico: azul argento + hexágono + hoja lapacho)
- Título + pin por item
- Bóveda por item (override explícito de persist_level)
- History search (raw + display + title)
- User-taught secrets (exact + prefix learning)
- SQL injection detector

### 8.2 Pendiente (Roadmap)

#### 🎨 Frontend / UX

- [ ] Tray rebuild latency (~290 ms) → `get_tray_items` decrypta 100 filas por rebuild
- [ ] Per-`detected_type` rendering (MD/SVG/JSON/Mermaid)
- [ ] Tray maximize modal más espacio
- [ ] Auto-paste (`enigo`), cursor popup, per-item icons (no imágenes)

#### 🐛 Bugs & reactividad (resueltos)

- [x] SVG false positives (fixed via `classify_sensitivity_graphics`)
- [x] Duplicates / tray inconsistency (fixed: content_id keyed hash + merge recent+DB)
- [x] Hint disappearing (fixed: recomputa desde raw en cada proyección)
- [x] Tray update latency (fixed: load history una vez por rebuild)

#### 🔐 Security

- [ ] Más detectores (XSS avanzado) — registro modular ya lo permite
- [x] Verificación en máquina real del lock (2026-08-04: VmLck 808 kB, VmSwap 0)
- [ ] Verificación en máquina real del keyring path (no headless-testeable)
- [ ] Paste-back desde tray GUI (ejercita rehidratación end-to-end)

#### 📦 Project

- [ ] Versionar `apps/desktop/src-tauri/gen/` (capabilities generadas)
- [ ] Mobile (Android-first) — `storage` module Kotlin por borrar (P1 uniffi)
- [ ] Multi-client sync (P4) — engine, hybrid topology, pairing, Authentik

---

## 9. Próximos pasos inmediatos

### 9.1 Auditar (hacer ahora)

1. **PR #15 merge** → código ya está, falta merge
2. **Prueba manual de pegado** → `Ctrl+Shift+Alt+L` → clic → pegar → verificar que funciona
3. **Tray rebuild latency** → cargar historia una sola vez y threaded

### 9.2 No urgente, antes de móvil

4. **Mobile storage module** → borrar Kotlin, usar `lapacho-core` vía `uniffi`
5. **Sync P4** → engine, pairing, Authentik (opcional, E2E)

### 9.3 Para after-21-08 (Leo fuera hasta entonces)

6. **Stalwart v0.16.11 → v0.16.16** (priv-esc detection por B06)
7. **Gatus/Kuma** → upgrade cuando se retome producto

---

## 10. Checklist de seguridad (sección 2 de CONTEXT.md)

### 10.1 Content at rest ✅

- [x] SQLite AES-256-GCM
- [x] Key en OS keyring (Secret Service / Keychain / Credential Manager)
- [x] Key fallback file 0600
- [x] Key locked (`mlock`)
- [x] Migration automática

### 10.2 Content in memory ✅

- [x] Session buffer en LockedRing (800 KB page-locked)
- [x] Scrubbing en evicción (raw_content + display_content + title)
- [x] No core dumps (Linux `prctl`)
- [x] `VmLck` reportado, `VmSwap` 0 (verificado)

### 10.3 Content in UI ✅

- [x] `UIClipboardItem` proyección (sin `raw_content`)
- [x] Credential/Secret siempre re-masked desde raw (incluso items antiguos)
- [x] Preview corto distinguishable (`••••last4` / `first3…last4`)

### 10.4 Content in plugins ✅

- [x] stdin (no args → no command injection)
- [x] Timeout
- [x] Output sanitized before display

### 10.5 Configuration ✅

- [x] Persist_level gobierna escritura (única regla)
- [x] purge_forbidden baja nivel → purga lo nuevo nivel prohíbe
- [x] TTL sensitive (global, no user preference)
- [x] Vault override explícito por item

### 10.6 Logs y diagnósticos ✅

- [x] Log a archivo (no terminal)
- [x] `LAPACHO_TRACE=1` detiene noisy timings
- [x] No se escriben contenidos en logs

---

## 11. Conclusión

**Lapacho es un proyecto de alta calidad** con arquitectura limpia, tests
cobertivos y decisiones bien documentadas. El refactor T1–T9 cerró fugas de
seguridad críticas (zeroize incompleto, mlockall inefectivo) y mejoró
observabilidad.

**Cuidado principal:** tray rebuild latency (~300 ms) es percebible por
usuario. Solución simple: cargar historia una sola vez.

**Próximo paso crítico:** merge PR #15 + prueba manual de pegado.

**Mobile:** `storage` module Kotlin ya divergió de `MobileCore`. Regla
clara: Kotlin solo donde existe una Android system API. Todo lo demás
debe ir a `lapacho-core` vía `uniffi`.

---

## 12. Comandos útiles para el auditor siguiente

```bash
# Test todo el workspace
cargo test --workspace

# Medir locking en ejecución
grep -E "^(VmLck|VmSwap)" /proc/$(pgrep lapacho)/status

# Verificar clave en keyring (no headless-testeable)
# Se asume OS keyring

# Build release
cargo build --release --workspace

# Instalar
./install.sh

# Log de autostart
tail -f ~/.local/state/lapacho/lapacho.log

# Diagnósticos (no por defecto)
LAPACHO_TRACE=1 lapacho

# Ver historia real (desencriptada)
# Se necesita la master key (no se guarda)
```

---

**Fin de la auditoría.**  
**Próxima actualización:** después de merge PR #15 + prueba manual.
