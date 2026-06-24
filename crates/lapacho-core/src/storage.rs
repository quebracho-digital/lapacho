use crate::crypto;
use crate::types::{ClipboardItem, DetectedType, PersistLevel, Sensitivity};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

/// Default time-to-live for sensitive items: 2 hours.
const DEFAULT_SENSITIVE_TTL_SECS: u64 = 7200;
/// Default hard cap on the number of items kept.
const DEFAULT_MAX_ITEMS: usize = 100;

/// Time- and size-based retention for the history.
///
/// Sensitive items (credentials and secrets) are purged after
/// `sensitive_ttl_secs` **regardless of the persistence level** — a security
/// requirement, not a user preference. The value is meant to be driven by the
/// Quebracho admin (global / per-user / per-group / per-role); it reaches the
/// client already resolved to a single number, so this open core stays free of
/// the multi-tenant policy engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Seconds a sensitive item is kept before deletion. `None` disables
    /// time-based expiry entirely.
    pub sensitive_ttl_secs: Option<u64>,
    /// Hard cap on the number of items kept (most recent wins).
    pub max_items: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            sensitive_ttl_secs: Some(DEFAULT_SENSITIVE_TTL_SECS),
            max_items: DEFAULT_MAX_ITEMS,
        }
    }
}

/// Storage backend for the clipboard history.
///
/// The rest of the app depends on this behavior, not on a concrete database.
/// Implementors own *where* and *how* items live (path, encryption, retention);
/// callers only ever see decrypted [`ClipboardItem`]s. Swapping SQLite for
/// another backend (or an in-memory fake in tests) is a matter of providing a
/// different impl.
pub trait HistoryRepo: Send + Sync {
    /// Persists `item` if `level` allows it; a no-op (returns `Ok`) otherwise.
    ///
    /// Identity is the **content**: if an item with the same `raw_content`
    /// already exists, it is moved to the top (its timestamp refreshed) instead
    /// of inserting a duplicate — re-copying an old clip resurfaces it, the way
    /// Diodon de-dupes by content hash.
    fn save(&self, item: &ClipboardItem, level: PersistLevel) -> Result<(), String>;
    /// Returns the most recent history, newest first.
    fn load(&self) -> Result<Vec<ClipboardItem>, String>;

    /// Searches history for items whose raw or display content contains the
    /// query (case-insensitive). Empty/blank query behaves like `load()`.
    /// Returns newest first among matches. Used for UI history search.
    fn search(&self, query: &str) -> Result<Vec<ClipboardItem>, String>;

    /// Removes a single item by id.
    fn delete(&self, id: &str) -> Result<(), String>;
    /// Removes every item.
    fn clear(&self) -> Result<(), String>;
    /// Enforces `policy`: expires sensitive items past their TTL and caps the
    /// total number of items kept.
    fn cleanup(&self, policy: &RetentionPolicy) -> Result<(), String>;

    /// Persist a user preference (e.g. persist_level, sensitive_ttl_secs).
    /// Used so UI choices survive app restarts.
    fn set_preference(&self, key: &str, value: &str) -> Result<(), String>;

    /// Retrieve a previously saved preference.
    fn get_preference(&self, key: &str) -> Result<Option<String>, String>;

    /// Id estable derivado del contenido (ver crypto::content_id). Mismo contenido
    /// → mismo id, para deduplicar en disco y en los buffers vivos.
    fn content_id(&self, raw: &str) -> String;
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
    cipher: crypto::Cipher,
    content_key: zeroize::Zeroizing<[u8; 32]>,
}

