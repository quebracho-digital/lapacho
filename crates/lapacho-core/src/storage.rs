use crate::crypto;
use crate::types::{ClipboardItem, DetectedType, PersistLevel, Sensitivity};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

const CREDENTIAL_TTL_SECS: u64 = 7200;
const HISTORY_LIMIT: usize = 100;

/// Storage backend for the clipboard history.
///
/// The rest of the app depends on this behavior, not on a concrete database.
/// Implementors own *where* and *how* items live (path, encryption, retention);
/// callers only ever see decrypted [`ClipboardItem`]s. Swapping SQLite for
/// another backend (or an in-memory fake in tests) is a matter of providing a
/// different impl.
pub trait HistoryRepo: Send + Sync {
    /// Persists `item` if `level` allows it; a no-op (returns `Ok`) otherwise.
    fn save(&self, item: &ClipboardItem, level: PersistLevel) -> Result<(), String>;
    /// Returns the most recent history, newest first.
    fn load(&self) -> Result<Vec<ClipboardItem>, String>;
    /// Removes a single item by id.
    fn delete(&self, id: &str) -> Result<(), String>;
    /// Removes every item.
    fn clear(&self) -> Result<(), String>;
    /// Enforces the retention policy for `level` (credential TTL, size cap).
    fn cleanup(&self, level: PersistLevel) -> Result<(), String>;
}

/// SQLite-backed [`HistoryRepo`].
///
/// Content (`raw_content` + `display_content`) is encrypted at rest with `key`;
/// metadata (type, sensitivity, timestamp) stays in clear so it can be queried
/// and ordered. A fresh connection is opened per call — WAL mode plus a
/// `busy_timeout` make that safe when the monitor thread writes while the UI
/// reads or deletes.
pub struct SqliteRepo {
    db_path: PathBuf,
    key: [u8; crypto::KEY_LEN],
}

impl SqliteRepo {
    /// Opens (creating and migrating if needed) the history database at
    /// `db_path`, encrypting content with `key`.
    pub fn new(db_path: impl Into<PathBuf>, key: [u8; crypto::KEY_LEN]) -> Result<Self, String> {
        let repo = Self {
            db_path: db_path.into(),
            key,
        };
        repo.init()?;
        Ok(repo)
    }

    fn conn(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.db_path).map_err(|e| e.to_string())?;
        // Prevents SQLITE_BUSY from concurrent writers (monitor thread + UI).
        let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
        Ok(conn)
    }

    fn init(&self) -> Result<(), String> {
        if let Some(parent) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = self.conn()?;
        // WAL mode: lets the monitor write while the UI reads/deletes.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS history (
                id TEXT PRIMARY KEY,
                raw_content TEXT NOT NULL,
                display_content TEXT NOT NULL,
                content_type TEXT NOT NULL,
                sensitivity TEXT NOT NULL,
                detected_type TEXT NOT NULL DEFAULT 'Text',
                timestamp INTEGER NOT NULL,
                thumbnail TEXT
            )",
            [],
        )
        .map_err(|e| e.to_string())?;

        // Migrations: ignore errors if the columns already exist.
        let _ = conn.execute("ALTER TABLE history ADD COLUMN thumbnail TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE history ADD COLUMN detected_type TEXT NOT NULL DEFAULT 'Text'",
            [],
        );
        Ok(())
    }
}

