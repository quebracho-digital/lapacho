//! Cifrado en reposo (AES-256-GCM) y manejo en memoria del material de clave.
//!
//! `lapacho-core` no decide de dónde sale la clave: la app consumidora la
//! provisiona (keyring, archivo, derivación) y la entrega como [`SecretKey`].
//! Esa clave se usa para construir un [`Cipher`], que es **el secreto que queda
//! residente** mientras la app vive; por eso es el que vale la pena bloquear en
//! RAM (mlock) y borrar al soltarlo (zeroize).
//!
//! Defensa en profundidad para datos sensibles en memoria:
//!  - [`SecretKey`] se zeroiza al dropearse (clave transitoria de provisión).
//!  - [`Cipher`] guarda el *key schedule* de AES, que `aes-gcm` zeroiza al
//!    dropearse (feature `zeroize`), y con la feature `mlock` además fija esa
//!    memoria en RAM física para que nunca llegue al swap.
//!
//! Formato del blob: `base64( nonce(12) || ciphertext+tag )`. Cada operación
//! usa un nonce aleatorio nuevo, así que cifrar dos veces el mismo texto da
//! blobs distintos. GCM verifica integridad: un blob manipulado o una clave
//! incorrecta hacen fallar el descifrado.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use zeroize::Zeroize;

/// Largo de la clave AES-256, en bytes.
pub const KEY_LEN: usize = 32;
/// Largo del nonce de GCM, en bytes.
const NONCE_LEN: usize = 12;

// ---------------------------------------------------------------------------
// Material de clave
// ---------------------------------------------------------------------------

/// Clave AES-256 de 32 bytes: secreto de **provisión**, de vida corta.
///
/// Se zeroiza al dropearse. Existe sólo para mover la clave desde el keyring o
/// el archivo hacia un [`Cipher`]; una vez construido el cipher, esta clave se
/// puede (y conviene) soltar. Se guarda en un `Box` para que sus bytes no se
/// dupliquen al mover la estructura.
pub struct SecretKey {
    bytes: Box<[u8; KEY_LEN]>,
}

impl SecretKey {
    /// Genera una clave aleatoria con el RNG del SO, directamente en el heap.
    pub fn generate() -> Result<Self, String> {
        let mut bytes = Box::new([0u8; KEY_LEN]);
        getrandom::getrandom(bytes.as_mut_slice()).map_err(|e| e.to_string())?;
        Ok(Self { bytes })
    }

    /// Envuelve bytes ya existentes.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self {
            bytes: Box::new(bytes),
        }
    }

    /// Reconstruye una clave desde su forma base64 (ver [`SecretKey::to_base64`]).
    pub fn from_base64(s: &str) -> Result<Self, String> {
        Ok(Self {
            bytes: Box::new(key_from_base64(s)?),
        })
    }

    /// Codifica la clave como base64, para guardarla en un almacén de secretos
    /// que sólo acepta texto (p. ej. el keyring del SO).
    pub fn to_base64(&self) -> String {
        key_to_base64(self.expose())
    }

    /// Acceso de sólo lectura a los bytes, para construir un [`Cipher`].
    pub fn expose(&self) -> &[u8; KEY_LEN] {
        &self.bytes
    }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

// ---------------------------------------------------------------------------
// Motor de cifrado residente
// ---------------------------------------------------------------------------

/// Motor de cifrado construido desde una [`SecretKey`].
///
/// Es el secreto que queda **residente** durante toda la vida de la app, así que
/// es el que se protege fuerte: el *key schedule* vive dentro de `Aes256Gcm`
/// (zeroizado al dropear) y, con la feature `mlock`, su memoria queda fijada en
/// RAM física para que no pueda ir al swap.
pub struct Cipher {
    // El guard se declara antes que `inner` para que, al dropear, se libere el
    // mlock ANTES de que el Box libere la memoria.
    #[cfg(feature = "mlock")]
    _lock: Option<region::LockGuard>,
    inner: Box<Aes256Gcm>,
}

