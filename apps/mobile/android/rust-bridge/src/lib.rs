//! Mobile UniFFI bridge exposing lapacho-core to Kotlin (Android IME & Companion).

use lapacho_core::crypto::SecretKey;
use lapacho_core::ingest::process_text as core_process_text;
use lapacho_core::storage::{HistoryRepo, SqliteRepo};
use lapacho_core::types::PersistLevel;
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(uniffi::Record)]
pub struct MobileItem {
    pub id: String,
    pub display_content: String,
    pub content_type: String,
    pub sensitivity: String,
    pub timestamp: u64,
}

#[derive(uniffi::Object)]
pub struct MobileCore {
    repo: Arc<SqliteRepo>,
}

#[uniffi::export]
impl MobileCore {
    #[uniffi::constructor]
    pub fn new(db_path: String, master_key_base64: String) -> Result<Arc<Self>, String> {
        let key = SecretKey::from_base64(&master_key_base64)
            .map_err(|e| format!("Invalid base64 key: {e}"))?;
        let repo = SqliteRepo::new(db_path, key)?;
        Ok(Arc::new(Self {
            repo: Arc::new(repo),
        }))
    }

    /// Process a new text payload from clipboard and save it.
    pub fn ingest_text(&self, raw: String, persist_level: String) -> Result<MobileItem, String> {
        let item = core_process_text(&raw);
        let level = match persist_level.as_str() {
            "Sensitive" => PersistLevel::Sensitive,
            "All" => PersistLevel::All,
            _ => PersistLevel::None,
        };

        self.repo.save(&item, level)?;

        Ok(MobileItem {
            id: item.id,
            display_content: item.display_content,
            content_type: item.content_type,
            sensitivity: format!("{:?}", item.sensitivity),
            timestamp: item.timestamp,
        })
    }

    /// Load recent history for the IME strip / companion list.
    pub fn get_recent_items(&self) -> Result<Vec<MobileItem>, String> {
        let items = self.repo.load()?;
        Ok(items
            .into_iter()
            .map(|it| MobileItem {
                id: it.id,
                display_content: it.display_content,
                content_type: it.content_type,
                sensitivity: format!("{:?}", it.sensitivity),
                timestamp: it.timestamp,
            })
            .collect())
    }

    /// Get raw content for a specific item to paste it.
    pub fn get_raw_content(&self, id: String) -> Result<Option<String>, String> {
        let items = self.repo.load()?;
        Ok(items.into_iter().find(|it| it.id == id).map(|it| it.raw_content))
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
        assert_eq!(raw, Some("hola lapacho mobile".to_string()));
    }
}