impl HistoryRepo for SqliteRepo {
    fn save(&self, item: &ClipboardItem, level: PersistLevel) -> Result<(), String> {
        let should_save = match level {
            PersistLevel::None => item.sensitivity == Sensitivity::None,
            PersistLevel::Sensitive => item.sensitivity != Sensitivity::Secret,
            PersistLevel::All => true,
        };
        if !should_save {
            return Ok(());
        }

        // Encryption at rest: the clipboard content never hits the disk in clear.
        let enc_raw = crypto::encrypt(&item.raw_content, &self.key)?;
        let enc_display = crypto::encrypt(&item.display_content, &self.key)?;

        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO history
             (id, raw_content, display_content, content_type, sensitivity, detected_type, timestamp, thumbnail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                item.id,
                enc_raw,
                enc_display,
                item.content_type,
                format!("{:?}", item.sensitivity),
                format!("{:?}", item.detected_type),
                item.timestamp,
                item.thumbnail,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn load(&self) -> Result<Vec<ClipboardItem>, String> {
        // Encrypted row as read from SQLite, before decryption.
        struct EncRow {
            id: String,
            enc_raw: String,
            enc_display: String,
            content_type: String,
            sensitivity: String,
            detected_type: String,
            timestamp: u64,
            thumbnail: Option<String>,
        }

        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, raw_content, display_content, content_type, sensitivity, detected_type, timestamp, thumbnail
                 FROM history ORDER BY timestamp DESC LIMIT 100",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(EncRow {
                    id: row.get(0)?,
                    enc_raw: row.get(1)?,
                    enc_display: row.get(2)?,
                    content_type: row.get(3)?,
                    sensitivity: row.get(4)?,
                    detected_type: row.get(5)?,
                    timestamp: row.get(6)?,
                    thumbnail: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows.flatten() {
            // Rows that don't decrypt (wrong key, corruption, legacy plaintext)
            // are skipped rather than aborting the whole load.
            let raw_content = match crypto::decrypt(&r.enc_raw, &self.key) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let display_content = match crypto::decrypt(&r.enc_display, &self.key) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let sensitivity = match r.sensitivity.as_str() {
                "Personal" => Sensitivity::Personal,
                "Credential" => Sensitivity::Credential,
                "Secret" => Sensitivity::Secret,
                _ => Sensitivity::None,
            };
            let detected_type = match r.detected_type.as_str() {
                "Svg" => DetectedType::Svg,
                "Url" => DetectedType::Url,
                "Json" => DetectedType::Json,
                "Mermaid" => DetectedType::Mermaid,
                "Markdown" => DetectedType::Markdown,
                _ => DetectedType::Text,
            };

            items.push(ClipboardItem {
                id: r.id,
                raw_content,
                display_content,
                content_type: r.content_type,
                sensitivity,
                detected_type,
                timestamp: r.timestamp,
                thumbnail: r.thumbnail,
            });
        }
        Ok(items)
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM history", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn cleanup(&self, level: PersistLevel) -> Result<(), String> {
        let conn = self.conn()?;

        if level != PersistLevel::All {
            let cutoff = now_secs().saturating_sub(CREDENTIAL_TTL_SECS);
            let _ = conn.execute(
                "DELETE FROM history WHERE sensitivity = 'Credential' AND timestamp < ?1",
                params![cutoff],
            );
        }

        // Hard cap on history size.
        let _ = conn.execute(
            "DELETE FROM history WHERE id NOT IN (
                SELECT id FROM history ORDER BY timestamp DESC LIMIT ?1
            )",
            params![HISTORY_LIMIT],
        );
        Ok(())
    }
}

/// Convenience for callers (and tests) that just need a path + key to bring up a
/// repository. Equivalent to [`SqliteRepo::new`]; spelled out as a free function
/// so call sites read as "open the history at this path".
pub fn open(db_path: &Path, key: [u8; crypto::KEY_LEN]) -> Result<SqliteRepo, String> {
    SqliteRepo::new(db_path, key)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: [u8; 32] = [9u8; 32];

    fn repo() -> (SqliteRepo, PathBuf) {
        let db = std::env::temp_dir().join(format!("lp_test_{}.db", uuid::Uuid::new_v4()));
        let repo = SqliteRepo::new(&db, TEST_KEY).unwrap();
        (repo, db)
    }

    fn dummy(id: &str, sensitivity: Sensitivity, ts: u64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_string(),
            raw_content: "raw".into(),
            display_content: "display".into(),
            content_type: "text".into(),
            sensitivity,
            detected_type: DetectedType::Text,
            timestamp: ts,
            thumbnail: None,
        }
    }

