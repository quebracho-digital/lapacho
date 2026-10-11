use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use lapacho_core::{ClipboardItem, Sensitivity, mask_display, process_text};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::Chain;

const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;

/// One clip sealed for a chain: what a transport stores and moves, as JSON.
/// The header is readable (a transport needs `chain_id` for the mailbox and
/// `sync_id` to name the file) but bound to the ciphertext, so a transport
/// can't move a sealed clip under another header. Everything about the clip
/// itself — content, title, sensitivity — is inside the ciphertext.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub v: u8,
    pub chain_id: String,
    pub sync_id: String,
    /// The sending device's id, so it can skip its own envelopes on pull.
    pub sender: String,
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds. Past it, receivers don't import the envelope and delete
    /// it from the transport, so the mailbox is a hand-off, not an archive.
    pub expires_at: u64,
    /// `base64(nonce || AES-256-GCM(payload))`, the header as associated data.
    pub ciphertext: String,
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    raw: String,
    title: Option<String>,
    sensitivity: Sensitivity,
    timestamp: u64,
}

impl Envelope {
    fn aad(&self) -> Vec<u8> {
        // Length-prefixed, so no field can bleed into the next.
        let mut out = vec![self.v];
        for f in [&self.chain_id, &self.sync_id, &self.sender] {
            out.extend_from_slice(&(f.len() as u32).to_be_bytes());
            out.extend_from_slice(f.as_bytes());
        }
        out.extend_from_slice(&self.created_at.to_be_bytes());
        out.extend_from_slice(&self.expires_at.to_be_bytes());
        out
    }
}

fn cipher(chain: &Chain) -> Aes256Gcm {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(chain.key()))
}

/// Seals `item` for every device of `chain`, to be dropped at `expires_at`
/// (see [`crate::expires_at`]).
pub fn seal(
    chain: &Chain,
    sender: &str,
    item: &ClipboardItem,
    expires_at: u64,
) -> Result<Envelope, String> {
    let mut env = Envelope {
        v: VERSION,
        chain_id: chain.id().to_string(),
        sync_id: chain.sync_id(&item.raw_content),
        sender: sender.to_string(),
        created_at: now_secs(),
        expires_at,
        ciphertext: String::new(),
    };
    let plain = Zeroizing::new(
        serde_json::to_vec(&Sealed {
            raw: item.raw_content.clone(),
            title: item.title.clone(),
            sensitivity: item.sensitivity,
            timestamp: item.timestamp,
        })
        .map_err(|e| e.to_string())?,
    );
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let aad = env.aad();
    let ct = cipher(chain)
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: &plain, aad: &aad })
        .map_err(|_| "sealing failed".to_string())?;
    env.ciphertext = STANDARD.encode([nonce.as_slice(), &ct].concat());
    Ok(env)
}

/// Opens an envelope into an item for this device's history. The receiver
/// classifies the content again and keeps the stricter of its verdict and the
/// sender's, so a secret the sender was taught stays a secret here, and a
/// sender with an older classifier can't downgrade one.
pub fn open(chain: &Chain, env: &Envelope) -> Result<ClipboardItem, String> {
    if env.v != VERSION {
        return Err(format!("unknown envelope version {}", env.v));
    }
    if env.expires_at <= now_secs() {
        return Err("envelope expired".into());
    }
    let blob = STANDARD.decode(&env.ciphertext).map_err(|e| e.to_string())?;
    if blob.len() < NONCE_LEN {
        return Err("envelope too short".into());
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let aad = env.aad();
    let plain = Zeroizing::new(
        cipher(chain)
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: &aad })
            .map_err(|_| "envelope doesn't open: another chain, or tampered".to_string())?,
    );
    let sealed: Sealed = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
    // The id is a MAC of the content: a mismatch means a valid sender lied.
    if chain.sync_id(&sealed.raw) != env.sync_id {
        return Err("envelope id doesn't match its content".into());
    }

    let mut item = process_text(&sealed.raw);
    if sealed.sensitivity > item.sensitivity {
        item.sensitivity = sealed.sensitivity;
        item.display_content = mask_display(&item.display_content, item.sensitivity);
    }
    item.title = sealed.title;
    item.timestamp = sealed.timestamp;
    item.sync_id = Some(env.sync_id.clone());
    item.sync_state = "Synced".into();
    Ok(item)
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAR: u64 = u64::MAX;

    fn chain() -> Chain {
        Chain::create().unwrap()
    }

    #[test]
    fn round_trip() {
        let c = chain();
        let mut item = process_text("hola mundo");
        item.title = Some("saludo".into());
        let env = seal(&c, "laptop", &item, FAR).unwrap();
        assert!(!env.ciphertext.contains("hola"));
        let back = open(&c, &env).unwrap();
        assert_eq!(back.raw_content, "hola mundo");
        assert_eq!(back.title.as_deref(), Some("saludo"));
        assert_eq!(back.sync_id.as_deref(), Some(env.sync_id.as_str()));
    }

    #[test]
    fn another_chain_cant_open() {
        let env = seal(&chain(), "laptop", &process_text("x"), FAR).unwrap();
        assert!(open(&chain(), &env).is_err());
    }

    #[test]
    fn header_is_bound() {
        let c = chain();
        let env = seal(&c, "laptop", &process_text("x"), FAR).unwrap();
        for tamper in [
            |e: &mut Envelope| e.sender = "phone".into(),
            |e: &mut Envelope| e.created_at += 1,
            |e: &mut Envelope| e.sync_id = "0".repeat(64),
            |e: &mut Envelope| e.expires_at -= 1, // extending it would need the key too
        ] {
            let mut bad = env.clone();
            tamper(&mut bad);
            assert!(open(&c, &bad).is_err());
        }
    }

    #[test]
    fn the_stricter_sensitivity_wins() {
        let c = chain();
        let mut item = process_text("just words");
        item.sensitivity = Sensitivity::Secret; // taught on the sender
        let back = open(&c, &seal(&c, "laptop", &item, FAR).unwrap()).unwrap();
        assert_eq!(back.sensitivity, Sensitivity::Secret);
        assert_ne!(back.display_content, "just words");
    }

    #[test]
    fn same_clip_same_id() {
        let c = chain();
        let a = seal(&c, "laptop", &process_text("same"), FAR).unwrap();
        let b = seal(&c, "phone", &process_text("same"), FAR).unwrap();
        assert_eq!(a.sync_id, b.sync_id);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    #[test]
    fn an_expired_envelope_does_not_open() {
        let c = chain();
        let env = seal(&c, "laptop", &process_text("x"), now_secs()).unwrap();
        assert_eq!(open(&c, &env).unwrap_err(), "envelope expired");
    }
}
