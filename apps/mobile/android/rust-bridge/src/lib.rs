//! Mobile UniFFI bridge exposing lapacho-core to Kotlin (Android IME & Companion).

use lapacho_core::crypto::SecretKey;
use lapacho_core::ingest::process_text as core_process_text;
use lapacho_core::storage::{HistoryRepo, RetentionPolicy, SqliteRepo};
use lapacho_core::types::PersistLevel;
use std::sync::Arc;

uniffi::setup_scaffolding!();

/// Errors crossing into Kotlin. Kept coarse on purpose: the IME only needs to
/// tell "re-unlock the vault" from "storage is broken" from "gone".
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    /// The master key is malformed or doesn't decrypt the DB — prompt to unlock.
    #[error("invalid master key: {reason}")]
    InvalidKey { reason: String },
    /// SQLite/crypto failure — the vault is unusable as-is.
    #[error("storage failure: {reason}")]
    Storage { reason: String },
    /// The requested item is not in history (expired by TTL, deleted, or synced away).
    #[error("item not found: {id}")]
    NotFound { id: String },
    /// A plugin ran and refused: a bad regex, nothing matched. `reason` is
    /// meant for the user.
    #[error("{reason}")]
    Plugin { reason: String },
}

type Result<T> = std::result::Result<T, MobileError>;

impl From<String> for MobileError {
    fn from(reason: String) -> Self {
        MobileError::Storage { reason }
    }
}

#[derive(uniffi::Record)]
pub struct MobileItem {
    pub id: String,
    pub display_content: String,
    pub content_type: String,
    pub sensitivity: String,
    pub timestamp: u64,
}

impl From<lapacho_core::types::ClipboardItem> for MobileItem {
    fn from(it: lapacho_core::types::ClipboardItem) -> Self {
        MobileItem {
            id: it.id,
            display_content: it.display_content,
            content_type: it.content_type,
            sensitivity: format!("{:?}", it.sensitivity),
            timestamp: it.timestamp,
        }
    }
}

/// Sensitivity of a clipboard payload — "None", "Personal", "Credential" or
/// A plugin the app can run, and the values it asks for first.
#[derive(uniffi::Record)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub params: Vec<PluginParamInfo>,
}

/// One value a plugin asks for: a text field, or a checkbox when `flag`.
#[derive(uniffi::Record)]
pub struct PluginParamInfo {
    pub name: String,
    pub label: String,
    pub flag: bool,
}

/// The plugins built into lapacho-core — the same ones desktop lists first.
/// Only these: an Android app can't run binaries of its own (W^X since
/// Android 10), so desktop's external-command plugins have nothing to run.
#[uniffi::export]
pub fn builtin_plugins() -> Vec<PluginInfo> {
    use lapacho_core::types::ParamKind;
    lapacho_core::plugins::builtin_plugins()
        .into_iter()
        .map(|p| PluginInfo {
            id: p.id,
            name: p.name,
            params: p
                .params
                .into_iter()
                .map(|q| PluginParamInfo { name: q.name, label: q.label, flag: q.kind == ParamKind::Flag })
                .collect(),
        })
        .collect()
}

/// Runs a built-in plugin over `input`, in this process. A flag param is
/// `"1"` when ticked; a missing one counts as empty.
#[uniffi::export]
pub fn run_plugin(id: String, input: String, params: std::collections::HashMap<String, String>) -> Result<String> {
    lapacho_core::plugins::run_builtin_with(&id, &input, |name| params.get(name).cloned().unwrap_or_default())
        .map_err(|reason| MobileError::Plugin { reason })
}

/// "Secret" — decided by the same ingest pipeline desktop runs, so a password
/// copied from a note is recognized on both. Stateless: needs no key or DB,
/// which lets the Kotlin storage use it before the P1 migration lands.
#[uniffi::export]
pub fn classify_sensitivity(text: String) -> String {
    format!("{:?}", core_process_text(&text).sensitivity)
}

