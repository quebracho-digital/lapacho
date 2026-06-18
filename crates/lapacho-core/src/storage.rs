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

pub fn save_item(db_path: &Path, item: &ClipboardItem, level: PersistLevel) -> Result<(), String> {
    let should_save = match level {
        PersistLevel::None => item.sensitivity == Sensitivity::None,
        PersistLevel::Sensitive => item.sensitivity != Sensitivity::Secret,
        PersistLevel::All => true,
    };
    if !should_save {
        return Ok(());
    }

    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT OR REPLACE INTO history
         (id, raw_content, display_content, content_type, sensitivity, detected_type, timestamp, thumbnail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            item.id,
            item.raw_content,
            item.display_content,
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

pub fn load_history(db_path: &Path) -> Result<Vec<ClipboardItem>, String> {
    let conn = open_conn(db_path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, raw_content, display_content, content_type, sensitivity, detected_type, timestamp, thumbnail
             FROM history ORDER BY timestamp DESC LIMIT 100",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let sens_str: String = row.get(4)?;
            let sensitivity = match sens_str.as_str() {
                "Personal" => Sensitivity::Personal,
                "Credential" => Sensitivity::Credential,
                "Secret" => Sensitivity::Secret,
                _ => Sensitivity::None,
            };
            let det_str: String = row.get(5)?;
            let detected_type = match det_str.as_str() {
                "Svg" => DetectedType::Svg,
                "Url" => DetectedType::Url,
                "Json" => DetectedType::Json,
                "Mermaid" => DetectedType::Mermaid,
                "Markdown" => DetectedType::Markdown,
                _ => DetectedType::Text,
            };
            Ok(ClipboardItem {
                id: row.get(0)?,
                raw_content: row.get(1)?,
                display_content: row.get(2)?,
                content_type: row.get(3)?,
                sensitivity,
                detected_type,
                timestamp: row.get(6)?,
                thumbnail: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;

    Ok(rows.flatten().collect())
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
            save_item(&db, &dummy(id, s, 1000), PersistLevel::None).unwrap();
        }
        assert_eq!(load_history(&db).unwrap().len(), 1);

        // PersistLevel::Sensitive saves everything except Secret
        save_item(&db, &dummy("p2", Sensitivity::Personal, 1001), PersistLevel::Sensitive).unwrap();
        save_item(&db, &dummy("c2", Sensitivity::Credential, 1002), PersistLevel::Sensitive).unwrap();
        save_item(&db, &dummy("s2", Sensitivity::Secret, 1003), PersistLevel::Sensitive).unwrap();
        let h = load_history(&db).unwrap();
        assert_eq!(h.len(), 3);
        assert!(!h.iter().any(|x| x.sensitivity == Sensitivity::Secret));

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn credential_ttl_cleanup() {
        let db = std::env::temp_dir().join(format!("lp_test_{}.db", uuid::Uuid::new_v4()));
        init_db(&db).unwrap();
        let now = now_secs();

        save_item(&db, &dummy("old", Sensitivity::Credential, now - 8000), PersistLevel::Sensitive).unwrap();
        save_item(&db, &dummy("new", Sensitivity::Credential, now - 600), PersistLevel::Sensitive).unwrap();
        save_item(&db, &dummy("txt", Sensitivity::None, now - 8000), PersistLevel::Sensitive).unwrap();

        run_cleanup(&db, PersistLevel::Sensitive).unwrap();
        let h = load_history(&db).unwrap();
        assert_eq!(h.len(), 2);
        assert!(!h.iter().any(|x| x.id == "old"));

        // PersistLevel::All skips TTL
        save_item(&db, &dummy("old2", Sensitivity::Credential, now - 8000), PersistLevel::All).unwrap();
        run_cleanup(&db, PersistLevel::All).unwrap();
        assert!(load_history(&db).unwrap().iter().any(|x| x.id == "old2"));

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn delete_item_works() {
        let db = std::env::temp_dir().join(format!("lp_test_{}.db", uuid::Uuid::new_v4()));
        init_db(&db).unwrap();
        save_item(&db, &dummy("x", Sensitivity::None, 1000), PersistLevel::All).unwrap();
        assert_eq!(load_history(&db).unwrap().len(), 1);
        delete_item(&db, "x").unwrap();
        assert!(load_history(&db).unwrap().is_empty());
        let _ = std::fs::remove_file(&db);
    }
}
