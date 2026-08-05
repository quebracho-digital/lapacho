# Plan de migración mobile Kotlin → Rust

**Objetivo:** borrar `apps/mobile/android/storage/` (~225 líneas Kotlin) y usar `lapacho-core` vía `uniffi`.

**Estado actual:** P0 spike Kotlin (sin Rust aún), implementado en `apps/mobile/android/` (Kotlin-only).

---

## 1. Diagnóstico

### Archivos Kotlin a borrar

| Archivo | Líneas | Qué hace | Reemplazo |
|---------|--------|----------|-----------|
| `HistoryRepo.kt` | 185 | SQLite wrapper con AES-GCM | `lapacho_core::storage::SqliteRepo` |
| `Types.kt` | 40 | Modelos Kotlin | `lapacho_core::types` (via uniffi) |
| `LapachoCipher.kt` | 93 | Keystore + AES-GCM | `lapacho_core::crypto` (via uniffi) |

**Total:** ~318 líneas Kotlin

### Qué falta en `lapacho-core` para mobile

| Funcionalidad | Estado | Notas |
|---------------|--------|-------|
| `SqliteRepo` | ✅ | Ya implementado en Rust |
| `SecretKey` / `Cipher` | ✅ | `lapacho-core::crypto` |
| `ClipboardItem` / `UIClipboardItem` | ✅ | `lapacho_core::types` |
| Android Keystore | ⚠️ | Requiere `android-keystore` crate o FFI |
| `uniffi` bindings | ☐ | Generar bindings para Kotlin |

**Gap crítico:** `lapacho-core` no tiene Android Keystore integrado. La solución es:
- Opción A: `android-keystore` crate (Rust) → FFI
- Opción B: Kotlin Keystore → Rust (pasar `SecretKey` como bytes)

---

## 2. Plan de implementación

### Fase 0: Preparación (2-4 horas)

#### 0.1 Agregar `uniffi` al workspace

**Cargo.toml (raíz):**
```toml
[workspace]
members = [
    "crates/lapacho-core",
    "crates/lapacho-predict",
    "crates/lapacho-sync",
    "apps/mobile/android/rust-bridge",
]
```

#### 0.2 Crear `rust-bridge/` crate

**apps/mobile/android/rust-bridge/Cargo.toml:**
```toml
[package]
name = "lapacho-rust-bridge"
version = "0.1.0"
edition = "2024"

[lib]
name = "lapacho_rust_bridge"
crate-type = ["cdylib"]

[dependencies]
lapacho-core = { path = "../.." }
uniffi = { version = "0.28", features = ["tokio"] }
```

**apps/mobile/android/rust-bridge/src/lib.rs:**
```rust
uniffi::setup_scaffolding!();

#[uniffi::export]
fn history_repo_new(path: String) -> Box<dyn HistoryRepo> {
    Box::new(SqliteRepo::new(path))
}

// Exponer otros tipos necesarios...
```

#### 0.3 Generar bindings Kotlin

```bash
uniffi-bindgen --language kotlin \
  --out-dir apps/mobile/android/rust-bridge/src/main/kotlin \
  apps/mobile/android/rust-bridge/src/lib.rs
```

### Fase 1: Migrar `HistoryRepo` (1 hora)

#### 1.1 Kotlin (antes)

```kotlin
// apps/mobile/android/storage/src/main/java/.../HistoryRepo.kt
class HistoryRepoImpl(path: String) : HistoryRepo {
    private val db = SQLiteDatabase.openDatabase(path, null, CREATE)
    
    override fun save(item: ClipboardItem) {
        val cipher = LapachoCipher()
        val blob = cipher.encrypt(item.toBytes())
        db.insert("history", null, contentValuesOf(...))
    }
}
```

#### 1.2 Rust (después)

```rust
// apps/mobile/android/rust-bridge/src/lib.rs
use lapacho_core::storage::{HistoryRepo, SqliteRepo};

#[uniffi::export]
fn history_repo_new(path: String) -> Box<dyn HistoryRepo> {
    Box::new(SqliteRepo::new(path))
}
```

#### 1.3 Kotlin (después)

```kotlin
// apps/mobile/android/app/src/main/java/.../HistoryRepo.kt
import digital.quebracho.lapacho.rustbridge.LapachoRustBridge

class HistoryRepoImpl(path: String) : HistoryRepo {
    private val native = LapachoRustBridge.historyRepoNew(path)
    
    override fun save(item: ClipboardItem) {
        native.save(item.toRust()) // uniffi convierte automáticamente
    }
}
```

### Fase 2: Migrar `LapachoCipher` / Keystore (2-4 horas)

#### 2.1 Opción A: Rust con `android-keystore` crate

```toml
# Cargo.toml
android-keystore = "0.4"
```