/// A new master key, base64. The phone keeps it wrapped by the Android
/// Keystore (the app's `MasterKey`) and hands it back to [`MobileCore::new`].
#[uniffi::export]
pub fn generate_master_key() -> Result<String> {
    Ok(SecretKey::generate()?.to_base64())
}

#[derive(uniffi::Object)]
pub struct MobileCore {
    repo: Arc<SqliteRepo>,
}

#[uniffi::export]
impl MobileCore {
    #[uniffi::constructor]
    pub fn new(db_path: String, master_key_base64: String) -> Result<Arc<Self>> {
        let key = SecretKey::from_base64(&master_key_base64)
            .map_err(|e| MobileError::InvalidKey { reason: e.to_string() })?;
        let repo = SqliteRepo::new(db_path, key)?;
        Ok(Arc::new(Self {
            repo: Arc::new(repo),
        }))
    }

    /// Process a new text payload from clipboard and save it. `timestamp`
    /// (Unix seconds) keeps an item's original time when importing; `None`
    /// is now.
    pub fn ingest_text(&self, raw: String, persist_level: String, timestamp: Option<u64>) -> Result<MobileItem> {
        let mut item = core_process_text(&raw);
        if let Some(ts) = timestamp {
            item.timestamp = ts;
        }
        // Keyed content hash, same as desktop's main.rs: dedups re-copies and
        // gives the same id on every device sharing the master key.
        item.id = self.repo.content_id(&item.raw_content);
        let level = match persist_level.as_str() {
            "Sensitive" => PersistLevel::Sensitive,
            "All" => PersistLevel::All,
            _ => PersistLevel::None,
        };

        self.repo.save(&item, level)?;
        Ok(item.into())
    }

    /// The newest `limit` items, for the IME strip and the companion list.
    /// Without their raw content: that is fetched by id when pasted.
    pub fn get_recent_items(&self, limit: u32) -> Result<Vec<MobileItem>> {
        Ok(self.repo.load()?.into_iter().take(limit as usize).map(MobileItem::from).collect())
    }

    /// The id an item with this content has (or would have): the keyed hash
    /// desktop uses too.
    pub fn content_id(&self, raw: String) -> String {
        self.repo.content_id(&raw)
    }

    /// Keeps the newest `max_items`. No TTL here: the phone never stores
    /// credentials or secrets in the first place.
    pub fn trim(&self, max_items: u32) -> Result<()> {
        let policy = RetentionPolicy { sensitive_ttl_secs: None, max_items: max_items as usize };
        Ok(self.repo.cleanup(&policy)?)
    }

    pub fn clear(&self) -> Result<()> {
        Ok(self.repo.clear()?)
    }

    /// A word the user taught the keyboard; twice is once.
    pub fn learn(&self, word: String) -> Result<()> {
        Ok(self.repo.learn(&word)?)
    }

    /// The learned words, oldest first.
    pub fn lexicon(&self) -> Result<Vec<String>> {
        Ok(self.repo.lexicon()?)
    }

    pub fn forget(&self, word: String) -> Result<()> {
        Ok(self.repo.forget(&word)?)
    }

    pub fn forget_all(&self) -> Result<()> {
        Ok(self.repo.forget_all()?)
    }

    pub fn get_preference(&self, key: String) -> Result<Option<String>> {
        Ok(self.repo.get_preference(&key)?)
    }

    pub fn set_preference(&self, key: String, value: String) -> Result<()> {
        Ok(self.repo.set_preference(&key, &value)?)
    }

    /// Get raw content for a specific item to paste it. Single-row lookup: the
    /// IME calls this on every paste, so it must not decrypt the whole history.
    pub fn get_raw_content(&self, id: String) -> Result<String> {
        self.repo
            .get_by_id(&id)?
            .map(|it| it.raw_content)
            .ok_or(MobileError::NotFound { id })
    }
}

