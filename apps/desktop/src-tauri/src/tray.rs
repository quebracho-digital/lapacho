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
use std::time::{Duration, Instant};

use base64::Engine as _;
use lapacho_core::ingest::sensitive_display;
use lapacho_core::types::{ClipboardItem, DetectedType, Sensitivity};
use tauri::menu::{IconMenuItem, Menu, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::AppState;

/// Tray id, used to look the icon up when we rebuild its menu.
pub const TRAY_ID: &str = "lapacho-tray";
/// How many recent clips to surface as native menu entries.
const TRAY_RECENT_CAP: usize = 25;
const TRAY_MENU_ITEMS: usize = 20;
/// Max characters per label before truncating (newlines collapsed to spaces).
const LABEL_MAX: usize = 50;

// Reserved menu-item ids. History items use UUIDs, which never start with
// `lapacho:`, so these can't collide.
const ID_OPEN: &str = "lapacho:open";
const ID_SEARCH: &str = "lapacho:search";
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
        let display = sensitive_display(&item.raw_content, sensitivity);
        let suffix = if sensitivity == Sensitivity::Secret { " [secret]" } else { " [credential]" };
        return format!("{}{}", display, suffix);
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

/// Returns the live tray items.
/// - Recent session copies (from tray_recent, which includes even non-persisted
///   sensitive items) come first.
/// - Then we supplement with older items from the persisted history (DB) so the
///   tray feels more like the full history the modal shows, while still
///   prioritizing what you just copied this session.
fn get_tray_items(app: &AppHandle) -> Vec<ClipboardItem> {
    let state = app.state::<AppState>();
    let mut result: Vec<ClipboardItem> = state.tray_recent.lock().unwrap().clone();

    // Load from DB and append items not already in recent (by id).
    // This makes tray show persistent history + recent on top.
    if let Ok(db_items) = state.repo.load() {
        for db in db_items {
            if result.iter().any(|r| r.id == db.id) {
                continue;
            }
            result.push(db);
            // Don't load the entire history into memory for the tray.
            if result.len() >= 50 {
                break;
            }
        }
    }

    crate::sort_for_display(&mut result);
    result
}

/// Returns an icon for the tray *indicator* (the panel icon) derived from the
/// top history item. Only images currently carry a thumbnail; everything else
/// (text, SVG, MD, …) falls back to the default Lapacho icon.
fn tray_icon_for_top(items: &[ClipboardItem]) -> Option<tauri::image::Image<'static>> {
    let top = items.first()?;
    if top.content_type != "image" {
        return None;
    }
    tray_icon_from_thumb(top.thumbnail.as_deref()?)
}

/// Builds the full tray menu from an already-loaded item list: the newest
/// clips, a separator, then the static "Open Lapacho…" / "Quit" entries.
fn build_menu(app: &AppHandle, items: &[ClipboardItem]) -> tauri::Result<Menu<Wry>> {
    let mut builder = MenuBuilder::new(app);
    if items.is_empty() {
        let empty = MenuItem::with_id(app, ID_EMPTY, "(no clips yet)", false, None::<&str>)?;
        builder = builder.item(&empty);
    } else {
        for it in items.iter().take(TRAY_MENU_ITEMS) {
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
    // A native GTK/AppIndicator menu can't host a text field, so the tray
    // can't search in place: this opens the window with the search box focused.
    let search = MenuItem::with_id(app, ID_SEARCH, "Buscar…  (Ctrl+Shift+Alt+V)", true, None::<&str>)?;
    let open = MenuItem::with_id(app, ID_OPEN, "Open Lapacho…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?;
    builder
        .item(&separator)
        .item(&search)
        .item(&open)
        .item(&quit)
        .build()
}

/// Rebuilds and installs the tray menu. Menu construction touches the platform
/// toolkit, so it must run on the main thread; `run_on_main_thread` is callable
/// from any thread (the monitor, the debounce thread, command handlers).
///
/// We also update the main tray *indicator icon* here (to the thumbnail of the
/// top item when it is an image). This gives visual feedback: the panel icon
/// reflects "what I copied last".
fn rebuild(app: &AppHandle) {
    if crate::trace_enabled() { eprintln!("lapacho: latency [rebuild enter]"); }
    let t0 = Instant::now();
    let handle = app.clone();
    let res = app.run_on_main_thread(move || {
        let t_main = Instant::now();
        // Load history once and reuse it for both the menu and the icon,
        // instead of decrypting the DB twice per rebuild.
        let items = get_tray_items(&handle);
        match build_menu(&handle, &items) {
            Ok(menu) => {
                if let Some(tray) = handle.tray_by_id(TRAY_ID) {
                    let _ = tray.set_menu(Some(menu));
                    if crate::trace_enabled() {
                        eprintln!(
                            "lapacho: latency [set_menu done] build+set {:?}",
                            t_main.elapsed()
                        );
                    }
                }
            }
            Err(e) => eprintln!("lapacho: tray rebuild failed: {e}"),
        }
        // Update the tray icon itself (debounced together with the menu).
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let icon = tray_icon_for_top(&items).or_else(|| handle.default_window_icon().cloned());
            let _ = tray.set_icon(icon);
        }
    });
    if let Err(e) = res {
        eprintln!("lapacho: could not schedule tray rebuild: {e}");
    } else {
        if crate::trace_enabled() { eprintln!("lapacho: latency [rebuild scheduled] {:?}", t0.elapsed()); }
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
        std::thread::sleep(Duration::from_millis(80));
        app.state::<AppState>()
            .tray_pending
            .store(false, Ordering::SeqCst);
        rebuild(&app);
    });
}

/// Shows and focuses the main window (creating nothing — it is pre-created and
/// merely hidden when the app launches to tray).
/// Also asks the webview to re-fetch history: the list only mounts once, and
/// live `clipboard-new` events can be missed while the window was hidden.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        raise(&window);
    }
    crate::request_history_refresh(app);
}

