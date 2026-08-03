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
use lapacho_core::ingest::sensitive_display;
use lapacho_core::security::{matches_secret_prefix, secret_prefix};
use lapacho_core::types::{ClipboardItem, PersistLevel, Sensitivity, UIClipboardItem};
use lapacho_core::{PluginDefinition, Threat, assess, plugins, process_text};
use tauri::{AppHandle, Emitter, Manager, State}; // Emitter used by emit_new_item / request_history_refresh
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use zeroize::Zeroize;

/// Max items kept in the volatile session buffer (`tray_recent`).
const TRAY_RECENT_MAX: usize = 25;

/// Scrub every plaintext-bearing field before an item is dropped from the
/// ephemeral buffer.
///
/// `raw_content` is the obvious one, but it is not the only copy: for a
/// `Public` item `display_content` *is* the payload, and `title` is whatever
/// the user typed to describe it. Scrubbing only `raw_content` left the other
/// two in freed memory, which defeats the point of the buffer.
fn zeroize_discarded(item: &mut ClipboardItem) {
    item.raw_content.zeroize();
    item.display_content.zeroize();
    if let Some(title) = item.title.as_mut() {
        title.zeroize();
    }
    // ponytail: `thumbnail` (18×18 RGBA of an image) is left alone — it is a
    // deliberate low-resolution preview, not the payload. Revisit if the
    // thumbnail size ever grows enough to reconstruct content.
}

/// Remove matching id from `tray_recent`, zeroizing the discarded item(s).
fn tray_recent_evict(rec: &mut Vec<ClipboardItem>, id: &str) {
    rec.retain_mut(|x| {
        if x.id == id {
            zeroize_discarded(x);
            false
        } else {
            true
        }
    });
}

/// Drop excess items past `max`, zeroizing each discarded payload.
fn tray_recent_truncate(rec: &mut Vec<ClipboardItem>, max: usize) {
    if rec.len() <= max {
        return;
    }
    for mut item in rec.drain(max..) {
        zeroize_discarded(&mut item);
    }
}

/// Clear the whole buffer, zeroizing every raw payload first.
fn tray_recent_clear(rec: &mut Vec<ClipboardItem>) {
    for item in rec.iter_mut() {
        zeroize_discarded(item);
    }
    rec.clear();
}

/// Replace an item in place by id, zeroizing the copy being displaced.
///
/// Assigning over a slot drops the previous `ClipboardItem` silently, so every
/// in-place replacement has to funnel through here or it leaks plaintext into
/// freed memory.
///
/// Returns the displaced item — already scrubbed — rather than a bool, so the
/// scrubbing is observable to a test instead of being a claim in a comment.
/// Callers are free to drop it.
fn tray_recent_replace(
    rec: &mut Vec<ClipboardItem>,
    id: &str,
    item: ClipboardItem,
) -> Option<ClipboardItem> {
    let slot = rec.iter_mut().find(|x| x.id == id)?;
    zeroize_discarded(slot);
    Some(std::mem::replace(slot, item))
}

/// Insert at front (dedup by id), then cap length — zeroizing all discards.
fn tray_recent_push_front(rec: &mut Vec<ClipboardItem>, item: ClipboardItem) {
    tray_recent_evict(rec, &item.id);
    rec.insert(0, item);
    tray_recent_truncate(rec, TRAY_RECENT_MAX);
}

/// How often the monitor polls the system clipboard.
/// Fallback polling interval when event-driven watching is not available
/// (or on non-Linux platforms). On Linux Wayland we prefer wl-paste --watch
/// which is truly event-driven and has zero CPU cost when idle.
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Event emitted to the frontend when a new clipboard item is captured.
/// Kept to a plain `a-z-` name to avoid any event-name validation surprises.
const EVENT_NEW_ITEM: &str = "clipboard-new";
/// Ask the UI to re-fetch history (window shown / live event fallback).
const EVENT_HISTORY_REFRESH: &str = "history-refresh";
/// Ask the UI to focus its search box (tray "Buscar…" entry).
const EVENT_FOCUS_SEARCH: &str = "focus-search";

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

