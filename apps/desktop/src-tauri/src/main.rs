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

mod keystore;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lapacho_core::storage::{HistoryRepo, RetentionPolicy, SqliteRepo};
use lapacho_core::types::{PersistLevel, UIClipboardItem};
use lapacho_core::{PluginDefinition, PluginResponse, disclose, plugins, process_text};
use tauri::{AppHandle, Emitter, Manager, State};

/// How often the monitor polls the system clipboard.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Event emitted to the frontend when a new clipboard item is captured.
/// Kept to a plain `a-z-` name to avoid any event-name validation surprises.
const EVENT_NEW_ITEM: &str = "clipboard-new";

/// Backend state shared between Tauri commands and the monitor thread.
struct AppState {
    /// History storage behind the [`HistoryRepo`] abstraction. The concrete
    /// backend (and the encryption key) is owned by the repo, not by the state.
    repo: Arc<dyn HistoryRepo>,
    plugins_dir: PathBuf,
    persist_level: Arc<Mutex<PersistLevel>>,
    /// Retention policy (sensitive TTL + size cap). Driven by the admin; mutable
    /// at runtime so a config/sync update takes effect without a restart.
    retention: Arc<Mutex<RetentionPolicy>>,
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
    let items = state.repo.load()?;
    Ok(items.into_iter().map(UIClipboardItem::from).collect())
}

#[tauri::command]
fn delete_item(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.repo.delete(&id)
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.repo.clear()
}

#[tauri::command]
fn get_persist_level(state: State<'_, AppState>) -> String {
    persist_level_to_str(*state.persist_level.lock().unwrap()).to_string()
}

/// Updates the persistence policy and runs a cleanup pass with the current
/// retention policy.
#[tauri::command]
fn set_persist_level(level: String, state: State<'_, AppState>) -> Result<(), String> {
    let lvl = persist_level_from_str(&level);
    *state.persist_level.lock().unwrap() = lvl;
    let policy = *state.retention.lock().unwrap();
    state.repo.cleanup(&policy)
}

/// Returns the current TTL (in seconds) after which sensitive items are purged,
/// or `null` if time-based expiry is disabled.
#[tauri::command]
fn get_sensitive_ttl(state: State<'_, AppState>) -> Option<u64> {
    state.retention.lock().unwrap().sensitive_ttl_secs
}

/// Sets the sensitive-item TTL (seconds; `null` to disable) and immediately
/// applies it. This is the seam the Quebracho admin/sync layer drives once the
/// per-user/group policy has been resolved to a single value.
#[tauri::command]
fn set_sensitive_ttl(secs: Option<u64>, state: State<'_, AppState>) -> Result<(), String> {
    let policy = {
        let mut guard = state.retention.lock().unwrap();
        guard.sensitive_ttl_secs = secs;
        *guard
    };
    state.repo.cleanup(&policy)
}

/// Loads a single history item by id and resolves the content the active
/// persistence level allows to leave the backend (the "doble vía"). This is the
/// shared seam behind every outbound channel — `copy_item`, `export_item`, and
/// eventually plugins — so disclosure is decided in exactly one place.
fn disclosed_content(id: &str, state: &AppState) -> Result<String, String> {
    let level = *state.persist_level.lock().unwrap();
    let items = state.repo.load()?;
    let item = items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| "Item not found in history".to_string())?;
    Ok(disclose(&item, level).to_string())
}

/// Copies a stored item back to the system clipboard, disclosing raw or
/// sanitized content per the active persistence level. The monitor is primed
/// with exactly what we write so it is not re-captured as a new item.
#[tauri::command]
fn copy_item(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let content = disclosed_content(&id, &state)?;
    *state.last_seen.lock().unwrap() = Some(hash_str(&content));

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(content).map_err(|e| e.to_string())
}

/// Returns a stored item's content for decoupling (save to a file, share, etc.).
/// Governed by the same disclosure policy as `copy_item`: in a paranoid
/// persistence level the export comes back sanitized, never the raw secret.
#[tauri::command]
fn export_item(id: String, state: State<'_, AppState>) -> Result<String, String> {
    disclosed_content(&id, &state)
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
    repo: Arc<dyn HistoryRepo>,
    persist_level: Arc<Mutex<PersistLevel>>,
    retention: Arc<Mutex<RetentionPolicy>>,
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
        if let Err(e) = repo.save(&item, level) {
            eprintln!("lapacho: failed to save clipboard item: {e}");
        }
        let policy = *retention.lock().unwrap();
        let _ = repo.cleanup(&policy);

        // Emit the UI-safe projection (masked for credentials/secrets).
        // Don't swallow the error: a failed emit is exactly the kind of bug
        // that makes the UI look like it isn't updating in real time.
        if let Err(e) = app.emit(EVENT_NEW_ITEM, UIClipboardItem::from(item)) {
            eprintln!("lapacho: failed to emit {EVENT_NEW_ITEM}: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Disables core dumps so a crash can't write decrypted secrets (clipboard
/// content, key material) to a core file. Linux-only; a no-op elsewhere.
#[cfg(target_os = "linux")]
fn harden_process() {
    // SAFETY: prctl with PR_SET_DUMPABLE is a simple, thread-safe process flag.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
}

#[cfg(not(target_os = "linux"))]
fn harden_process() {}

fn main() {
    harden_process();
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let db_path = data_dir.join("history.db");
            let plugins_dir = data_dir.join("plugins");
            // Key from the OS keyring (file fallback). The repo owns path + key
            // and initializes the schema on open.
            let key = keystore::load_or_create_key(&data_dir)?;
            let repo: Arc<dyn HistoryRepo> = Arc::new(SqliteRepo::new(db_path, key)?);
            let _ = plugins::init_plugins_dir(&plugins_dir);

            let persist_level = Arc::new(Mutex::new(PersistLevel::None));
            let retention = Arc::new(Mutex::new(RetentionPolicy::default()));
            let last_seen = Arc::new(Mutex::new(None));

            app.manage(AppState {
                repo: repo.clone(),
                plugins_dir,
                persist_level: persist_level.clone(),
                retention: retention.clone(),
                last_seen: last_seen.clone(),
            });

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                run_monitor(handle, repo, persist_level, retention, last_seen)
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_history,
            delete_item,
            clear_history,
            get_persist_level,
            set_persist_level,
            get_sensitive_ttl,
            set_sensitive_ttl,
            copy_item,
            export_item,
            list_plugins,
            run_plugin
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Lapacho desktop app");
}
