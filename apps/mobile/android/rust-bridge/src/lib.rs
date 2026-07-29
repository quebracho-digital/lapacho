//! Mobile UniFFI bridge exposing lapacho-core to Kotlin (Android IME & Companion).

use lapacho_core::crypto::SecretKey;
use lapacho_core::ingest::process_text as core_process_text;
use lapacho_core::storage::{HistoryRepo, SqliteRepo};
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

    /// Process a new text payload from clipboard and save it.
    pub fn ingest_text(&self, raw: String, persist_level: String) -> Result<MobileItem> {
        let item = core_process_text(&raw);
        let level = match persist_level.as_str() {
            "Sensitive" => PersistLevel::Sensitive,
            "All" => PersistLevel::All,
            _ => PersistLevel::None,
        };

        self.repo.save(&item, level)?;
        Ok(item.into())
    }

    /// Load recent history for the IME strip / companion list.
    pub fn get_recent_items(&self) -> Result<Vec<MobileItem>> {
        Ok(self.repo.load()?.into_iter().map(MobileItem::from).collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use lapacho_core::crypto;

    #[test]
    fn test_mobile_bridge_roundtrip() {
        let db_dir = std::env::temp_dir().join(format!("mobile_test_{}.db", uuid::Uuid::new_v4()));
        let key_b64 = crypto::SecretKey::generate().unwrap().to_base64();
        
        let core = MobileCore::new(db_dir.to_str().unwrap().to_string(), key_b64).unwrap();
        let item = core.ingest_text("hola lapacho mobile".into(), "All".into()).unwrap();
        
        assert_eq!(item.display_content, "hola lapacho mobile");
        
        let recent = core.get_recent_items().unwrap();
        assert_eq!(recent.len(), 1);
        
        let raw = core.get_raw_content(item.id.clone()).unwrap();
        assert_eq!(raw, "hola lapacho mobile");

        assert!(matches!(
            core.get_raw_content("nope".into()),
            Err(MobileError::NotFound { .. })
        ));
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