/// The active dictionaries, held for the life of the keyboard process.
///
/// Kotlin reads them (the bundled asset plus any the user imported) and
/// hands over their text once; nothing here opens a
/// file or a socket, and suggesting never writes anything down — the same
/// dictionary gives the same suggestions to everyone who has it.
#[derive(uniffi::Object)]
pub struct WordPredictor {
    inner: lapacho_predict::Predictor,
}

#[uniffi::export]
impl WordPredictor {
    /// Each dictionary is one `word frequency` per line (see
    /// `docs/DICTIONARIES.md`); they are mixed, each normalized to its own
    /// corpus. `learned` are the words the user taught the keyboard.
    #[uniffi::constructor]
    pub fn new(dictionaries: Vec<String>, learned: Vec<String>) -> Arc<Self> {
        let dicts: Vec<&str> = dictionaries.iter().map(String::as_str).collect();
        let learned: Vec<&str> = learned.iter().map(String::as_str).collect();
        Arc::new(Self {
            inner: lapacho_predict::Predictor::new(&dicts, &learned),
        })
    }

    /// Words that continue `prefix`, most frequent first.
    pub fn suggest(&self, prefix: String, limit: u32) -> Vec<String> {
        self.inner.suggest(&prefix, limit as usize)
    }

    /// Words the user probably meant by a misspelled `word`, best first
    /// (one edit away, a swap of two letters counting as one; two only if
    /// nothing is one away). Never applied by itself: the keyboard offers
    /// them in the strip and the user taps one.
    pub fn correct(&self, word: String, limit: u32) -> Vec<String> {
        self.inner.correct(&word, limit as usize)
    }

    /// Words a swipe spelled, best first. `path` is the finger's line as
    /// `x0, y0, x1, y1, …`; `letters` the keys' letters, one char each, and
    /// `centres` their centres the same way; `key_width` the distance
    /// between two neighbouring keys, in the same units (pixels). The line
    /// is decoded and dropped: nothing about it is kept.
    pub fn swipe(&self, path: Vec<f32>, letters: String, centres: Vec<f32>, key_width: f32, limit: u32) -> Vec<String> {
        let path: Vec<(f32, f32)> = path.chunks_exact(2).map(|p| (p[0], p[1])).collect();
        let keys: Vec<lapacho_predict::SwipeKey> =
            letters.chars().zip(centres.chunks_exact(2)).map(|(c, p)| (c, p[0], p[1])).collect();
        self.inner.swipe(&path, &keys, key_width, limit as usize)
    }

    /// Whether the word is already known, accents aside. What the keyboard
    /// asks before offering to learn one.
    pub fn knows(&self, word: String) -> bool {
        self.inner.knows(&word)
    }

