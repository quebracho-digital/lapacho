//! Cifrado en reposo (AES-256-GCM) para el contenido del historial.
//!
//! `lapacho-core` no decide de dónde sale la clave: recibe una clave de 32
//! bytes y la usa. La gestión de la clave (generación, almacenamiento seguro,
//! derivación desde passphrase) es responsabilidad de la app consumidora —
//! ver `apps/desktop` para la implementación basada en archivo.
//!
//! Formato del blob: `base64( nonce(12) || ciphertext+tag )`. Cada operación
//! usa un nonce aleatorio nuevo, así que cifrar dos veces el mismo texto da
//! blobs distintos. GCM verifica integridad: un blob manipulado o una clave
//! incorrecta hacen fallar el descifrado.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Largo de la clave AES-256, en bytes.
pub const KEY_LEN: usize = 32;
/// Largo del nonce de GCM, en bytes.
const NONCE_LEN: usize = 12;

/// Genera una clave AES-256 aleatoria con el RNG del sistema operativo.
pub fn generate_key() -> Result<[u8; KEY_LEN], String> {
    let mut key = [0u8; KEY_LEN];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    Ok(key)
}

/// Cifra `plaintext` y devuelve `base64(nonce || ciphertext)`.
pub fn encrypt(plaintext: &str, key: &[u8; KEY_LEN]) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("cifrado falló: {e}"))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(STANDARD.encode(blob))
}

/// Descifra un blob producido por [`encrypt`]. Falla si la clave es incorrecta
/// o si el contenido fue manipulado.
pub fn decrypt(blob_b64: &str, key: &[u8; KEY_LEN]) -> Result<String, String> {
    let blob = STANDARD.decode(blob_b64).map_err(|e| format!("base64: {e}"))?;
    if blob.len() < NONCE_LEN {
        return Err("blob demasiado corto".to_string());
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| "descifrado falló (clave incorrecta o datos manipulados)".to_string())?;

    String::from_utf8(plaintext).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; KEY_LEN] {
        [7u8; KEY_LEN]
    }

    #[test]
    fn round_trip() {
        let key = test_key();
        let blob = encrypt("hola mundo", &key).unwrap();
        assert_eq!(decrypt(&blob, &key).unwrap(), "hola mundo");
    }

    #[test]
    fn ciphertext_hides_plaintext() {
        let blob = encrypt("SECRETO", &test_key()).unwrap();
        assert!(!blob.contains("SECRETO"));
    }

    #[test]
    fn fresh_nonce_each_time() {
        let key = test_key();
        assert_ne!(encrypt("x", &key).unwrap(), encrypt("x", &key).unwrap());
    }

    #[test]
    fn wrong_key_fails() {
        let blob = encrypt("dato", &test_key()).unwrap();
        let mut other = test_key();
        other[0] ^= 0xFF;
        assert!(decrypt(&blob, &other).is_err());
    }

    #[test]
    fn tampered_blob_fails() {
        let key = test_key();
        let blob = encrypt("dato", &key).unwrap();
        let mut raw = STANDARD.decode(&blob).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        let tampered = STANDARD.encode(raw);
        assert!(decrypt(&tampered, &key).is_err());
    }

    #[test]
    fn generated_keys_are_random() {
        assert_ne!(generate_key().unwrap(), generate_key().unwrap());
    }

    #[test]
    fn unicode_round_trip() {
        let key = test_key();
        let s = "café ☕ 日本語 •••• \n\t fin";
        assert_eq!(decrypt(&encrypt(s, &key).unwrap(), &key).unwrap(), s);
    }

    #[test]
    fn empty_string_round_trip() {
        let key = test_key();
        assert_eq!(decrypt(&encrypt("", &key).unwrap(), &key).unwrap(), "");
    }
}
