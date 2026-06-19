//! Integration: a plugin's raw output flows into the history as a raw item
//! whose display is sanitized.
//!
//! This locks the contract `run_plugin` relies on — "plugins operate on raw,
//! the response is stored raw and masked for render" — by composing the same
//! public pieces the desktop command does (`execute_plugin` → `process_text` →
//! `repo.save` → `load`), minus the Tauri event.
#![cfg(unix)]

use lapacho_core::crypto::SecretKey;
use lapacho_core::storage::{HistoryRepo, SqliteRepo};
use lapacho_core::types::{PersistLevel, Sensitivity};
use lapacho_core::{plugins, process_text};

#[test]
fn plugin_output_is_stored_raw_and_masked_for_display() {
    let base = std::env::temp_dir().join(format!("lp_it_{}", std::process::id()));
    let plugins_dir = base.join("plugins");
    let db = base.join("history.db");
    let _ = std::fs::remove_dir_all(&base);

    plugins::init_plugins_dir(&plugins_dir).unwrap();

    // 36 lowercase letters: the seeded `uppercase` plugin (`tr a-z A-Z`) keeps
    // this a valid GitHub-style credential after transforming it.
    let token = "ghp_abcdefghijklmnopqrstuvwxyzabcdefghij";
    let resp = plugins::execute_plugin(&plugins_dir, "uppercase", token).unwrap();
    assert!(resp.success);

    // Route the plugin's raw output through the same ingest pipeline as the
    // monitor, then persist it.
    let item = process_text(&resp.result_raw_content);
    assert_eq!(item.sensitivity, Sensitivity::Credential);

    let repo = SqliteRepo::new(&db, SecretKey::from_bytes([7u8; 32])).unwrap();
    repo.save(&item, PersistLevel::All).unwrap();

    let loaded = repo.load().unwrap();
    assert_eq!(loaded.len(), 1);
    let stored = &loaded[0];

    // Stored raw and intact (the uppercased token), recovered through the cipher.
    assert_eq!(stored.raw_content, resp.result_raw_content);
    assert_eq!(stored.raw_content, token.to_uppercase());
    // The render projection never carries the credential.
    assert_ne!(stored.display_content, stored.raw_content);
    assert!(!stored.display_content.contains(&token.to_uppercase()));

    let _ = std::fs::remove_dir_all(&base);
}
