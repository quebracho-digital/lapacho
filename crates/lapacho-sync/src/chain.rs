use bip39::Mnemonic;
use zeroize::Zeroizing;

const SEED_LEN: usize = 32;

/// The secret every device of one chain shares (decision S7: one shared key,
/// any device reads everything synced). Everything else derives from the
/// 32-byte seed, each key under its own context so none can stand in for
/// another.
///
/// The seed belongs in the OS keystore, never in the history database; the
/// app persists [`Chain::words`] and restores with [`Chain::from_words`].
pub struct Chain {
    seed: Zeroizing<[u8; SEED_LEN]>,
    /// Public: names the mailbox on a transport. Reveals nothing about the key.
    id: String,
    key: Zeroizing<[u8; 32]>,
    content_key: Zeroizing<[u8; 32]>,
}

impl Chain {
    /// A new chain, on the first device.
    pub fn create() -> Result<Self, String> {
        let mut seed = Zeroizing::new([0u8; SEED_LEN]);
        getrandom::getrandom(seed.as_mut_slice()).map_err(|e| e.to_string())?;
        Ok(Self::from_seed(seed))
    }

    fn from_seed(seed: Zeroizing<[u8; SEED_LEN]>) -> Self {
        let id = blake3::derive_key("lapacho sync chain-id v1", seed.as_slice());
        Self {
            id: blake3::Hash::from(id).to_hex().to_string(),
            key: Zeroizing::new(blake3::derive_key("lapacho sync envelope-key v1", seed.as_slice())),
            content_key: Zeroizing::new(blake3::derive_key("lapacho sync content-id v1", seed.as_slice())),
            seed,
        }
    }

    /// The 24 words (BIP-39, English) another device types or scans to join.
    pub fn words(&self) -> Zeroizing<String> {
        let m = Mnemonic::from_entropy(self.seed.as_slice()).expect("32 bytes is valid entropy");
        Zeroizing::new(m.to_string())
    }

    /// Joins the chain whose [`Chain::words`] these are. The checksum catches
    /// a mistyped or swapped word.
    pub fn from_words(words: &str) -> Result<Self, String> {
        let m = Mnemonic::parse_normalized(&words.trim().to_lowercase())
            .map_err(|e| format!("not a valid set of words: {e}"))?;
        let entropy = Zeroizing::new(m.to_entropy());
        let seed: [u8; SEED_LEN] = entropy
            .as_slice()
            .try_into()
            .map_err(|_| "expected 24 words".to_string())?;
        Ok(Self::from_seed(Zeroizing::new(seed)))
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn key(&self) -> &[u8; 32] {
        &self.key
    }

    /// The same clip gets the same id on every device of the chain, so a
    /// receiver dedups it. Keyed: without the chain, a guess can't be checked
    /// against an id a transport stores. (`lapacho_core::crypto::content_id`
    /// is keyed per device and can't serve here.)
    pub fn sync_id(&self, raw: &str) -> String {
        blake3::keyed_hash(&self.content_key, raw.as_bytes()).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_join_the_same_chain() {
        let a = Chain::create().unwrap();
        let b = Chain::from_words(&a.words()).unwrap();
        assert_eq!(a.id(), b.id());
        assert_eq!(a.key(), b.key());
        assert_eq!(a.sync_id("hola"), b.sync_id("hola"));
        assert_eq!(a.words().split(' ').count(), 24);
    }

    #[test]
    fn words_tolerate_case_and_spaces() {
        let a = Chain::create().unwrap();
        let typed = format!("  {}  ", a.words().to_uppercase());
        assert_eq!(Chain::from_words(&typed).unwrap().id(), a.id());
    }

    #[test]
    fn a_swapped_word_is_caught() {
        // A fixed seed: the checksum is 8 bits, so a random one would let
        // 1 swap in 256 through and the test would flake.
        let a = Chain::from_seed(Zeroizing::new(core::array::from_fn(|i| i as u8)));
        let mut w: Vec<String> = a.words().split(' ').map(String::from).collect();
        assert_ne!(w[0], w[1]);
        w.swap(0, 1);
        assert!(Chain::from_words(&w.join(" ")).is_err());
    }

    #[test]
    fn chains_differ() {
        let a = Chain::create().unwrap();
        let b = Chain::create().unwrap();
        assert_ne!(a.id(), b.id());
        assert_ne!(a.sync_id("x"), b.sync_id("x"));
        assert_ne!(a.id(), blake3::Hash::from(*a.key()).to_hex().to_string());
    }
}