/// Opens the frameless quick-search window — search box plus the matching
/// clips, nothing else.
///
/// It is a separate window because it is declared `alwaysOnTop`, so it never
/// cedes stacking and the WM has nothing to refuse; the main window has to
/// beg for a raise (see [`raise`]) precisely because it does not. Falls back
/// to the main window if the spotlight one is missing.
pub fn show_search(app: &AppHandle) {
    let Some(window) = app.get_webview_window("spotlight") else {
        show_main(app);
        crate::request_search_focus(app);
        return;
    };
    pull_to_current_workspace(&window);
    let _ = window.center();
    let _ = window.show();
    let _ = window.set_focus();
    crate::request_search_focus(app);
}

/// Makes the window land on the workspace the user is on *right now*.
///
/// `alwaysOnTop` only wins stacking within one workspace, and GTK maps a hidden
/// window back onto the desktop it was last on. So from any other desktop
/// "open Lapacho" looks like nothing happened — the window really did open, two
/// workspaces away, and the only cure was remembering where it was. Sticking
/// makes it visible on every desktop; the delayed unstick then drops it onto
/// whichever one is active by then, which is the one the user asked from.
///
/// ponytail: the delay is a guess at WM latency, not a handshake. Switching
/// desktop inside that window makes the window follow — harmless, and cheaper
/// than tracking `_NET_CURRENT_DESKTOP` ourselves.
fn pull_to_current_workspace(window: &tauri::WebviewWindow) {
    let _ = window.set_visible_on_all_workspaces(true);
    let w = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let _ = w.set_visible_on_all_workspaces(false);
    });
}

/// Brings the window to the front and gives it keyboard focus.
///
/// `show` + `set_focus` alone is not enough: a hidden window can also be
/// minimized, and window managers with focus-stealing prevention ignore a
/// focus request from an app the user did not just interact with — which is
/// exactly our case, since the request comes from a tray click or a global
/// shortcut. Toggling always-on-top around the focus call is the portable way
/// to force the raise, because that *is* a stacking request rather than a
/// focus one, and WMs honour it.
///
/// ponytail: the always-on-top nudge is a workaround, not a fix. If a
/// compositor ignores it too, the real answer is an activation token
/// (xdg-activation on Wayland), which Tauri does not expose today.
fn raise(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    // Before the raise, not after: a raise aimed at another desktop is a raise
    // the user never sees.
    pull_to_current_workspace(window);
    let _ = window.show();
    // Hold the stacking request while the WM catches up. `set_focus` alone is
    // ignored by focus-stealing prevention, and a `show()` that hasn't been
    // mapped yet drops it too — so we re-ask a few times and only drop
    // always-on-top once focus has had a chance to land. Dropping it in the
    // same tick (the previous version) undid the one hint Muffin honours.
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();

    let w = window.clone();
    std::thread::spawn(move || {
        for delay in [60, 150, 300] {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            if w.is_focused().unwrap_or(false) {
                break;
            }
            let _ = w.set_focus();
        }
        // Give up the stacking override either way: staying on top forever is
        // worse than the occasional missed raise.
        let _ = w.set_always_on_top(false);
    });
}

/// Toggles the main window's visibility — bound to the global shortcut.
pub fn toggle_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            _ => {
                raise(&window);
                crate::request_history_refresh(app);
                // Opening by shortcut is almost always "I want to find
                // something", so land the cursor in the search box.
                crate::request_search_focus(app);
            }
        }
    }
}

/// Routes a tray menu click: static entries act; anything else is an item id,
/// whose raw content we copy back to the clipboard.
fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        ID_OPEN => show_main(app),
        ID_SEARCH => show_search(app),
        // Same path as a SIGTERM: the tray has to leave the bus before we go.
        ID_QUIT => crate::shutdown(app),
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
    let items = get_tray_items(app);
    let menu = build_menu(app, &items)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Lapacho — secure clipboard")
        .menu(&menu)
        .on_menu_event(on_menu_event);
    // Set the initial indicator icon to either the top history item's thumbnail
    // (if the most recent copy was an image) or the embedded Lapacho default.
    let initial_icon = tray_icon_for_top(&items).or_else(|| app.default_window_icon().cloned());
    if let Some(icon) = initial_icon {
        builder = builder.icon(icon);
    } else {
        eprintln!("lapacho: no default window icon; tray may be invisible");
    }
    builder.build(app)?;
    Ok(())
}