/// Session recent + persisted history (recent first), as UI-safe items.
/// Matches the tray model so the list shows live captures even when the
/// webview missed a `clipboard-new` event (hidden window, race on listen).
fn merge_recent_and_db(state: &AppState) -> Result<Vec<ClipboardItem>, String> {
    let mut result: Vec<ClipboardItem> = state.tray_recent.lock().unwrap().clone();
    for db in state.repo.load()? {
        if result.iter().any(|r| r.id == db.id) {
            continue;
        }
        result.push(db);
        if result.len() >= 100 {
            break;
        }
    }
    sort_for_display(&mut result);
    Ok(result)
}

/// The order the user sees in the list and the tray: items they asked to keep
/// float to the top, everything else by recency.
///
/// Without this, pinning an old item does nothing visible — it stays buried at
/// whatever position its timestamp puts it, which is the opposite of what "keep
/// this one" means.
pub(crate) fn sort_for_display(items: &mut [ClipboardItem]) {
    items.sort_by_key(|i| (!(i.pinned || i.vaulted), std::cmp::Reverse(i.timestamp)));
}

/// Returns the live list: session buffer first, then DB (never includes `raw_content`).
#[tauri::command]
fn get_history(state: State<'_, AppState>) -> Result<Vec<UIClipboardItem>, String> {
    let items = merge_recent_and_db(&state)?;
    Ok(items.into_iter().map(UIClipboardItem::from).collect())
}

/// Search session recent + persisted history (raw + display, case-insensitive).
#[tauri::command]
fn search_history(query: String, state: State<'_, AppState>) -> Result<Vec<UIClipboardItem>, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return get_history(state);
    }
    let items = merge_recent_and_db(&state)?;
    Ok(items
        .into_iter()
        .filter(|it| {
            it.raw_content.to_lowercase().contains(&q)
                || it.display_content.to_lowercase().contains(&q)
                // The point of a title: find the item by what it is, not by
                // what it says. Kept in sync with `HistoryRepo::search`.
                || it.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&q))
        })
        .map(UIClipboardItem::from)
        .collect())
}

/// Emit a new item to the main webview (preferred) and as a global fallback.
fn emit_new_item(app: &AppHandle, ui: UIClipboardItem) {
    if let Some(w) = app.get_webview_window("main") {
        if let Err(e) = w.emit(EVENT_NEW_ITEM, ui.clone()) {
            eprintln!("lapacho: failed to emit {EVENT_NEW_ITEM} to main: {e}");
        }
    }
    if let Err(e) = app.emit(EVENT_NEW_ITEM, ui) {
        eprintln!("lapacho: failed to emit {EVENT_NEW_ITEM}: {e}");
    }
}

/// Tell the UI to re-pull history (e.g. when the window is shown again).
pub(crate) fn request_history_refresh(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Err(e) = w.emit(EVENT_HISTORY_REFRESH, ()) {
            eprintln!("lapacho: failed to emit {EVENT_HISTORY_REFRESH} to main: {e}");
        }
    }
    if let Err(e) = app.emit(EVENT_HISTORY_REFRESH, ()) {
        eprintln!("lapacho: failed to emit {EVENT_HISTORY_REFRESH}: {e}");
    }
}

/// Tell the UI to put the cursor in the search box. Emitted after `show_main`
/// by the tray's "Buscar…" entry, which can't hold a text field itself.
pub(crate) fn request_search_focus(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Err(e) = w.emit(EVENT_FOCUS_SEARCH, ()) {
            eprintln!("lapacho: failed to emit {EVENT_FOCUS_SEARCH} to main: {e}");
        }
    }
}

#[tauri::command]
fn delete_item(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.repo.delete(&id)?;
    tray_recent_evict(&mut state.tray_recent.lock().unwrap(), &id);
    tray::schedule_rebuild(&app);
    Ok(())
}