    #[test]
    fn persistence_rules_by_level() {
        let (repo, db) = repo();

        // PersistLevel::None only saves Sensitivity::None.
        for (s, id) in [
            (Sensitivity::None, "none"),
            (Sensitivity::Personal, "personal"),
            (Sensitivity::Credential, "cred"),
            (Sensitivity::Secret, "secret"),
        ] {
            repo.save(&dummy(id, s, 1000), PersistLevel::None).unwrap();
        }
        assert_eq!(repo.load().unwrap().len(), 1);

        // PersistLevel::Sensitive saves everything except Secret.
        repo.save(&dummy("p2", Sensitivity::Personal, 1001), PersistLevel::Sensitive)
            .unwrap();
        repo.save(&dummy("c2", Sensitivity::Credential, 1002), PersistLevel::Sensitive)
            .unwrap();
        repo.save(&dummy("s2", Sensitivity::Secret, 1003), PersistLevel::Sensitive)
            .unwrap();
        let h = repo.load().unwrap();
        assert_eq!(h.len(), 3);
        assert!(!h.iter().any(|x| x.sensitivity == Sensitivity::Secret));

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn credential_ttl_cleanup() {
        let (repo, db) = repo();
        let now = now_secs();

        repo.save(&dummy("old", Sensitivity::Credential, now - 8000), PersistLevel::Sensitive)
            .unwrap();
        repo.save(&dummy("new", Sensitivity::Credential, now - 600), PersistLevel::Sensitive)
            .unwrap();
        repo.save(&dummy("txt", Sensitivity::None, now - 8000), PersistLevel::Sensitive)
            .unwrap();

        repo.cleanup(PersistLevel::Sensitive).unwrap();
        let h = repo.load().unwrap();
        assert_eq!(h.len(), 2);
        assert!(!h.iter().any(|x| x.id == "old"));

        // PersistLevel::All skips the TTL.
        repo.save(&dummy("old2", Sensitivity::Credential, now - 8000), PersistLevel::All)
            .unwrap();
        repo.cleanup(PersistLevel::All).unwrap();
        assert!(repo.load().unwrap().iter().any(|x| x.id == "old2"));

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn delete_and_clear_work() {
        let (repo, db) = repo();
        repo.save(&dummy("x", Sensitivity::None, 1000), PersistLevel::All)
            .unwrap();
        repo.save(&dummy("y", Sensitivity::None, 1001), PersistLevel::All)
            .unwrap();
        assert_eq!(repo.load().unwrap().len(), 2);

        repo.delete("x").unwrap();
        let h = repo.load().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].id, "y");

        repo.clear().unwrap();
        assert!(repo.load().unwrap().is_empty());

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn content_is_encrypted_at_rest() {
        let (repo, db) = repo();

        let mut item = dummy("m", Sensitivity::None, 1000);
        let marker = "PLAINTEXT_MARKER_a1b2c3";
        item.raw_content = marker.to_string();
        item.display_content = marker.to_string();
        repo.save(&item, PersistLevel::All).unwrap();

        // The plaintext must not appear in any DB file on disk (.db / -wal / -shm).
        for suffix in ["", "-wal", "-shm"] {
            let path = db.with_extension(format!("db{suffix}"));
            if let Ok(bytes) = std::fs::read(&path) {
                let hay = String::from_utf8_lossy(&bytes);
                assert!(!hay.contains(marker), "plaintext leaked to {path:?}");
            }
        }

        // A load with the right key recovers it.
        let h = repo.load().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].raw_content, marker);

        // A repo with the wrong key over the same file recovers nothing.
        let wrong = SqliteRepo::new(&db, [1u8; 32]).unwrap();
        assert!(wrong.load().unwrap().is_empty());

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn repo_behind_trait_object() {
        // The whole point of the abstraction: callers can hold `dyn HistoryRepo`.
        let (repo, db) = repo();
        let repo: Box<dyn HistoryRepo> = Box::new(repo);
        repo.save(&dummy("z", Sensitivity::None, 1000), PersistLevel::All)
            .unwrap();
        assert_eq!(repo.load().unwrap().len(), 1);
        let _ = std::fs::remove_file(&db);
    }
}
