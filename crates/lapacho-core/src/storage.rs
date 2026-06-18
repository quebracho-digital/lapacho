use crate::crypto;
use crate::types::{ClipboardItem, DetectedType, PersistLevel, Sensitivity};
use rusqlite::{Connection, params};
use std::path::Path;

const CREDENTIAL_TTL_SECS: u64 = 7200;
const HISTORY_LIMIT: usize = 100;

fn open_conn(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    // Prevents SQLITE_BUSY from concurrent writers (monitor thread + UI)
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
    Ok(conn)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn init_db(db_path: &Path) -> Result<(), String> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = open_conn(db_path)?;
    // WAL mode: allows monitor to write while UI reads/deletes
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

    // Migrations: ignore errors if columns already exist
    let _ = conn.execute("ALTER TABLE history ADD COLUMN thumbnail TEXT", []);
    let _ = conn.execute("ALTER TABLE history ADD COLUMN detected_type TEXT NOT NULL DEFAULT 'Text'", []);
    Ok(())
}

/// Persists an item, honoring the persistence policy. The text content
/// (`raw_content` and `display_content`) is encrypted at rest with `key`;
/// metadata (type, sensitivity, timestamp) stays in clear for querying.
pub fn save_item(
    db_path: &Path,
    item: &ClipboardItem,
    level: PersistLevel,
    key: &[u8; crypto::KEY_LEN],
) -> Result<(), String> {
    let should_save = match level {
        PersistLevel::None => item.sensitivity == Sensitivity::None,
        PersistLevel::Sensitive => item.sensitivity != Sensitivity::Secret,
        PersistLevel::All => true,
    };
    if !should_save {
        return Ok(());
    }

    // Encryption at rest: the clipboard content never hits the disk in clear.
    let enc_raw = crypto::encrypt(&item.raw_content, key)?;
    let enc_display = crypto::encrypt(&item.display_content, key)?;

    let conn = open_conn(db_path)?;
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

pub fn load_history(
    db_path: &Path,
    key: &[u8; crypto::KEY_LEN],
) -> Result<Vec<ClipboardItem>, String> {
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

    let conn = open_conn(db_path)?;
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
        // Rows that don't decrypt (wrong key, corruption, legacy plaintext) are
        // skipped rather than aborting the whole load.
        let raw_content = match crypto::decrypt(&r.enc_raw, key) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let display_content = match crypto::decrypt(&r.enc_display, key) {
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

pub fn run_cleanup(db_path: &Path, level: PersistLevel) -> Result<(), String> {
    let conn = open_conn(db_path)?;

    if level != PersistLevel::All {
        let cutoff = now_secs().saturating_sub(CREDENTIAL_TTL_SECS);
        let _ = conn.execute(
            "DELETE FROM history WHERE sensitivity = 'Credential' AND timestamp < ?1",
            params![cutoff],
        );
    }

    // Hard cap on history size
    let _ = conn.execute(
        "DELETE FROM history WHERE id NOT IN (
            SELECT id FROM history ORDER BY timestamp DESC LIMIT ?1
        )",
        params![HISTORY_LIMIT],
    );
    Ok(())
}

pub fn delete_item(db_path: &Path, id: &str) -> Result<(), String> {
    let conn = open_conn(db_path)?;
    conn.execute("DELETE FROM history WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn clear_all(db_path: &Path) -> Result<(), String> {
    let conn = open_conn(db_path)?;
    conn.execute("DELETE FROM history", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: [u8; 32] = [9u8; 32];

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
        let db = std::env::temp_dir().join(format!("lp_test_{}.db", uuid::Uuid::new_v4()));
        init_db(&db).unwrap();

        // PersistLevel::None only saves Sensitivity::None
        for (s, id) in [
            (Sensitivity::None, "none"),
            (Sensitivity::Personal, "personal"),
            (Sensitivity::Credential, "cred"),
            (Sensitivity::Secret, "secret"),
        ] {
            save_item(&db, &dummy(id, s, 1000), PersistLevel::None, &TEST_KEY).unwrap();
        }
        assert_eq!(load_history(&db, &TEST_KEY).unwrap().len(), 1);

        // PersistLevel::Sensitive saves everything except Secret
        save_item(&db, &dummy("p2", Sensitivity::Personal, 1001), PersistLevel::Sensitive, &TEST_KEY).unwrap();
        save_item(&db, &dummy("c2", Sensitivity::Credential, 1002), PersistLevel::Sensitive, &TEST_KEY).unwrap();
        save_item(&db, &dummy("s2", Sensitivity::Secret, 1003), PersistLevel::Sensitive, &TEST_KEY).unwrap();
        let h = load_history(&db, &TEST_KEY).unwrap();
        assert_eq!(h.len(), 3);
        assert!(!h.iter().any(|x| x.sensitivity == Sensitivity::Secret));

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn credential_ttl_cleanup() {
        let db = std::env::temp_dir().join(format!("lp_test_{}.db", uuid::Uuid::new_v4()));
        init_db(&db).unwrap();
        let now = now_secs();

        save_item(&db, &dummy("old", Sensitivity::Credential, now - 8000), PersistLevel::Sensitive, &TEST_KEY).unwrap();
        save_item(&db, &dummy("new", Sensitivity::Credential, now - 600), PersistLevel::Sensitive, &TEST_KEY).unwrap();
        save_item(&db, &dummy("txt", Sensitivity::None, now - 8000), PersistLevel::Sensitive, &TEST_KEY).unwrap();

        run_cleanup(&db, PersistLevel::Sensitive).unwrap();
        let h = load_history(&db, &TEST_KEY).unwrap();
        assert_eq!(h.len(), 2);
        assert!(!h.iter().any(|x| x.id == "old"));

        // PersistLevel::All skips TTL
        save_item(&db, &dummy("old2", Sensitivity::Credential, now - 8000), PersistLevel::All, &TEST_KEY).unwrap();
        run_cleanup(&db, PersistLevel::All).unwrap();
        assert!(load_history(&db, &TEST_KEY).unwrap().iter().any(|x| x.id == "old2"));

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn delete_item_works() {
        let db = std::env::temp_dir().join(format!("lp_test_{}.db", uuid::Uuid::new_v4()));
        init_db(&db).unwrap();
        save_item(&db, &dummy("x", Sensitivity::None, 1000), PersistLevel::All, &TEST_KEY).unwrap();
        assert_eq!(load_history(&db, &TEST_KEY).unwrap().len(), 1);
        delete_item(&db, "x").unwrap();
        assert!(load_history(&db, &TEST_KEY).unwrap().is_empty());
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn content_is_encrypted_at_rest() {
        let db = std::env::temp_dir().join(format!("lp_test_{}.db", uuid::Uuid::new_v4()));
        init_db(&db).unwrap();

        let mut item = dummy("m", Sensitivity::None, 1000);
        let marker = "PLAINTEXT_MARKER_a1b2c3";
        item.raw_content = marker.to_string();
        item.display_content = marker.to_string();
        save_item(&db, &item, PersistLevel::All, &TEST_KEY).unwrap();

        // The plaintext must not appear in any DB file on disk (.db / -wal / -shm).
        for suffix in ["", "-wal", "-shm"] {
            let path = db.with_extension(format!("db{suffix}"));
            if let Ok(bytes) = std::fs::read(&path) {
                let hay = String::from_utf8_lossy(&bytes);
                assert!(!hay.contains(marker), "plaintext leaked to {path:?}");
            }
        }

        // But a load with the right key recovers it.
        let h = load_history(&db, &TEST_KEY).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].raw_content, marker);

        // A load with the wrong key recovers nothing (rows skipped).
        let wrong = [1u8; 32];
        assert!(load_history(&db, &wrong).unwrap().is_empty());

        let _ = std::fs::remove_file(&db);
    }
}