#[tauri::command]
fn clear_history(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.repo.clear()?;
    tray_recent_clear(&mut state.tray_recent.lock().unwrap());
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
fn set_persist_level(level: String, state: State<'_, AppState>) -> Result<usize, String> {
    let lvl = persist_level_from_str(&level);
    *state.persist_level.lock().unwrap() = lvl;
    // Persist the choice so it survives restart.
    let _ = state.repo.set_preference("persist_level", &persist_level_to_str(lvl));
    // Apply the new level to what is *already* on disk, not just to future
    // writes. Without this, picking Paranoia leaves every secret written under
    // a laxer level sitting there, under a label saying it is not kept.
    let purged = state.repo.purge_forbidden(lvl)?;
    let policy = *state.retention.lock().unwrap();
    state.repo.cleanup(&policy)?;
    Ok(purged)
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
        .get_by_id(id)?
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

/// Copies the item and dismisses the quick-search window in one step — the
/// whole point of the launcher is that picking a clip ends the interaction.
#[tauri::command]
fn pick_item(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    copy_raw(&id, &state)?;
    hide_spotlight(app);
    Ok(())
}

/// Dismisses the quick-search window (Esc, or after picking a clip).
#[tauri::command]
fn hide_spotlight(app: AppHandle) {
    if let Some(w) = app.get_webview_window("spotlight") {
        let _ = w.hide();
    }
}

/// User says "this is a secret". Remembers the choice (keyed by content id, so
/// no plaintext is stored) and re-masks the item everywhere. The next time the
/// same content is copied, the monitor classifies it Secret from the start.
#[tauri::command]
fn mark_secret(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut item = load_item(&id, &state)?;
    // Learn: the id *is* the keyed content hash (crypto::content_id).
    state.repo.set_preference(&format!("secret:{id}"), "1")?;
    // Generalize: if the value has token structure (literal prefix + random
    // tail, e.g. "acme_live_9fK2…"), learn the prefix so *different* future
    // values with the same shape classify Secret too. Only the prefix is
    // stored — never the secret.
    if let Some(prefix) = secret_prefix(&item.raw_content) {
        let existing = state
            .repo
            .get_preference("secret_prefixes")?
            .unwrap_or_default();
        if !existing.split('\n').any(|p| p == prefix) {
            let updated = if existing.is_empty() { prefix } else { format!("{existing}\n{prefix}") };
            state.repo.set_preference("secret_prefixes", &updated)?;
        }
    }
    if item.sensitivity != Sensitivity::Secret {
        item.sensitivity = Sensitivity::Secret;
        item.display_content = sensitive_display(&item.raw_content, Sensitivity::Secret);
        // Re-persist under the active level: delete + save so sensitivity and
        // display are rewritten (save's upsert only refreshes the timestamp).
        // Under PersistLevel::Sensitive/None this drops the row — correct:
        // secrets must not stay on disk at those levels.
        state.repo.delete(&id)?;
        let level = *state.persist_level.lock().unwrap();
        state.repo.save(&item, level)?;
        {
            let mut rec = state.tray_recent.lock().unwrap();
            tray_recent_replace(&mut rec, &id, item.clone());
        }
        tray::schedule_rebuild(&app);
        request_history_refresh(&app);
    }
    Ok(())
}

/// Names an item so it can be found by what it is, not by its content. An empty
/// title clears it.
#[tauri::command]
fn set_item_title(id: String, title: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let title = title.trim();
    let title = (!title.is_empty()).then_some(title);
    state.repo.set_title(&id, title)?;
    {
        let mut rec = state.tray_recent.lock().unwrap();
        if let Some(slot) = rec.iter_mut().find(|x| x.id == id) {
            slot.title = title.map(str::to_string);
        }
    }
    tray::schedule_rebuild(&app);
    request_history_refresh(&app);
    Ok(())
}

/// "Keep this one": exempts the item from the history size cap. Returns the new
/// state so the UI doesn't have to guess.
///
/// Deliberately *not* a persistence override: at the Paranoia level a sensitive
/// item was never written to disk, so pinning it can only keep it for this
/// session. Making the pin force a secret onto disk would turn a UI affordance
/// into a hole in the persist policy.
#[tauri::command]
fn toggle_pin(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let pinned = !load_item(&id, &state)?.pinned;
    state.repo.set_pinned(&id, pinned)?;
    {
        let mut rec = state.tray_recent.lock().unwrap();
        if let Some(slot) = rec.iter_mut().find(|x| x.id == id) {
            slot.pinned = pinned;
        }
    }
    tray::schedule_rebuild(&app);
    request_history_refresh(&app);
    Ok(pinned)
}

/// "Guardar en bóveda": the one per-item override of the persist level.
///
/// Turning it **on** cannot be an UPDATE: at the Paranoia level the item was
/// never written, so there is no row to flag. It goes through `save` with
/// `vaulted` already set, which forces the write.
///
/// Turning it **off** re-applies the active level immediately: if the level
/// forbids the item, it leaves the disk right now rather than lingering until
/// the next cleanup. Dropping the vault flag has to mean the item is gone, or
/// "unvault" would be a promise the app keeps only eventually.
#[tauri::command]
fn toggle_vault(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let mut item = load_item(&id, &state)?;
    let level = *state.persist_level.lock().unwrap();
    let vaulted = !item.vaulted;

    if vaulted {
        item.vaulted = true;
        state.repo.save(&item, level)?;
    } else if level.persists(item.sensitivity) {
        // Still allowed at this level: keep the row, just drop the flag.
        state.repo.unvault(&id)?;
        item.vaulted = false;
    } else {
        state.repo.delete(&id)?;
        item.vaulted = false;
    }

    {
        let mut rec = state.tray_recent.lock().unwrap();
        if let Some(slot) = rec.iter_mut().find(|x| x.id == id) {
            slot.vaulted = vaulted;
        }
    }
    tray::schedule_rebuild(&app);
    request_history_refresh(&app);
    Ok(vaulted)
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
    let mut item = process_text(&resp.result_raw_content);
    item.id = state.repo.content_id(&item.raw_content);
    let level = *state.persist_level.lock().unwrap();
    if let Err(e) = state.repo.save(&item, level) {
        eprintln!("lapacho: failed to save plugin output: {e}");
    }
    // Track in tray_recent so it appears even under restrictive persist.
    {
        let mut rec = state.tray_recent.lock().unwrap();
        tray_recent_push_front(&mut rec, item.clone());
    }

    let ui = UIClipboardItem::from(item);
    emit_new_item(&app, ui.clone());
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
        let t0 = std::time::Instant::now();
        let mut item = item;
        item.id = repo.content_id(&item.raw_content);
        // User-taught secrets: exact content match (keyed hash) or a learned
        // token prefix (e.g. "acme_live_") override the heuristic.
        if item.sensitivity != Sensitivity::Secret
            && (matches!(repo.get_preference(&format!("secret:{}", item.id)), Ok(Some(_)))
                || repo
                    .get_preference("secret_prefixes")
                    .ok()
                    .flatten()
                    .is_some_and(|ps| {
                        ps.split('\n').any(|p| matches_secret_prefix(&item.raw_content, p))
                    }))
        {
            item.sensitivity = Sensitivity::Secret;
            item.display_content = sensitive_display(&item.raw_content, Sensitivity::Secret);
        }
        let level = *persist_level.lock().unwrap();
        if let Err(e) = repo.save(&item, level) {
            eprintln!("lapacho: failed to save clipboard item: {e}");
        }
        let policy = *retention.lock().unwrap();
        let _ = repo.cleanup(&policy);
        // Always record in volatile recent so tray/list show *new* copied items
        // this session even when persist level skips writing to DB.
        {
            let mut rec = tray_recent.lock().unwrap();
            tray_recent_push_front(&mut rec, item.clone());
        }
        emit_new_item(&app, UIClipboardItem::from(item));
        tray::schedule_rebuild(&app);
        eprintln!(
            "lapacho: latency [persist_and_emit exit] {:?}",
            t0.elapsed()
        );
    };

    // Cheap change-gate so an image sitting on the clipboard isn't re-encoded
    // to PNG on every poll.
    let mut last_img_rgba: Option<u64> = None;

    // On Linux Wayland we can (and should) avoid polling entirely for battery life.
    // wl-paste --watch is event-driven: the compositor wakes us only on actual changes.
    #[cfg(target_os = "linux")]
    if is_wayland() {
        if let Err(e) = run_wayland_watcher(
            &mut clipboard,
            &persist_and_emit,
            &mut last_img_rgba,
            &last_seen,
        ) {
            eprintln!("lapacho: wl-paste watcher failed ({}), falling back to polling", e);
        } else {
            return;
        }
    }

    use clipboard_master::{CallbackResult, ClipboardHandler, Master};
    use std::sync::mpsc;

    struct ClipNotify { tx: mpsc::Sender<()> }
    impl ClipboardHandler for ClipNotify {
        fn on_clipboard_change(&mut self) -> CallbackResult {
            let _ = self.tx.send(());
            CallbackResult::Next
        }
        fn on_clipboard_error(&mut self, e: std::io::Error) -> CallbackResult {
            eprintln!("lapacho: clipboard monitor error: {e}");
            CallbackResult::Next
        }
    }

    let (tx, rx) = mpsc::channel::<()>();
    let spawned = std::thread::Builder::new()
        .name("clip-xfixes".into())
        .spawn(move || match Master::new(ClipNotify { tx }) {
            Ok(mut m) => { let _ = m.run(); }
            Err(e) => eprintln!("lapacho: could not start clipboard monitor: {e}"),
        });

    if spawned.is_ok() {
        // El evento solo dispara en cambios futuros → un snapshot inicial.
        check_clipboard_once(&mut clipboard, &persist_and_emit, &mut last_img_rgba, &last_seen);
        while rx.recv().is_ok() {
            check_clipboard_once(&mut clipboard, &persist_and_emit, &mut last_img_rgba, &last_seen);
        }
    } else {
        // Fallback: polling, solo si no se pudo lanzar el monitor de eventos.
        loop {
            check_clipboard_once(&mut clipboard, &persist_and_emit, &mut last_img_rgba, &last_seen);
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

/// Returns true if we are running under a Wayland session.
#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v == "wayland")
            .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn is_wayland() -> bool {
    false
}

/// Event-driven clipboard watcher for Wayland using `wl-paste --watch`.
/// This is the battery-friendly path: we only wake when the compositor tells us
/// the clipboard changed. No periodic polling.
#[cfg(target_os = "linux")]
fn run_wayland_watcher(
    clipboard: &mut arboard::Clipboard,
    persist_and_emit: &dyn Fn(ClipboardItem),
    last_img_rgba: &mut Option<u64>,
    last_seen: &Arc<Mutex<Option<u64>>>,
) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    // We use "echo CLIP_CHANGED" because it is simple and reliable.
    // Every time the clipboard changes, wl-paste will run the command.
    // Our thread blocks on read_line until that happens.
    let mut child = Command::new("wl-paste")
        .args(["--watch", "echo", "CLIP_CHANGED"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn wl-paste: {}. Is wl-clipboard installed?", e))?;

    let stdout = child.stdout.take().ok_or("wl-paste has no stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                // EOF, watcher died. Restart it.
                eprintln!("lapacho: wl-paste watcher exited, restarting...");
                std::thread::sleep(Duration::from_millis(500));
                // Recreate child (simple restart)
                child = Command::new("wl-paste")
                    .args(["--watch", "echo", "CLIP_CHANGED"])
                    .stdout(Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("failed to respawn wl-paste: {}", e))?;
                continue;
            }
            Ok(_) => {
                if line.trim() == "CLIP_CHANGED" {
                    // Real change notification. Now inspect current clipboard
                    // using arboard (same logic as the poll path).
                    check_clipboard_once(clipboard, persist_and_emit, last_img_rgba, last_seen);
                }
            }
            Err(e) => {
                eprintln!("lapacho: error reading from wl-paste: {}", e);
                return Err(e.to_string());
            }
        }
    }
}

/// One-shot inspection of the current clipboard state.
/// Extracted so it can be used from both the polling path and the Wayland watcher.
fn check_clipboard_once(
    clipboard: &mut arboard::Clipboard,
    persist_and_emit: &dyn Fn(ClipboardItem),
    last_img_rgba: &mut Option<u64>,
    last_seen: &Arc<Mutex<Option<u64>>>,
) {
    // 1. Plain text
    if let Ok(text) = clipboard.get_text() {
        if !text.is_empty() {
            *last_img_rgba = None;
            let hash = hash_str(&text);
            {
                let mut last = last_seen.lock().unwrap();
                if *last == Some(hash) {
                    return;
                }
                *last = Some(hash);
            }
            eprintln!("lapacho: latency [detect] text");
            persist_and_emit(process_text(&text));
            return;
        }
    }

    // 2. HTML (for SVG etc.)
    if let Ok(html) = clipboard.get().html() {
        if let Some(svg) = svg_from_html(&html) {
            if !svg.trim().is_empty() {
                *last_img_rgba = None;
                let hash = hash_str(&svg);
                {
                    let mut last = last_seen.lock().unwrap();
                    if *last == Some(hash) {
                        return;
                    }
                    *last = Some(hash);
                }
                eprintln!("lapacho: latency [detect] svg/html");
                persist_and_emit(process_text(&svg));
                return;
            }
        }
    }

    // 3. Image
    if let Ok(img) = clipboard.get_image() {
        let rgba_hash = hash_bytes(&img.bytes);
        if *last_img_rgba == Some(rgba_hash) {
            return;
        }
        *last_img_rgba = Some(rgba_hash);

        if let Some(item) = images::process_image(img.width, img.height, &img.bytes) {
            let png_hash = hash_str(&item.raw_content);
            {
                let mut last = last_seen.lock().unwrap();
                if *last == Some(png_hash) {
                    return;
                }
                *last = Some(png_hash);
            }
            eprintln!("lapacho: latency [detect] image");
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

/// WebKitGTK on some AMD/NVIDIA stacks paints a blank (black/white) webview when
/// the DMABUF renderer fails. Tauri docs recommend disabling it before any
/// WebKit init. Harmless when the driver is fine; set before `Builder::run`.
#[cfg(target_os = "linux")]
fn mitigate_webkit_blank_window() {
    // Only set if the user did not already choose a value.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: env mutation before other threads start; single-threaded main.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn mitigate_webkit_blank_window() {}

/// Routes SIGTERM/SIGINT through Tauri's own shutdown instead of the default
/// "die immediately" disposition.
///
/// Dying with the tray's StatusNotifierItem still registered leaves
/// `xapp-sn-watcher` holding a proxy to a name that will never reply. Cinnamon
/// keeps that dead `GDBusProxy` on the GJS heap with its `g-properties-changed`
/// handlers attached; when the GC later sweeps it, it tries to call back into
/// JS mid-sweep and blocks `paint()` — the compositor stops drawing and the
/// screen goes black until something forces a full repaint.
///
/// Signals are blocked process-wide first so no other thread can take one and
/// terminate on our behalf; a dedicated thread then waits for them. This must
/// run before any thread is spawned, since the mask is inherited.
///
/// ponytail: `sigwait` on a blocked set rather than a `signal()` handler,
/// because a handler may only call async-signal-safe functions and
/// `AppHandle::exit` is nowhere near that. SIGKILL is still unfixable by
/// design — nothing runs on `kill -9`.
/// The signals we take over, in one place so the mask and the wait cannot
/// disagree about which ones they cover.
unsafe fn shutdown_sigset() -> libc::sigset_t {
    let mut set: libc::sigset_t = std::mem::zeroed();
    libc::sigemptyset(&mut set);
    libc::sigaddset(&mut set, libc::SIGTERM);
    libc::sigaddset(&mut set, libc::SIGINT);
    libc::sigaddset(&mut set, libc::SIGHUP);
    set
}

/// Blocks the shutdown signals process-wide. **Must be the first thing `main`
/// does**: the mask is only inherited by threads spawned afterwards, so any
/// thread that already exists keeps its default disposition and will terminate
/// the process the moment a signal lands — the exact bug this used to have,
/// when the call sat after `Builder::build`.
fn block_shutdown_signals() {
    unsafe {
        let set = shutdown_sigset();
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Takes the tray icon off DBus, lets the main loop actually deliver that,
/// and only then asks Tauri to exit.
///
/// The delay is the whole point. Removing the tray in `RunEvent::Exit` is too
/// late: by then the GTK main loop is tearing down and the unregister message
/// never reaches the bus, so `xapp-sn-watcher` is left calling a name that
/// will never answer (`NoReply`) — which is what strands a dead `GDBusProxy`
/// on Cinnamon's GJS heap and eventually blocks `paint()` mid-GC.
///
/// ponytail: a fixed 300ms wait instead of watching for the bus to confirm
/// the removal. Tauri exposes no such acknowledgement; if this proves flaky,
/// the upgrade is to own the StatusNotifierItem directly (ksni) rather than
/// to grow the timeout.
pub(crate) fn shutdown(app: &AppHandle) {
    let _ = app.remove_tray_by_id(tray::TRAY_ID);
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        handle.exit(0);
    });
}

/// Waits for a blocked shutdown signal and routes it through [`shutdown`]
/// instead of letting the default disposition kill the process outright.
fn spawn_signal_waiter(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut sig: libc::c_int = 0;
        let ok = unsafe {
            let set = shutdown_sigset();
            libc::sigwait(&set, &mut sig) == 0
        };
        if ok {
            eprintln!("lapacho: signal {sig}, shutting down cleanly");
            shutdown(&app);
        }
    });
}

fn main() {
    // First statement on purpose — see `block_shutdown_signals`.
    block_shutdown_signals();
    harden_process();
    mitigate_webkit_blank_window();
    let app = tauri::Builder::default()
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
            // Enforce the loaded rules against what is *already* on disk.
            // `set_persist_level` purges on change, but that only covers the
            // one moment the user touches the setting: rows written by an
            // older build, or under a level that was later lowered while this
            // code wasn't there to act, survive every restart under a label
            // saying they are not kept. The level is a promise about the disk,
            // so it has to be re-applied every time we open the disk.
            let saved_level = *persist_level.lock().unwrap();
            match repo.purge_forbidden(saved_level) {
                Ok(n) if n > 0 => eprintln!("lapacho: purged {n} item(s) the saved persist level forbids"),
                Err(e) => eprintln!("lapacho: startup purge failed: {e}"),
                _ => {}
            }
            // Same reason for the TTL: reading history doesn't trigger cleanup,
            // so without this an app closed for a week shows expired secrets
            // until the next copy happens to run retention.
            let saved_policy = *retention.lock().unwrap();
            if let Err(e) = repo.cleanup(&saved_policy) {
                eprintln!("lapacho: startup cleanup failed: {e}");
            }
            // Last, so it reclaims what the two passes above just freed. Here
            // rather than on a timer because it wants the database to itself,
            // and startup is the one moment the monitor isn't writing yet.
            match repo.compact() {
                Ok(n) if n > 0 => eprintln!("lapacho: reclaimed {} MB of deleted history", n / 1_048_576),
                Err(e) => eprintln!("lapacho: compaction failed: {e}"),
                _ => {}
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

            // Debug aid: LAPACHO_SHOW=1 opens the window on launch.
            if std::env::var_os("LAPACHO_SHOW").is_some() {
                tray::show_main(app.handle());
            }

            // Global shortcut (Ctrl+Shift+Alt+L) toggles the main window. Registered
            // and handled entirely in Rust, so no webview capability is needed.
            // Lapacho-exclusive (L for Lapacho) to avoid conflicts with common
            // clipboard/terminal shortcuts.
            let toggle = Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT),
                Code::KeyL,
            );
            // Dedicated search shortcut: always opens (never toggles shut) and
            // lands the cursor in the box, so the muscle memory is "V = find a
            // clip" without the risk of hiding the window you just asked for.
            let search = Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT),
                Code::KeyV,
            );
            let toggle_for_handler = toggle;
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, shortcut, event| {
                        if event.state != ShortcutState::Pressed {
                            return;
                        }
                        if shortcut == &toggle_for_handler {
                            tray::toggle_main(app);
                        } else if shortcut == &search {
                            tray::show_search(app);
                        }
                    })
                    .build(),
            )?;
            for (sc, name) in [(toggle, "Ctrl+Shift+Alt+L"), (search, "Ctrl+Shift+Alt+V")] {
                if let Err(e) = app.global_shortcut().register(sc) {
                    eprintln!("lapacho: could not register {name}: {e}");
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Launch-to-tray app: closing the window hides it instead of
            // quitting, so Lapacho keeps watching the clipboard in the tray.
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let _ = window.hide();
                    api.prevent_close();
                }
                // A launcher that stays up after you click elsewhere is just a
                // window in the way. Only the spotlight behaves this way — the
                // main window is a normal window you leave open on purpose.
                tauri::WindowEvent::Focused(false) if window.label() == "spotlight" => {
                    let _ = window.hide();
                }
                _ => {}
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
            pick_item,
            hide_spotlight,
            mark_secret,
            set_item_title,
            toggle_pin,
            toggle_vault,
            export_item,
            list_plugins,
            run_plugin
        ])
        .build(tauri::generate_context!())
        .expect("error while running the Lapacho desktop app");

    spawn_signal_waiter(app.handle().clone());

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Unregister the tray icon *before* the process goes away, so the
            // StatusNotifierItem leaves the bus with a live connection to
            // answer on. See `install_signal_shutdown` for what a dirty exit
            // does to Cinnamon's compositor.
            let _ = handle.remove_tray_by_id(tray::TRAY_ID);
        }
    });
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