    /// Words across all dictionaries (learned ones included) — the IME logs
    /// it once to tell a truncated asset from a missing one.
    pub fn size(&self) -> u32 {
        self.inner.len() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lapacho_core::crypto;

    #[test]
    fn the_desktop_built_ins_run_here_too() {
        let replace = builtin_plugins().into_iter().find(|p| p.id == "replace").unwrap();
        assert!(replace.params.iter().any(|p| p.name == "regex" && p.flag));
        let params = std::collections::HashMap::from([
            ("search".to_string(), "(\\w+)@(\\w+)".to_string()),
            ("replace".to_string(), "$2 at $1".to_string()),
            ("regex".to_string(), "1".to_string()),
        ]);
        assert_eq!(run_plugin("replace".into(), "leo@quebracho".into(), params).unwrap(), "quebracho at leo");
        assert!(matches!(
            run_plugin("replace".into(), "hola".into(), Default::default()),
            Err(MobileError::Plugin { reason }) if reason == "Nothing to search for"
        ));
    }

    #[test]
    fn classify_sensitivity_flags_a_password_copied_from_a_note() {
        // The case that leaked in plain text on a phone: copied from a text
        // note, so no password manager flagged it as sensitive.
        assert_eq!(classify_sensitivity("E2DBNVWU5xFfcjV+++!".into()), "Secret");
        assert_eq!(classify_sensitivity("hola lapacho mobile".into()), "None");
    }

    #[test]
    fn test_mobile_bridge_roundtrip() {
        let db_dir = std::env::temp_dir().join(format!("mobile_test_{}.db", uuid::Uuid::new_v4()));
        let key_b64 = crypto::SecretKey::generate().unwrap().to_base64();
        
        let core = MobileCore::new(db_dir.to_str().unwrap().to_string(), key_b64).unwrap();
        let item = core.ingest_text("hola lapacho mobile".into(), "All".into(), None).unwrap();
        
        assert_eq!(item.display_content, "hola lapacho mobile");
        
        let recent = core.get_recent_items(100).unwrap();
        assert_eq!(recent.len(), 1);
        
        let raw = core.get_raw_content(item.id.clone()).unwrap();
        assert_eq!(raw, "hola lapacho mobile");

        assert!(matches!(
            core.get_raw_content("nope".into()),
            Err(MobileError::NotFound { .. })
        ));
    }

    #[test]
    fn test_recopy_dedups_with_keyed_content_id() {
        let db = std::env::temp_dir().join(format!("mobile_dedup_{}.db", uuid::Uuid::new_v4()));
        let core = MobileCore::new(
            db.to_str().unwrap().to_string(),
            crypto::SecretKey::generate().unwrap().to_base64(),
        )
        .unwrap();
        let a = core.ingest_text("mismo texto".into(), "All".into(), None).unwrap();
        let b = core.ingest_text("mismo texto".into(), "All".into(), None).unwrap();

        // Same id scheme as desktop (main.rs sets item.id = repo.content_id(raw)),
        // so re-copying moves to top instead of duplicating, and sync can dedup.
        assert_eq!(a.id, b.id);
        assert_eq!(a.id, core.repo.content_id("mismo texto"));
        assert_eq!(core.get_recent_items(100).unwrap().len(), 1);
    }

    /// What the Kotlin storage did, now here: the cap, the learned words,
    /// the preferences, an imported item keeping its time.
    #[test]
    fn everything_the_phone_stored_in_kotlin() {
        let db = std::env::temp_dir().join(format!("mobile_all_{}.db", uuid::Uuid::new_v4()));
        let core = MobileCore::new(db.to_str().unwrap().to_string(), generate_master_key().unwrap()).unwrap();
        core.ingest_text("viejo".into(), "All".into(), Some(1_000)).unwrap();
        for i in 0..5 {
            core.ingest_text(format!("clip {i}"), "All".into(), None).unwrap();
        }
        let recent = core.get_recent_items(3).unwrap();
        assert_eq!(recent.len(), 3);
        assert!(recent.iter().all(|it| it.timestamp > 1_000), "the imported one is the oldest");
        core.trim(4).unwrap();
        assert_eq!(core.get_recent_items(100).unwrap().len(), 4);
        assert!(core.get_recent_items(100).unwrap().iter().all(|it| it.display_content != "viejo"));

        core.learn("lapacho".into()).unwrap();
        core.learn("lapacho".into()).unwrap();
        assert_eq!(core.lexicon().unwrap(), vec!["lapacho"]);
        core.forget_all().unwrap();
        assert!(core.lexicon().unwrap().is_empty());

        assert_eq!(core.get_preference("favorites".into()).unwrap(), None);
        core.set_preference("favorites".into(), "🌳 🧉".into()).unwrap();
        assert_eq!(core.get_preference("favorites".into()).unwrap().as_deref(), Some("🌳 🧉"));

        core.clear().unwrap();
        assert!(core.get_recent_items(100).unwrap().is_empty());
    }

    #[test]
    fn test_bad_key_is_invalid_key_error() {
        let db = std::env::temp_dir().join(format!("mobile_badkey_{}.db", uuid::Uuid::new_v4()));
        assert!(matches!(
            MobileCore::new(db.to_str().unwrap().to_string(), "not-base64!!".into()),
            Err(MobileError::InvalidKey { .. })
        ));
    }
}
