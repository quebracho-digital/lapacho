//! System-tray indicator with a **native** menu of recent clips.
//!
//! The whole point (Diodon's lesson) is *fluidity*: the recent-items list is a
//! native indicator menu, not a webview — it renders instantly and the platform
//! positions and dismisses it for us. We rebuild it whenever the history
//! changes (debounced), pulling the newest items straight from the
//! [`HistoryRepo`]. Clicking an item copies its **raw** content back to the
//! clipboard. The native menu only carries text, so the rich per-item actions
//! (export, plugins, search) stay in the Leptos window, reachable via
//! "Abrir Lapacho…".

use std::sync::atomic::Ordering;
use std::time::Duration;

use base64::Engine as _;
use lapacho_core::types::{ClipboardItem, DetectedType, Sensitivity};
use tauri::menu::{IconMenuItem, Menu, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::AppState;

/// Tray id, used to look the icon up when we rebuild its menu.
pub const TRAY_ID: &str = "lapacho-tray";
/// How many recent clips to surface as native menu entries.
const TRAY_ITEMS: usize = 12;
/// Max characters per label before truncating (newlines collapsed to spaces).
const LABEL_MAX: usize = 50;

// Reserved menu-item ids. History items use UUIDs, which never start with
// `lapacho:`, so these can't collide.
const ID_OPEN: &str = "lapacho:open";
const ID_QUIT: &str = "lapacho:quit";
const ID_EMPTY: &str = "lapacho:empty";

/// Builds the one-line label for a clip.
/// For Credential/Secret we always derive a short distinguishable preview from the raw
/// (last non-control chars) so different sensitive items are identifiable in the
/// native tray menu, regardless of what display_content was stored at the time
/// (old history items had full redaction).
/// Non-sensitive use the (truncated) display.
fn item_label(item: &ClipboardItem) -> String {
    let sensitivity = item.sensitivity;
    let content_type = &item.content_type;
    let detected = &item.detected_type;

    if content_type == "image" {
        return "[image]".to_string();
    }

    if sensitivity == Sensitivity::Credential || sensitivity == Sensitivity::Secret {
        let preview = safe_preview(&item.raw_content);
        let suffix = if sensitivity == Sensitivity::Secret { " [secret]" } else { " [credential]" };
        return format!("••••{}{}", preview, suffix);
    }

    let display = &item.display_content;

    // Structured / diagram types: short label (avoid raw <svg> or long fenced source in tray)
    let base = match detected {
        DetectedType::Svg => "[SVG]".to_string(),
        DetectedType::Mermaid => "[Mermaid]".to_string(),
        DetectedType::Json => "[JSON]".to_string(),
        DetectedType::Markdown => {
            // For MD keep a short readable prefix (RustyBoard-like preview in tray)
            let one_line = display.replace(['\n', '\r'], " ");
            let trimmed = one_line.trim();
            if trimmed.is_empty() {
                "[Markdown]".to_string()
            } else if trimmed.chars().count() > LABEL_MAX {
                let head: String = trimmed.chars().take(LABEL_MAX - 1).collect();
                format!("{head}…")
            } else {
                trimmed.to_string()
            }
        }
        _ => {
            let one_line = display.replace(['\n', '\r'], " ");
            let trimmed = one_line.trim();
            if trimmed.is_empty() {
                "[empty]".to_string()
            } else if trimmed.chars().count() > LABEL_MAX {
                let head: String = trimmed.chars().take(LABEL_MAX - 1).collect();
                format!("{head}…")
            } else {
                trimmed.to_string()
            }
        }
    };
    base
}

/// Short safe preview from raw (up to 4 last non-control chars) for tray labels.
fn safe_preview(raw: &str) -> String {
    raw.chars()
        .rev()
        .filter(|c| !c.is_control())
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Decodes an 18×18 RGBA thumbnail (base64, as produced by `images.rs`) into a
/// native menu icon. `None` if it doesn't decode to exactly 18×18×4 bytes.
fn tray_icon_from_thumb(b64: &str) -> Option<tauri::image::Image<'static>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if bytes.len() == 18 * 18 * 4 {
        Some(tauri::image::Image::new_owned(bytes, 18, 18))
    } else {
        None
    }
}

/// Returns the live tray items (tray_recent buffer first, else fallback to
/// persisted history). This is the source for both the native menu and the
/// dynamic tray indicator icon.
fn get_tray_items(app: &AppHandle) -> Vec<ClipboardItem> {
    let state = app.state::<AppState>();
    let recent = state.tray_recent.lock().unwrap();
    if !recent.is_empty() {
        recent.clone()
    } else {
        state.repo.load().unwrap_or_default()
    }
}