/// The ephemeral session buffer promises that a discarded payload stops
/// existing. These pin the two ways that promise was broken.
#[cfg(test)]
mod tray_recent_tests {
    use super::*;
    use lapacho_core::types::DetectedType;

    fn item(id: &str, body: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_string(),
            raw_content: body.to_string(),
            display_content: body.to_string(),
            content_type: "text".to_string(),
            sensitivity: Sensitivity::None,
            detected_type: DetectedType::Text,
            timestamp: 0,
            thumbnail: None,
            size: None,
            title: Some(format!("title-{body}")),
            pinned: false,
            vaulted: false,
            sync_id: None,
            sync_eligible: false,
            sync_state: "local".to_string(),
        }
    }

    #[test]
    fn zeroize_scrubs_every_plaintext_field_not_just_raw() {
        let mut it = item("a", "hunter2");
        zeroize_discarded(&mut it);
        assert!(it.raw_content.is_empty(), "raw_content survived");
        // Regression: `display_content` *is* the payload for a public item,
        // and used to be left intact.
        assert!(it.display_content.is_empty(), "display_content survived");
        assert_eq!(it.title.as_deref(), Some(""), "title survived");
    }

    #[test]
    fn replace_zeroizes_the_displaced_item() {
        // Regression: `mark_secret` assigned over the slot, dropping the old
        // item without scrubbing it — on the very path where the user just
        // said "this is a secret".
        let mut rec = vec![item("a", "hunter2")];
        let displaced = tray_recent_replace(&mut rec, "a", item("a", "masked"))
            .expect("slot should match");
        assert!(displaced.raw_content.is_empty(), "displaced raw_content survived");
        assert!(
            displaced.display_content.is_empty(),
            "displaced display_content survived"
        );
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].raw_content, "masked");
    }

    #[test]
    fn replace_reports_a_miss_instead_of_inserting() {
        let mut rec = vec![item("a", "x")];
        assert!(tray_recent_replace(&mut rec, "nope", item("nope", "y")).is_none());
        assert_eq!(rec.len(), 1);
    }

    #[test]
    fn push_front_dedups_by_id_and_caps_at_max() {
        let mut rec = Vec::new();
        for i in 0..TRAY_RECENT_MAX + 10 {
            tray_recent_push_front(&mut rec, item(&format!("id{i}"), "body"));
        }
        assert_eq!(rec.len(), TRAY_RECENT_MAX);
        // Newest first, oldest dropped.
        assert_eq!(rec[0].id, format!("id{}", TRAY_RECENT_MAX + 9));

        tray_recent_push_front(&mut rec, item("id0", "again"));
        assert_eq!(rec.len(), TRAY_RECENT_MAX);
        assert_eq!(rec.iter().filter(|x| x.id == "id0").count(), 1);
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut rec = vec![item("a", "x"), item("b", "y")];
        tray_recent_clear(&mut rec);
        assert!(rec.is_empty());
    }
}
