//! Lapacho desktop backend (Tauri 2).
//!
//! Owns the clipboard monitor thread and exposes the commands the UI calls.
//! Every clipboard payload is routed through `lapacho-core`
//! ([`process_text`]): classified, sanitized, and — for credentials and
//! secrets — masked before it can reach the frontend. `raw_content` never
//! leaves the backend except when the user explicitly copies an item back to
//! the system clipboard.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lapacho_core::types::{PersistLevel, UIClipboardItem};
use lapacho_core::{PluginDefinition, PluginResponse, plugins, process_text, storage};
use tauri::{AppHandle, Emitter, Manager, State};

/// How often the monitor polls the system clipboard.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Event emitted to the frontend when a new clipboard item is captured.
const EVENT_NEW_ITEM: &str = "clipboard://new";

/// Backend state shared between Tauri commands and the monitor thread.
struct AppState {
    db_path: PathBuf,
    plugins_dir: PathBuf,
    persist_level: Arc<Mutex<PersistLevel>>,
    /// Hash of the last clipboard value processed. Prevents re-ingesting our
    /// own writes (when the user copies an item back) and de-dupes repeats.
    last_seen: Arc<Mutex<Option<u64>>>,
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn persist_level_from_str(s: &str) -> PersistLevel {
    match s {
        "all" => PersistLevel::All,
        "sensitive" => PersistLevel::Sensitive,
        _ => PersistLevel::None,
    }
}

fn persist_level_to_str(level: PersistLevel) -> &'static str {
    match level {
        PersistLevel::All => "all",
        PersistLevel::Sensitive => "sensitive",
        PersistLevel::None => "none",
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Returns the persisted history as UI-safe items (never includes `raw_content`).
#[tauri::command]
fn get_history(state: State<'_, AppState>) -> Result<Vec<UIClipboardItem>, String> {
    let items = storage::load_history(&state.db_path)?;
    Ok(items.into_iter().map(UIClipboardItem::from).collect())
}

#[tauri::command]
fn delete_item(id: String, state: State<'_, AppState>) -> Result<(), String> {
    storage::delete_item(&state.db_path, &id)
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    storage::clear_all(&state.db_path)
}

#[tauri::command]
fn get_persist_level(state: State<'_, AppState>) -> String {
    persist_level_to_str(*state.persist_level.lock().unwrap()).to_string()
}

/// Updates the persistence policy and immediately reconciles the stored
/// history with the new policy (a stricter level prunes now-forbidden items).
#[tauri::command]
fn set_persist_level(level: String, state: State<'_, AppState>) -> Result<(), String> {
    let lvl = persist_level_from_str(&level);
    *state.persist_level.lock().unwrap() = lvl;
    storage::run_cleanup(&state.db_path, lvl)
}

/// Copies a stored item's original content back to the system clipboard.
/// The monitor is primed to ignore this value so it is not re-captured.
#[tauri::command]
fn copy_item(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let items = storage::load_history(&state.db_path)?;
    let item = items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| "Item not found in history".to_string())?;

    *state.last_seen.lock().unwrap() = Some(hash_str(&item.raw_content));

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(item.raw_content).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginDefinition>, String> {
    plugins::load_plugins(&state.plugins_dir)
}

#[tauri::command]
fn run_plugin(
    plugin_id: String,
    input: String,
    state: State<'_, AppState>,
) -> Result<PluginResponse, String> {
    plugins::execute_plugin(&state.plugins_dir, &plugin_id, &input)
}

// ---------------------------------------------------------------------------
// Clipboard monitor
// ---------------------------------------------------------------------------

fn run_monitor(
    app: AppHandle,
    db_path: PathBuf,
    persist_level: Arc<Mutex<PersistLevel>>,
    last_seen: Arc<Mutex<Option<u64>>>,
) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("lapacho: could not access the clipboard: {e}");
            return;
        }
    };

    loop {
        std::thread::sleep(POLL_INTERVAL);

        let text = match clipboard.get_text() {
            Ok(t) if !t.is_empty() => t,
            // Empty, non-text (e.g. an image), or transient read error: skip.
            _ => continue,
        };

        let hash = hash_str(&text);
        {
            let mut last = last_seen.lock().unwrap();
            if *last == Some(hash) {
                continue;
            }
            *last = Some(hash);
        }

        let item = process_text(&text);
        let level = *persist_level.lock().unwrap();
        if let Err(e) = storage::save_item(&db_path, &item, level) {
            eprintln!("lapacho: failed to save clipboard item: {e}");
        }
        let _ = storage::run_cleanup(&db_path, level);

        // Emit the UI-safe projection (masked for credentials/secrets).
        let _ = app.emit(EVENT_NEW_ITEM, UIClipboardItem::from(item));
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let db_path = data_dir.join("history.db");
            let plugins_dir = data_dir.join("plugins");
            storage::init_db(&db_path)?;
            let _ = plugins::init_plugins_dir(&plugins_dir);

            let persist_level = Arc::new(Mutex::new(PersistLevel::None));
            let last_seen = Arc::new(Mutex::new(None));

            app.manage(AppState {
                db_path: db_path.clone(),
                plugins_dir,
                persist_level: persist_level.clone(),
                last_seen: last_seen.clone(),
            });

            let handle = app.handle().clone();
            std::thread::spawn(move || run_monitor(handle, db_path, persist_level, last_seen));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_history,
            delete_item,
            clear_history,
            get_persist_level,
            set_persist_level,
            copy_item,
            list_plugins,
            run_plugin
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Lapacho desktop app");
}
