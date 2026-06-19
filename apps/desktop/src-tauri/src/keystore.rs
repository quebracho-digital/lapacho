//! Provisioning of the AES-256 key that encrypts the history at rest.
//!
//! Precedence, by decreasing security:
//!  1. **OS keyring** (Secret Service / Keychain / Credential Manager) — the key
//!     never touches the plaintext filesystem and is unlocked by the login
//!     session. This is the default on a normal desktop.
//!  2. **File fallback** (`history.key`, 0600) — used when no keyring is
//!     available (headless boxes, servers, CI). Degrades gracefully instead of
//!     refusing to start.
//!
//! On the first run with a keyring present, an existing `history.key` from an
//! older version is migrated into the keyring and the plaintext file removed
//! (after verifying the keyring round-trips), so existing encrypted history
//! stays readable.
//!
//! The returned [`SecretKey`] is short-lived: it's handed to the repo, which
//! builds a resident (zeroized, mlocked) cipher from it and drops it.
//!
//! Hook for the future: a Vaultwarden-backed escrow / shared-key provider for
//! multi-device clipboard sync would slot in here as another source, cached into
//! the keyring for offline use. The rest of the app is unaffected.

use std::path::Path;

use keyring::Entry;
use lapacho_core::crypto::{self, SecretKey};

/// Keyring service name (matches the bundle identifier).
const KEYRING_SERVICE: &str = "digital.quebracho.lapacho";
/// Keyring entry name for the history encryption key.
const KEYRING_USER: &str = "history-encryption-key";
/// Plaintext fallback location, relative to the app data dir.
const KEY_FILE: &str = "history.key";

/// Resolves the encryption key, creating it on first run. Prefers the OS
/// keyring, falling back to a file when no keyring is reachable.
pub fn load_or_create_key(data_dir: &Path) -> Result<SecretKey, String> {
    let file_path = data_dir.join(KEY_FILE);
    match Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(entry) => from_keyring(&entry, &file_path),
        Err(e) => {
            eprintln!("lapacho: keyring no disponible ({e}); usando archivo {file_path:?}");
            from_file(&file_path)
        }
    }
}

fn from_keyring(entry: &Entry, file_path: &Path) -> Result<SecretKey, String> {
    match entry.get_password() {
        Ok(b64) => SecretKey::from_base64(&b64),
        Err(keyring::Error::NoEntry) => provision_keyring(entry, file_path),
        Err(e) => {
            // Keyring present but unreadable (locked, access denied): fall back to
            // the file so the app still works instead of losing the history.
            eprintln!("lapacho: no se pudo leer la clave del keyring ({e}); usando archivo");
            from_file(file_path)
        }
    }
}

/// No key in the keyring yet: migrate an existing file key (if any) or generate
/// a fresh one, store it in the keyring, and retire the plaintext file.
fn provision_keyring(entry: &Entry, file_path: &Path) -> Result<SecretKey, String> {
    let migrating = file_path.exists();
    let key = if migrating {
        read_key_file(file_path)?
    } else {
        SecretKey::generate()?
    };

    let b64 = key.to_base64();
    entry
        .set_password(&b64)
        .map_err(|e| format!("no se pudo guardar la clave en el keyring: {e}"))?;

    if migrating {
        // Only remove the plaintext file once we've confirmed the keyring holds
        // the same key — never leave the user with no readable key.
        let verified = matches!(entry.get_password().as_deref(), Ok(stored) if stored == b64.as_str());
        if verified {
            match std::fs::remove_file(file_path) {
                Ok(()) => eprintln!("lapacho: clave migrada del archivo al keyring del SO"),
                Err(e) => eprintln!(
                    "lapacho: clave migrada al keyring pero no pude borrar {file_path:?}: {e}"
                ),
            }
        } else {
            eprintln!(
                "lapacho: clave guardada en keyring pero la verificación falló; conservo {file_path:?}"
            );
        }
    }
    Ok(key)
}

fn read_key_file(path: &Path) -> Result<SecretKey, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let arr: [u8; crypto::KEY_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "archivo de clave con tamaño inválido".to_string())?;
    Ok(SecretKey::from_bytes(arr))
}

fn from_file(path: &Path) -> Result<SecretKey, String> {
    if path.exists() {
        return read_key_file(path);
    }
    let key = SecretKey::generate()?;
    std::fs::write(path, key.expose()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}
