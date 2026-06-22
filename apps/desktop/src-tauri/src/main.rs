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

mod images;
mod keystore;
mod tray;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lapacho_core::storage::{HistoryRepo, RetentionPolicy, SqliteRepo};
use lapacho_core::types::{ClipboardItem, PersistLevel, UIClipboardItem};
use lapacho_core::{PluginDefinition, Threat, assess, plugins, process_text};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// How often the monitor polls the system clipboard.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Event emitted to the frontend when a new clipboard item is captured.
/// Kept to a plain `a-z-` name to avoid any event-name validation surprises.
const EVENT_NEW_ITEM: &str = "clipboard-new";

/// Backend state shared between Tauri commands and the monitor thread.
pub(crate) struct AppState {
    /// History storage behind the [`HistoryRepo`] abstraction. The concrete
    /// backend (and the encryption key) is owned by the repo, not by the state.
    pub(crate) repo: Arc<dyn HistoryRepo>,
    plugins_dir: PathBuf,
    persist_level: Arc<Mutex<PersistLevel>>,
    /// Retention policy (sensitive TTL + size cap). Driven by the admin; mutable
    /// at runtime so a config/sync update takes effect without a restart.
    retention: Arc<Mutex<RetentionPolicy>>,
    /// Hash of the last clipboard value processed. Prevents re-ingesting our
    /// own writes (when the user copies an item back) and de-dupes repeats.
    last_seen: Arc<Mutex<Option<u64>>>,
    /// True while a debounced tray-menu rebuild is already queued, so bursts of
    /// changes coalesce into one rebuild instead of flooding the main thread.
    pub(crate) tray_pending: AtomicBool,
    /// Volatile buffer of the most recent captures (this session). Tray and
    /// live display pull from here so *new* copies always appear even if the
    /// active PersistLevel decided not to write them to disk.
    tray_recent: Arc<Mutex<Vec<ClipboardItem>>>,
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
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

/// Search the history (raw + display content, case-insensitive).
/// Used by the UI search box. Returns UI-safe projection.
#[tauri::command]
fn search_history(query: String, state: State<'_, AppState>) -> Result<Vec<UIClipboardItem>, String> {
    let items = state.repo.search(&query)?;
    Ok(items.into_iter().map(UIClipboardItem::from).collect())
}

#[tauri::command]
fn delete_item(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.repo.delete(&id)?;
    state.tray_recent.lock().unwrap().retain(|x| x.id != id);
    tray::schedule_rebuild(&app);
    Ok(())
}

#[tauri::command]
fn clear_history(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.repo.clear()?;
    state.tray_recent.lock().unwrap().clear();
    tray::schedule_rebuild(&app);
    Ok(())
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
    // Persist the choice so it survives restart.
    let _ = state.repo.set_preference("persist_level", &persist_level_to_str(lvl));
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
    // Persist the choice so it survives restart.
    let val = secs.map_or_else(|| "off".to_string(), |s| s.to_string());
    let _ = state.repo.set_preference("sensitive_ttl_secs", &val);
    state.repo.cleanup(&policy)
}

/// Loads a single history item by id. Shared by the commands that act on one
/// item (copy, export).
fn load_item(id: &str, state: &AppState) -> Result<ClipboardItem, String> {
    // Check volatile recent first (supports items copied this session that the
    // persist level chose not to write to the on-disk history).
    {
        let rec = state.tray_recent.lock().unwrap();
        if let Some(it) = rec.iter().find(|i| i.id == id).cloned() {
            return Ok(it);
        }
    }
    state
        .repo
        .load()?
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| "Item not found in history".to_string())
}