impl SqliteRepo {
    /// Opens (creating and migrating if needed) the history database at
    /// `db_path`, encrypting content with `key`. The key is consumed to build a
    /// resident [`Cipher`] and then dropped (zeroized).
    pub fn new(db_path: impl Into<PathBuf>, key: crypto::SecretKey) -> Result<Self, String> {
        let content_key = zeroize::Zeroizing::new(crypto::derive_content_key(key.expose()));
        let repo = Self { db_path: db_path.into(), cipher: crypto::Cipher::new(&key), content_key };
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
        let _ = conn.execute("ALTER TABLE history ADD COLUMN size INTEGER", []);

        // Simple key-value settings for user preferences (persist_level, ttl, etc.)
        // so they survive restarts.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| e.to_string())?;

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

        let conn = self.conn()?;

        let enc_raw = self.cipher.encrypt(&item.raw_content)?;
        let enc_display = self.cipher.encrypt(&item.display_content)?;

        conn.execute(
            "INSERT INTO history
               (id, raw_content, display_content, content_type, sensitivity, detected_type, timestamp, thumbnail, size)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET timestamp = excluded.timestamp",
            params![ item.id, enc_raw, enc_display, item.content_type,
                format!("{:?}", item.sensitivity), format!("{:?}", item.detected_type),
                item.timestamp, item.thumbnail, item.size.map(|s| s as i64) ],
        ).map_err(|e| e.to_string())?;
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
            size: Option<i64>,
        }

        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, raw_content, display_content, content_type, sensitivity, detected_type, timestamp, thumbnail, size
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
                    size: row.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows.flatten() {
            // Rows that don't decrypt (wrong key, corruption, legacy plaintext)
            // are skipped rather than aborting the whole load.
            let raw_content = match self.cipher.decrypt(&r.enc_raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let display_content = match self.cipher.decrypt(&r.enc_display) {
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
                size: r.size.map(|s| s as usize),
            });
        }
        Ok(items)
    }

    fn search(&self, query: &str) -> Result<Vec<ClipboardItem>, String> {
        let q = query.trim();
        if q.is_empty() {
            return self.load();
        }
        let lower = q.to_lowercase();
        let all = self.load()?;
        Ok(all
            .into_iter()
            .filter(|it| {
                it.raw_content.to_lowercase().contains(&lower)
                    || it.display_content.to_lowercase().contains(&lower)
            })
            .collect())
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

    fn cleanup(&self, policy: &RetentionPolicy) -> Result<(), String> {
        let conn = self.conn()?;

        if let Some(ttl) = policy.sensitive_ttl_secs {
            let cutoff = now_secs().saturating_sub(ttl);
            // Sensitive = credentials + secrets; they expire regardless of the
            // persistence level.
            let _ = conn.execute(
                "DELETE FROM history WHERE sensitivity IN ('Credential', 'Secret') AND timestamp < ?1",
                params![cutoff],
            );
        }

        // Hard cap on history size.
        let _ = conn.execute(
            "DELETE FROM history WHERE id NOT IN (
                SELECT id FROM history ORDER BY timestamp DESC LIMIT ?1
            )",
            params![policy.max_items],
        );
        Ok(())
    }

    fn set_preference(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_preference(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![key]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let v: String = row.get(0).map_err(|e| e.to_string())?;
            Ok(Some(v))
        } else {
            Ok(None)
        }
    }

    fn content_id(&self, raw: &str) -> String {
        crypto::content_id(&*self.content_key, raw)
    }
}