/// Returns an icon for the tray *indicator* (the panel icon) derived from the
/// top history item. Only images currently carry a thumbnail; everything else
/// (text, SVG, MD, …) falls back to the default Lapacho icon.
fn tray_icon_for_top(app: &AppHandle) -> Option<tauri::image::Image<'static>> {
    let items = get_tray_items(app);
    if let Some(top) = items.first() {
        if top.content_type == "image" {
            if let Some(thumb) = &top.thumbnail {
                if let Some(icon) = tray_icon_from_thumb(thumb) {
                    return Some(icon);
                }
            }
        }
    }
    None
}

/// Builds the full tray menu: the newest clips, a separator, then the static
/// "Open Lapacho…" / "Quit" entries.
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let items = get_tray_items(app);

    let mut builder = MenuBuilder::new(app);
    if items.is_empty() {
        let empty = MenuItem::with_id(app, ID_EMPTY, "(no clips yet)", false, None::<&str>)?;
        builder = builder.item(&empty);
    } else {
        for it in items.iter().take(TRAY_ITEMS) {
            let label = item_label(it);
            // The id is the item's UUID; the menu-event handler routes it to copy.
            // Image items carry their 18×18 thumbnail as a native menu icon.
            if it.content_type == "image" {
                if let Some(icon) = it.thumbnail.as_deref().and_then(tray_icon_from_thumb) {
                    let entry =
                        IconMenuItem::with_id(app, &it.id, &label, true, Some(icon), None::<&str>)?;
                    builder = builder.item(&entry);
                    continue;
                }
            }
            let entry = MenuItem::with_id(app, &it.id, &label, true, None::<&str>)?;
            builder = builder.item(&entry);
        }
    }

    let separator = PredefinedMenuItem::separator(app)?;
    let open = MenuItem::with_id(app, ID_OPEN, "Open Lapacho…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?;
    builder.item(&separator).item(&open).item(&quit).build()
}

/// Rebuilds and installs the tray menu. Menu construction touches the platform
/// toolkit, so it must run on the main thread; `run_on_main_thread` is callable
/// from any thread (the monitor, the debounce thread, command handlers).
///
/// We also update the main tray *indicator icon* here (to the thumbnail of the
/// top item when it is an image). This gives visual feedback: the panel icon
/// reflects "what I copied last".
fn rebuild(app: &AppHandle) {
    let handle = app.clone();
    let res = app.run_on_main_thread(move || {
        match build_menu(&handle) {
            Ok(menu) => {
                if let Some(tray) = handle.tray_by_id(TRAY_ID) {
                    let _ = tray.set_menu(Some(menu));
                }
            }
            Err(e) => eprintln!("lapacho: tray rebuild failed: {e}"),
        }
        // Update the tray icon itself (debounced together with the menu).
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let icon = tray_icon_for_top(&handle).or_else(|| handle.default_window_icon().cloned());
            let _ = tray.set_icon(icon);
        }
    });
    if let Err(e) = res {
        eprintln!("lapacho: could not schedule tray rebuild: {e}");
    }
}

/// Debounced tray rebuild. A burst of changes (clearing history, copying a
/// few items, a flurry of clipboard activity) coalesces into a single rebuild
/// shortly after things settle, so we never pile menu work onto the main
/// thread. Uses a plain thread + sleep to avoid pulling in an async runtime.
pub fn schedule_rebuild(app: &AppHandle) {
    let state = app.state::<AppState>();
    // If a rebuild is already queued, let it cover this change too.
    if state.tray_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        app.state::<AppState>()
            .tray_pending
            .store(false, Ordering::SeqCst);
        rebuild(&app);
    });
}

/// Shows and focuses the main window (creating nothing — it is pre-created and
/// merely hidden when the app launches to tray).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Toggles the main window's visibility — bound to the global shortcut.
pub fn toggle_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            _ => {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    }
}

/// Routes a tray menu click: static entries act; anything else is an item id,
/// whose raw content we copy back to the clipboard.
fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        ID_OPEN => show_main(app),
        ID_QUIT => app.exit(0),
        ID_EMPTY => {}
        item_id => {
            let state = app.state::<AppState>();
            if let Err(e) = crate::copy_raw(item_id, &state) {
                eprintln!("lapacho: tray copy failed: {e}");
            }
        }
    }
}

/// Builds the tray icon with its initial menu and click handler. Call once
/// during setup, on the main thread.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Lapacho — secure clipboard")
        .menu(&menu)
        .on_menu_event(on_menu_event);
    // Set the initial indicator icon to either the top history item's thumbnail
    // (if the most recent copy was an image) or the embedded Lapacho default.
    let initial_icon = tray_icon_for_top(app).or_else(|| app.default_window_icon().cloned());
    if let Some(icon) = initial_icon {
        builder = builder.icon(icon);
    } else {
        eprintln!("lapacho: no default window icon; tray may be invisible");
    }
    builder.build(app)?;
    Ok(())
}