```rust
// apps/mobile/android/rust-bridge/src/lib.rs
use android_keystore::{Keystore, KeystoreAlgorithm};

fn get_key_from_keystore() -> Result<SecretKey, String> {
    let keystore = Keystore::new()?;
    let key = keystore.get_key("lapacho-key")?;
    Ok(SecretKey::from_bytes(&key)?)
}
```

#### 2.2 Opción B: Kotlin Keystore → Rust

```kotlin
// apps/mobile/android/app/src/main/java/.../KeyManager.kt
class KeyManager {
    private val keystore = KeyStore.getInstance("AndroidKeyStore")
    
    fun getKeyBytes(): ByteArray {
        val entry = keystore.getEntry("lapacho-key", null)
        return (entry as KeyStore.SecretKeyEntry).secretKey.encoded
    }
}
```

```rust
// apps/mobile/android/rust-bridge/src/lib.rs
#[uniffi::export]
fn secret_key_from_bytes(bytes: Vec<u8>) -> Result<SecretKey, String> {
    SecretKey::from_bytes(&bytes)
        .map_err(|e| e.to_string())
}
```

### Fase 3: Actualizar `Types.kt` (30 minutos)

#### 3.1 Kotlin (antes)

```kotlin
// Types.kt
data class ClipboardItem(
    val id: String,
    val rawContent: String,
    val displayContent: String,
    val sensitivity: Sensitivity,
    // ...
)
```

#### 3.2 Rust + uniffi (después)

```rust
// apps/mobile/android/rust-bridge/src/lib.rs
use lapacho_core::types::ClipboardItem as CoreClipboardItem;

#[derive(uniffi::Record)]
pub struct UIClipboardItem {
    pub id: String,
    pub display_content: String,
    pub sensitivity: Sensitivity,
    // ...
}

#[uniffi::export]
fn from_core(item: CoreClipboardItem) -> UIClipboardItem {
    UIClipboardItem {
        id: item.id,
        display_content: item.display_content, // safe projection
        sensitivity: item.sensitivity,
        // ...
    }
}
```

#### 3.3 Kotlin (después)

```kotlin
// Types.kt (borrado, ahora usado directo desde uniffi)
// import digital.quebracho.lapacho.rustbridge.UIClipboardItem
```

### Fase 4: Limpieza (15 minutos)

1. Borrar `apps/mobile/android/storage/`
2. Actualizar `settings.gradle.kts`:
   ```kotlin
   include(":app", ":ime", ":rust-bridge")
   ```
3. Actualizar `build.gradle.kts`:
   ```kotlin
   dependencies {
       implementation(project(":rust-bridge"))
   }
   ```

---

## 3. Checklist de validación

Después de la migración, verificar:

- [ ] 98 tests pasan (`cargo test --workspace`)
- [ ] Build Kotlin exitoso (`./gradlew assembleDebug`)
- [ ] App comparte la misma DB que desktop (misma clave, mismo schema)
- [ ] Items copiados en desktop se ven en mobile (y viceversa)
- [ ] `VmLck` 808 kB, `VmSwap` 0 (verificado con `adb shell cat /proc/.../status`)
- [ ] Key en Android Keystore (no plaintext en disco)

---

## 4. riesgos y mitigaciones

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Keystore API diferente en Android versions | Alta | Medio | Soportar API nivel 23+ (Android 6.0+) |
| `uniffi` breaking changes | Media | Bajo | Pin version específica, CI tests |
| Divergencia en schema SQLite | Baja | Alto | Compartir `lapacho-core` (una sola fuente) |
| Performance peor en Kotlin | Baja | Medio | Benchmarks previos a merge |

---

## 5. Estimación

| Fase | Tiempo | Notas |
|------|--------|-------|
| Preparación (uniffi) | 2-4 horas | Config inicial |
| Migrar `HistoryRepo` | 1 hora | Directo |
| Migrar Keystore | 2-4 horas | Complex, depende de approach |
| Limpieza | 0.5 horas | Borrar y ajustar build |
| Tests y validación | 2 horas | CI + manual |
| **Total** | **7.5-11.5 horas** | ~1-1.5 días |

---

## 6. ¿Qué hacer ahora?

**Opción A: Implementar ahora** (requiere Android SDK/NDK)  
**Opción B: Dejar el plan documentado y esperar** (recomendado)  

**Recomendación:** Opción B.  
El plan está documentado aquí. Cuando Leo tenga Android SDK/NDK configurado, puede seguir estos pasos.

**Antes de eso, prioridades:**
- [ ] Merge PR #15 (ya merged, `06e30cb`)
- [ ] Prueba manual de pegado (ya verificado por Leo)
- [ ] Push de commits locales (1 commit: `eaa7e9b`)

---

**Última actualización:** 2026-08-04  
**Autores:** Claude Code