/// Convenience for callers (and tests) that just need a path + key to bring up a
/// repository. Equivalent to [`SqliteRepo::new`]; spelled out as a free function
/// so call sites read as "open the history at this path".
pub fn open(db_path: &Path, key: crypto::SecretKey) -> Result<SqliteRepo, String> {
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
        let repo = SqliteRepo::new(&db, crypto::SecretKey::from_bytes(TEST_KEY)).unwrap();
        (repo, db)
    }

    fn dummy(id: &str, sensitivity: Sensitivity, ts: u64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_string(),
            // Distinct content per id: identity is the content, so reusing one
            // string across ids would (correctly) de-dupe them into one row.
            raw_content: format!("raw-{id}"),
            display_content: "display".into(),
            content_type: "text".into(),
            sensitivity,
            detected_type: DetectedType::Text,
            timestamp: ts,
            thumbnail: None,
            size: None,
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
    fn sensitive_ttl_cleanup() {
        let (repo, db) = repo();
        let now = now_secs();

        // Saved under All so credentials *and* secrets land on disk.
        repo.save(&dummy("old_cred", Sensitivity::Credential, now - 8000), PersistLevel::All)
            .unwrap();
        repo.save(&dummy("old_secret", Sensitivity::Secret, now - 8000), PersistLevel::All)
            .unwrap();
        repo.save(&dummy("new_cred", Sensitivity::Credential, now - 600), PersistLevel::All)
            .unwrap();
        repo.save(&dummy("txt", Sensitivity::None, now - 8000), PersistLevel::All)
            .unwrap();

        repo.cleanup(&RetentionPolicy::default()).unwrap();
        let ids: Vec<String> = repo.load().unwrap().into_iter().map(|x| x.id).collect();
        // Sensitive + stale → gone, even under PersistLevel::All.
        assert!(!ids.contains(&"old_cred".to_string()));
        assert!(!ids.contains(&"old_secret".to_string()));
        // Fresh sensitive and non-sensitive (any age) → kept.
        assert!(ids.contains(&"new_cred".to_string()));
        assert!(ids.contains(&"txt".to_string()));

        // A `None` TTL disables time-based expiry entirely.
        repo.save(&dummy("old_cred2", Sensitivity::Credential, now - 8000), PersistLevel::All)
            .unwrap();
        let no_ttl = RetentionPolicy {
            sensitive_ttl_secs: None,
            ..RetentionPolicy::default()
        };
        repo.cleanup(&no_ttl).unwrap();
        assert!(repo.load().unwrap().iter().any(|x| x.id == "old_cred2"));

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
    fn recopy_moves_to_top_instead_of_duplicating() {
        let (repo, db) = repo();

        // Two distinct clips (dummy gives "raw-a" / "raw-b").
        repo.save(&dummy("a", Sensitivity::None, 1000), PersistLevel::All)
            .unwrap();
        repo.save(&dummy("b", Sensitivity::None, 1001), PersistLevel::All)
            .unwrap();
        assert_eq!(repo.load().unwrap().len(), 2);

        // Mismo contenido ⇒ mismo id ("a"); timestamp nuevo.
        let again = dummy("a", Sensitivity::None, 2000);
        repo.save(&again, PersistLevel::All).unwrap();

        let h = repo.load().unwrap();
        // No duplicate row was created.
        assert_eq!(h.len(), 2);
        // The re-copied content moved to the top, keeping the original id and
        // taking the refreshed timestamp.
        assert_eq!(h[0].raw_content, "raw-a");
        assert_eq!(h[0].id, "a");
        assert_eq!(h[0].timestamp, 2000);
        // The other item is untouched.
        assert!(h.iter().any(|x| x.id == "b" && x.raw_content == "raw-b"));

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
        let wrong = SqliteRepo::new(&db, crypto::SecretKey::from_bytes([1u8; 32])).unwrap();
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

    #[test]
    fn search_matches_raw_and_display_case_insensitive() {
        let (repo, db) = repo();

        // Normal item searchable by visible display
        let mut md = dummy("md1", Sensitivity::None, 1000);
        md.display_content = "The secret code is LAPACHO-42 for project".into();
        md.raw_content = md.display_content.clone();
        md.detected_type = DetectedType::Markdown;
        repo.save(&md, PersistLevel::All).unwrap();

        // Credential: display masked, but searchable by raw content
        let mut cred = dummy("cred1", Sensitivity::Credential, 1001);
        cred.raw_content = "ghp_supersecretapikeyXYZ123".into();
        cred.display_content = "•••••••• [credential]".into();
        cred.sensitivity = Sensitivity::Credential;
        repo.save(&cred, PersistLevel::All).unwrap();

        // Blank query acts as load
        assert_eq!(repo.search("").unwrap().len(), 2);
        assert_eq!(repo.search("   ").unwrap().len(), 2);

        // Match on display (visible text)
        let res = repo.search("LAPACHO").unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "md1");

        // Case insensitive
        let res2 = repo.search("lapacho-42").unwrap();
        assert_eq!(res2.len(), 1);

        // Match on raw even for credential (masked in display)
        let res3 = repo.search("supersecretapikey").unwrap();
        assert_eq!(res3.len(), 1);
        assert_eq!(res3[0].id, "cred1");

        // No match
        assert!(repo.search("no-such-thing").unwrap().is_empty());

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn content_id_stable_between_calls_and_via_trait_object() {
        let (repo, db) = repo();
        let id1 = repo.content_id("x");
        let id2 = repo.content_id("x");
        assert_eq!(id1, id2);
        let repo: Box<dyn HistoryRepo> = Box::new(repo);
        assert_eq!(repo.content_id("x"), id1);
        let _ = std::fs::remove_file(&db);
    }
}