/// Copies a stored item's original content back to the system clipboard,
/// **intact** (raw) — that is the whole point of keeping the history. The
/// monitor is primed to ignore this value so it is not re-captured as new.
/// Shared by the `copy_item` command and the tray's click handler.
pub(crate) fn copy_raw(id: &str, state: &AppState) -> Result<(), String> {
    let item = load_item(id, state)?;
    // Prime the monitor to ignore our own write. Images and text both key off
    // `raw_content` (for images that's the PNG base64, which the monitor
    // re-derives deterministically when it reads the image back).
    *state.last_seen.lock().unwrap() = Some(hash_str(&item.raw_content));

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    if item.content_type == "image" {
        let data = images::image_data_from_b64(&item.raw_content)
            .ok_or_else(|| "invalid image in history".to_string())?;
        clipboard.set_image(data).map_err(|e| e.to_string())
    } else {
        clipboard.set_text(item.raw_content).map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn copy_item(id: String, state: State<'_, AppState>) -> Result<(), String> {
    copy_raw(&id, &state)
}

/// Raw content prepared for save/export, plus the security findings to surface
/// before it leaves the app. The content is returned **intact** — we never
/// rewrite what the user chose to keep; we only warn.
#[derive(serde::Serialize)]
struct ExportResult {
    content: String,
    threats: Vec<Threat>,
}

/// Prepares an item to be saved/exported (the "export" action). Returns the raw
/// content unchanged together with a threat assessment, so the UI can warn the
/// user (XSS, hidden unicode, secrets, …) before the value leaves Lapacho's
/// protections. The user always decides whether to proceed.
#[tauri::command]
fn export_item(id: String, state: State<'_, AppState>) -> Result<ExportResult, String> {
    let item = load_item(&id, &state)?;
    // For images, the "export" is the PNG data-URL; the text threat scanner
    // doesn't apply to a base64 blob, so skip it.
    if item.content_type == "image" {
        return Ok(ExportResult {
            content: item.display_content,
            threats: Vec::new(),
        });
    }
    let threats = assess(&item.raw_content);
    Ok(ExportResult {
        content: item.raw_content,
        threats,
    })
}

#[tauri::command]
fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginDefinition>, String> {
    plugins::load_plugins(&state.plugins_dir)
}

/// Runs a plugin over a stored item's **raw** content and saves the plugin's
/// output as a new history item.
///
/// The plugin receives the original content intact (the "two-way" contract: plugins
/// operate on raw, never on the masked projection). Its output is routed back
/// through the same ingest pipeline as the clipboard monitor, so the response
/// is stored raw and gets a freshly classified, sanitized display — masked if
/// the plugin happened to produce a secret. The new item is emitted to the UI
/// and returned.
#[tauri::command]
fn run_plugin(
    plugin_id: String,
    item_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UIClipboardItem, String> {
    let source = load_item(&item_id, &state)?;
    if source.content_type == "image" {
        return Err("Plugins operate on text, not images.".to_string());
    }
    let resp = plugins::execute_plugin(&state.plugins_dir, &plugin_id, &source.raw_content)?;
    if !resp.success {
        return Err(resp.error.unwrap_or_else(|| "Plugin failed".to_string()));
    }

    // Store the output raw; the pipeline derives the sanitized display. Saving
    // respects the active persist level (a no-op if the level forbids it), just
    // like the monitor — the item is still shown live either way.
    let item = process_text(&resp.result_raw_content);
    let level = *state.persist_level.lock().unwrap();
    if let Err(e) = state.repo.save(&item, level) {
        eprintln!("lapacho: failed to save plugin output: {e}");
    }
    // Track in tray_recent so it appears even under restrictive persist.
    {
        let mut rec = state.tray_recent.lock().unwrap();
        rec.retain(|x| x.id != item.id);
        rec.insert(0, item.clone());
        rec.truncate(12);
    }

    let ui = UIClipboardItem::from(item);
    if let Err(e) = app.emit(EVENT_NEW_ITEM, ui.clone()) {
        eprintln!("lapacho: failed to emit {EVENT_NEW_ITEM} for plugin output: {e}");
    }
    tray::schedule_rebuild(&app);
    Ok(ui)
}

// ---------------------------------------------------------------------------
// Clipboard monitor
// ---------------------------------------------------------------------------

/// Extracts the first `<svg>…</svg>` block from an HTML clipboard payload.
fn svg_from_html(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<svg")?;
    let rel_end = lower[start..].find("</svg>")?;
    let end = start + rel_end + "</svg>".len();
    Some(html[start..end].to_string())
}

/// Reads SVG from the HTML MIME type when plain text is empty (common in browsers).
fn try_clipboard_svg(clipboard: &mut arboard::Clipboard) -> Option<String> {
    let html = clipboard.get().html().ok()?;
    let svg = svg_from_html(&html)?;
    if svg.trim().is_empty() {
        None
    } else {
        Some(svg)
    }
}

fn run_monitor(
    app: AppHandle,
    repo: Arc<dyn HistoryRepo>,
    persist_level: Arc<Mutex<PersistLevel>>,
    retention: Arc<Mutex<RetentionPolicy>>,
    last_seen: Arc<Mutex<Option<u64>>>,
    tray_recent: Arc<Mutex<Vec<ClipboardItem>>>,
) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("lapacho: could not access the clipboard: {e}");
            return;
        }
    };

    // Persist (respecting the active level), run retention, push the UI-safe
    // projection live, and refresh the tray. Shared by the text and image paths.
    // A failed emit is not swallowed: that's exactly the bug that makes the UI
    // look like it isn't updating in real time.
    let persist_and_emit = |item: ClipboardItem| {
        let level = *persist_level.lock().unwrap();
        if let Err(e) = repo.save(&item, level) {
            eprintln!("lapacho: failed to save clipboard item: {e}");
        }
        let policy = *retention.lock().unwrap();
        let _ = repo.cleanup(&policy);
        if let Err(e) = app.emit(EVENT_NEW_ITEM, UIClipboardItem::from(item.clone())) {
            eprintln!("lapacho: failed to emit {EVENT_NEW_ITEM}: {e}");
        }
        // Always record in volatile recent so tray shows *new* copied items
        // this session even when persist level skips writing to DB.
        {
            let mut rec = tray_recent.lock().unwrap();
            rec.retain(|x| x.id != item.id);
            rec.insert(0, item.clone());
            rec.truncate(12);
        }
        tray::schedule_rebuild(&app);
    };

    // Cheap change-gate so an image sitting on the clipboard isn't re-encoded
    // to PNG on every poll.
    let mut last_img_rgba: Option<u64> = None;

    loop {
        std::thread::sleep(POLL_INTERVAL);

        // 1. Plain text (includes SVG copied as XML from editors).
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                last_img_rgba = None; // a text copy supersedes the image gate
                let hash = hash_str(&text);
                {
                    let mut last = last_seen.lock().unwrap();
                    if *last == Some(hash) {
                        continue;
                    }
                    *last = Some(hash);
                }
                persist_and_emit(process_text(&text));
                continue;
            }
        }

        // 2. HTML clipboard (browsers/vector apps often expose SVG only here).
        if let Some(svg) = try_clipboard_svg(&mut clipboard) {
            last_img_rgba = None;
            let hash = hash_str(&svg);
            {
                let mut last = last_seen.lock().unwrap();
                if *last == Some(hash) {
                    continue;
                }
                *last = Some(hash);
            }
            persist_and_emit(process_text(&svg));
            continue;
        }

        // 3. Raster image.
        if let Ok(img) = clipboard.get_image() {
            let rgba_hash = hash_bytes(&img.bytes);
            if last_img_rgba == Some(rgba_hash) {
                continue; // unchanged image — skip the expensive encode
            }
            last_img_rgba = Some(rgba_hash);

            let item = match images::process_image(img.width, img.height, &img.bytes) {
                Some(it) => it,
                None => continue,
            };
            // Gate on the PNG base64 (deterministic) so copying an image back to
            // the clipboard isn't re-captured as a brand-new item.
            let png_hash = hash_str(&item.raw_content);
            {
                let mut last = last_seen.lock().unwrap();
                if *last == Some(png_hash) {
                    continue;
                }
                *last = Some(png_hash);
            }
            persist_and_emit(item);
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

            // Load persisted user preferences (if any) so history rules survive restarts.
            if let Some(saved) = repo.get_preference("persist_level").ok().flatten() {
                *persist_level.lock().unwrap() = persist_level_from_str(&saved);
            }
            if let Some(saved) = repo.get_preference("sensitive_ttl_secs").ok().flatten() {
                let mut r = retention.lock().unwrap();
                if saved == "off" || saved == "none" {
                    r.sensitive_ttl_secs = None;
                } else if let Ok(secs) = saved.parse::<u64>() {
                    r.sensitive_ttl_secs = Some(secs);
                }
            }
            let last_seen = Arc::new(Mutex::new(None));
            let tray_recent = Arc::new(Mutex::new(Vec::new()));

            app.manage(AppState {
                repo: repo.clone(),
                plugins_dir,
                persist_level: persist_level.clone(),
                retention: retention.clone(),
                last_seen: last_seen.clone(),
                tray_pending: AtomicBool::new(false),
                tray_recent: tray_recent.clone(),
            });

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                run_monitor(handle, repo, persist_level, retention, last_seen, tray_recent)
            });

            // Native tray with the fluid recent-clips menu.
            tray::init(app.handle())?;

            // Global shortcut (Ctrl+Shift+Alt+L) toggles the main window. Registered
            // and handled entirely in Rust, so no webview capability is needed.
            // Lapacho-exclusive (L for Lapacho) to avoid conflicts with common
            // clipboard/terminal shortcuts.
            let toggle = Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT),
                Code::KeyL,
            );
            let toggle_for_handler = toggle;
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, shortcut, event| {
                        if event.state == ShortcutState::Pressed && shortcut == &toggle_for_handler
                        {
                            tray::toggle_main(app);
                        }
                    })
                    .build(),
            )?;
            if let Err(e) = app.global_shortcut().register(toggle) {
                eprintln!("lapacho: could not register Ctrl+Shift+Alt+L: {e}");
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Launch-to-tray app: closing the window hides it instead of
            // quitting, so Lapacho keeps watching the clipboard in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_history,
            search_history,
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

#[cfg(test)]
mod clipboard_tests {
    use super::svg_from_html;

    #[test]
    fn extracts_inline_svg_from_html() {
        let html = r#"<meta/><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
        let svg = svg_from_html(html).expect("svg");
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }
}