impl Cipher {
    /// Construye el motor desde una clave. La clave puede dropearse después.
    pub fn new(key: &SecretKey) -> Self {
        let inner = Box::new(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.expose())));

        #[cfg(feature = "mlock")]
        let _lock = {
            let ptr = inner.as_ref() as *const Aes256Gcm as *const u8;
            let len = core::mem::size_of::<Aes256Gcm>();
            match region::lock(ptr, len) {
                Ok(guard) => Some(guard),
                Err(e) => {
                    // Sin RLIMIT_MEMLOCK suficiente esto falla; seguimos sin
                    // mlock en vez de no arrancar.
                    eprintln!("lapacho: no se pudo mlock la clave en RAM ({e}); sigo sin fijar");
                    None
                }
            }
        };

        Self {
            #[cfg(feature = "mlock")]
            _lock,
            inner,
        }
    }

    /// Cifra `plaintext` y devuelve `base64(nonce || ciphertext)`.
    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        encrypt_with(&self.inner, plaintext)
    }

    /// Descifra un blob producido por [`Cipher::encrypt`]. Falla si la clave es
    /// incorrecta o si el contenido fue manipulado.
    pub fn decrypt(&self, blob_b64: &str) -> Result<String, String> {
        decrypt_with(&self.inner, blob_b64)
    }
}

// ---------------------------------------------------------------------------
// Operaciones de bajo nivel
// ---------------------------------------------------------------------------

fn encrypt_with(cipher: &Aes256Gcm, plaintext: &str) -> Result<String, String> {
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

fn decrypt_with(cipher: &Aes256Gcm, blob_b64: &str) -> Result<String, String> {
    let blob = STANDARD.decode(blob_b64).map_err(|e| format!("base64: {e}"))?;
    if blob.len() < NONCE_LEN {
        return Err("blob demasiado corto".to_string());
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);

    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| "descifrado falló (clave incorrecta o datos manipulados)".to_string())?;

    String::from_utf8(plaintext).map_err(|e| e.to_string())
}

/// Genera una clave AES-256 aleatoria con el RNG del sistema operativo.
pub fn generate_key() -> Result<[u8; KEY_LEN], String> {
    let mut key = [0u8; KEY_LEN];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    Ok(key)
}

/// Cifra `plaintext` con una clave cruda. Conveniencia para usos puntuales; el
/// camino residente debería usar [`Cipher`] (reusa el key schedule y lo protege).
pub fn encrypt(plaintext: &str, key: &[u8; KEY_LEN]) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    encrypt_with(&cipher, plaintext)
}

/// Descifra un blob con una clave cruda. Ver [`encrypt`].
pub fn decrypt(blob_b64: &str, key: &[u8; KEY_LEN]) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    decrypt_with(&cipher, blob_b64)
}

/// Codifica una clave como base64, para guardarla en un almacén de secretos que
/// sólo acepta texto (p. ej. el keyring del SO).
pub fn key_to_base64(key: &[u8; KEY_LEN]) -> String {
    STANDARD.encode(key)
}

/// Decodifica una clave producida por [`key_to_base64`]. Falla si el texto no es
/// base64 válido o no decodifica a exactamente [`KEY_LEN`] bytes.
pub fn key_from_base64(s: &str) -> Result<[u8; KEY_LEN], String> {
    let bytes = STANDARD
        .decode(s.trim())
        .map_err(|e| format!("base64: {e}"))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| "clave con tamaño inválido".to_string())
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

    #[test]
    fn key_base64_round_trip() {
        let key = generate_key().unwrap();
        let encoded = key_to_base64(&key);
        assert_eq!(key_from_base64(&encoded).unwrap(), key);
    }

    #[test]
    fn key_from_base64_rejects_bad_input() {
        assert!(key_from_base64("not base64!!!").is_err());
        // Valid base64 but wrong length (16 bytes, not 32).
        assert!(key_from_base64(&STANDARD.encode([0u8; 16])).is_err());
    }

    #[test]
    fn cipher_round_trip() {
        let key = SecretKey::from_bytes(test_key());
        let cipher = Cipher::new(&key);
        let blob = cipher.encrypt("contenido sensible").unwrap();
        assert!(!blob.contains("contenido"));
        assert_eq!(cipher.decrypt(&blob).unwrap(), "contenido sensible");
    }

    #[test]
    fn cipher_matches_raw_key_path() {
        // A Cipher and the free functions over the same key are interoperable.
        let raw = test_key();
        let cipher = Cipher::new(&SecretKey::from_bytes(raw));
        let blob = encrypt("x", &raw).unwrap();
        assert_eq!(cipher.decrypt(&blob).unwrap(), "x");
        assert_eq!(decrypt(&cipher.encrypt("y").unwrap(), &raw).unwrap(), "y");
    }

    #[test]
    fn secret_key_base64_round_trip() {
        let key = SecretKey::generate().unwrap();
        let restored = SecretKey::from_base64(&key.to_base64()).unwrap();
        assert_eq!(restored.expose(), key.expose());
    }
}
