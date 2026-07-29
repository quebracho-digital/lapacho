//! Frameless quick-search window: a box you type in and a list that narrows.
//!
//! Deliberately not the main UI in a smaller frame — no persistence selector,
//! no per-item actions, no modals. The only verbs are "find" and "pick", so
//! everything else would be a thing to skip past on the way to the clip.
//!
//! Runs the same WASM bundle as [`crate::app::App`]; which one mounts is
//! decided by the window label (see [`crate::bindings::window_label`]).

use leptos::prelude::*;
use leptos::web_sys::{self, KeyboardEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings;
use crate::types::UIClipboardItem;

/// One-line preview of a clip: its title if it has one, else its content
/// flattened, since a frameless row has no space for a second line.
fn preview(it: &UIClipboardItem) -> String {
    let body = it.display_content.replace(['\n', '\r', '\t'], " ");
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let body: String = body.chars().take(120).collect();
    match it.title.as_deref().filter(|t| !t.is_empty()) {
        Some(t) => format!("{t} — {body}"),
        None => body,
    }
}

#[component]
pub fn Spotlight() -> impl IntoView {
    let (query, set_query) = signal(String::new());
    let (items, set_items) = signal(Vec::<UIClipboardItem>::new());
    // Index of the highlighted row. Arrow keys move it; Enter picks it.
    let (cursor, set_cursor) = signal(0usize);

    // Seed with the full history so the window is useful before typing.
    spawn_local(async move {
        set_items.set(bindings::get_history().await);
    });

    // Re-opening only *shows* the window — the DOM is never rebuilt, so
    // `autofocus` fires once and never again. The backend pings us on every
    // open so the launcher always lands ready to type, with last time's query
    // cleared and a fresh list.
    bindings::listen_event("focus-search", move |_| {
        set_query.set(String::new());
        set_cursor.set(0);
        spawn_local(async move {
            set_items.set(bindings::get_history().await);
        });
        if let Some(el) = document()
            .get_element_by_id("search-input")
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            let _ = el.focus();
            el.select();
        }
    });

    // Re-run the search on every keystroke. The backend filters over decrypted
    // rows, so this is not a client-side filter of a stale list.
    let on_input = move |ev| {
        let q = event_target_value(&ev);
        set_query.set(q.clone());
        set_cursor.set(0);
        spawn_local(async move {
            let res = if q.trim().is_empty() {
                bindings::get_history().await
            } else {
                bindings::search_history(&q).await
            };
            set_items.set(res);
        });
    };

    let pick = move |id: String| {
        spawn_local(async move {
            let _ = bindings::pick_item(&id).await;
        });
    };

    let on_key = move |ev: KeyboardEvent| {
        let len = items.get().len();
        match ev.key().as_str() {
            "Escape" => {
                ev.prevent_default();
                spawn_local(async { bindings::hide_spotlight().await });
            }
            "ArrowDown" => {
                ev.prevent_default();
                if len > 0 {
                    set_cursor.update(|c| *c = (*c + 1).min(len - 1));
                }
            }
            "ArrowUp" => {
                ev.prevent_default();
                set_cursor.update(|c| *c = c.saturating_sub(1));
            }
            "Enter" => {
                ev.prevent_default();
                if let Some(it) = items.get().get(cursor.get()) {
                    pick(it.id.clone());
                }
            }
            _ => {}
        }
    };

    view! {
        <div id="spotlight">
            <input
                id="search-input"
                type="text"
                autofocus
                placeholder="Buscar en el portapapeles…"
                prop:value=move || query.get()
                on:input=on_input
                on:keydown=on_key
            />
            <ul>
                {move || {
                    let cur = cursor.get();
                    items
                        .get()
                        .into_iter()
                        .enumerate()
                        .map(|(i, it)| {
                            let id = it.id.clone();
                            let cls = if i == cur { "row sel" } else { "row" };
                            let sens = it.sensitivity.is_sensitive();
                            let tag_class = format!("tag s-{}", it.sensitivity.label());
                            let text = preview(&it);
                            view! {
                                <li class=cls on:click=move |_| pick(id.clone())>
                                    {it.pinned.then(|| view! { <span>"📌"</span> })}
                                    {it.vaulted.then(|| view! { <span>"🗄"</span> })}
                                    // Sensitive content stays masked here: the
                                    // launcher can be on screen in a meeting.
                                    <span class=if sens { "text masked" } else { "text" }>
                                        {if sens { "••••••••".to_string() } else { text }}
                                    </span>
                                    <span class=tag_class>{it.sensitivity.label()}</span>
                                </li>
                            }
                        })
                        .collect_view()
                }}
            </ul>
            {move || {
                items.get().is_empty().then(|| view! { <div class="none">"Sin resultados"</div> })
            }}
        </div>
    }
}
