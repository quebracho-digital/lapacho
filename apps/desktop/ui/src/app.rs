//! The Lapacho clipboard UI.
//!
//! Ports the original vanilla popup (live list, copy/delete/clear, persistence
//! selector) to Leptos and adds the item-decoupling actions the backend now
//! supports: maximize (full view), export (raw content + threat assessment),
//! and run-a-plugin (operates on raw, result becomes a new item).

use base64::Engine as _;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

use crate::bindings;
use crate::types::{DetectedType, ExportResult, PluginDef, UIClipboardItem};

/// Relative time label, e.g. "3m ago".
fn time_ago(ts: u64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as i64;
    let s = (now - ts as i64).max(0);
    if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else {
        format!("{}h ago", s / 3600)
    }
}

// ---------------------------------------------------------------------------
// Rich rendering by detected type
//
// All previews are built client-side and offline. Security note: the only path
// that injects HTML into the webview is Markdown (via inner_html). We feed the
// original source to pulldown-cmark (for correct code/literal <>&) and drop
// raw HTML events (plus neutralize dangerous links). SVG uses <img data:>
// (never innerHTML). JSON/Mermaid are plain.
// ---------------------------------------------------------------------------

/// Whether a link target uses a scheme that can execute script when followed.
fn is_dangerous_url(url: &str) -> bool {
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    cleaned.starts_with("javascript:") || cleaned.starts_with("vbscript:") || cleaned.starts_with("data:")
}

/// Renders Markdown to HTML. We feed the original source (so code with <>&
/// renders correctly) but neutralize any raw HTML blocks from the clipboard
/// (the only path using inner_html) and rewrite dangerous links.
fn render_markdown(md: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag, Options, html};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(md, options).map(|event| match event {
        // Drop or neutralize raw HTML to avoid injection (shows as text if we pass through)
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) if is_dangerous_url(&dest_url) => {
            Event::Start(Tag::Link { link_type, dest_url: "#".into(), title, id })
        }
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Pretty-prints JSON; `None` if it doesn't parse (caller falls back to raw).
fn pretty_json(s: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(s.trim()).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// Cheap truncation for list preview of long Markdown (parse only first few lines).
fn md_preview_src(s: &str) -> String {
    let mut out = String::new();
    for (i, line) in s.lines().take(4).enumerate() {
        if i > 0 { out.push('\n'); }
        out.push_str(line);
    }
    if s.len() > 180 {
        out.push_str("…");
    }
    out
}

/// Builds a `data:image/svg+xml` URL so SVG can be previewed inside an `<img>`
/// (no script execution, no access to the Tauri bridge).
fn svg_data_url(svg: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
    format!("data:image/svg+xml;base64,{b64}")
}

/// Strips a ```` ```mermaid ```` … ```` ``` ```` fence if present, leaving the
/// bare diagram source.
fn extract_mermaid_code(raw: &str) -> String {
    let trimmed = raw.trim();
    let body = trimmed.strip_prefix("```mermaid").or_else(|| trimmed.strip_prefix("```"));
    match body {
        Some(rest) => {
            let rest = rest.trim_start_matches(['\r', '\n']);
            match rest.rfind("```") {
                Some(end) => rest[..end].trim().to_string(),
                None => rest.trim().to_string(),
            }
        }
        None => trimmed.to_string(),
    }
}

#[component]
pub fn App() -> impl IntoView {
    let (items, set_items) = signal(Vec::<UIClipboardItem>::new());
    let (persist, set_persist) = signal(String::from("none"));
    let (plugins, set_plugins) = signal(Vec::<PluginDef>::new());
    let (ttl, set_ttl) = signal(None::<u64>);
    let (detail, set_detail) = signal(None::<UIClipboardItem>);
    let (export, set_export) = signal(None::<ExportResult>);
    let (sel_plugin, set_sel_plugin) = signal(String::new());
    // Raw vs. rendered preview inside the maximize modal (reset on each open).
    let (view_raw, set_view_raw) = signal(false);
    // Search query (client-side filter on display_content for live list).
    let (search, set_search) = signal(String::new());
    // Id of the item whose title is being edited inline, if any.
    let (editing, set_editing) = signal(None::<String>);
    // Id of a sensitive item whose vault button is armed and awaiting the
    // confirming second click. Putting a secret on disk shouldn't be one stray
    // click away from the pin next to it.
    let (arming, set_arming) = signal(None::<String>);

    // Initial load (runs once at mount).
    spawn_local(async move {
        set_items.set(bindings::get_history().await);
        set_persist.set(bindings::get_persist_level().await);
        set_plugins.set(bindings::list_plugins().await);
        set_ttl.set(bindings::get_sensitive_ttl().await);
    });

    // Reload the list from the backend (session recent + DB). Used when the
    // window is shown again and as a fallback when live events are flaky.
    let reload_list = move || {
        let set = set_items;
        let q = search.get_untracked();
        spawn_local(async move {
            let fresh = if q.trim().is_empty() {
                bindings::get_history().await
            } else {
                bindings::search_history(&q).await
            };
            set.set(fresh);
        });
    };

    // Backend asks for a full re-fetch when the window is opened from tray /
    // shortcut (the Leptos tree is not remounted on hide/show).
    {
        let reload_list = reload_list;
        bindings::listen_event("history-refresh", move |_evt| {
            reload_list();
        });
    }

    // The tray's "Buscar…" entry: a native menu can't hold a text field, so the
    // backend opens the window and asks us to put the cursor in the search box.
    bindings::listen_event("focus-search", move |_evt| {
        if let Some(el) = document()
            .get_element_by_id("search-input")
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            let _ = el.focus();
            el.select();
        }
    });

    // Live capture: prepend the emitted item (works for non-persisted live
    // items too). Update is queued via spawn_local so it runs inside the
    // task executor (avoids "outside Leptos runtime" reactivity issues).
    // If a search is active we re-execute search so a matching new item
    // (by raw content) appears in the filtered results.
    // Fallback: if the payload can't be decoded, re-fetch the full list.
    bindings::listen_event("clipboard-new", move |evt| {
        let set = set_items;
        let q = search.get_untracked();
        let payload = js_sys::Reflect::get(&evt, &JsValue::from_str("payload")).ok();
        let parsed = payload.and_then(|p| {
            serde_wasm_bindgen::from_value::<UIClipboardItem>(p).ok()
        });
        spawn_local(async move {
            if q.trim().is_empty() {
                if let Some(item) = parsed {
                    set.update(|v| {
                        v.retain(|x| x.id != item.id);
                        v.insert(0, item);
                    });
                } else {
                    // Missed/undecodable payload → authoritative reload.
                    set.set(bindings::get_history().await);
                }
            } else {
                let fresh = bindings::search_history(&q).await;
                set.set(fresh);
            }
        });
    });

    let on_persist = move |ev| {
        let level = event_target_value(&ev);
        set_persist.set(level.clone());
        let q = search.get();
        spawn_local(async move {
            let _ = bindings::set_persist_level(&level).await;
            let res = if q.trim().is_empty() {
                bindings::get_history().await
            } else {
                bindings::search_history(&q).await
            };
            set_items.set(res);
        });
    };

    let on_ttl = move |ev| {
        let v = event_target_value(&ev);
        let secs = if v == "off" { None } else { v.parse::<u64>().ok() };
        set_ttl.set(secs);
        spawn_local(async move {
            let _ = bindings::set_sensitive_ttl(secs).await;
        });
    };

    let on_search = move |ev| {
        let val = event_target_value(&ev);
        set_search.set(val.clone());
        spawn_local(async move {
            let res = if val.trim().is_empty() {
                bindings::get_history().await
            } else {
                bindings::search_history(&val).await
            };
            set_items.set(res);
        });
    };

    let on_clear = move |_| {
        spawn_local(async move {
            let _ = bindings::clear_history().await;
            set_items.set(Vec::new());
        });
    };

    // Reflects the active TTL onto the preset <select>.
    let ttl_value = move || match ttl.get() {
        None => "off".to_string(),
        Some(s) => s.to_string(),
    };

    view! {
        <header>"Lapacho " <small>"· secure clipboard"</small></header>

        <div id="controls">
            <label>
                "Persistence: "
                // The vault is a per-item override of this setting, so the
                // labels must not promise more than the mode now delivers.
                <select prop:value=move || persist.get() on:change=on_persist
                        title="La bóveda (💾) persiste un item puntual aunque el modo lo prohíba">
                    <option value="none">"Paranoia (salvo bóveda)"</option>
                    <option value="sensitive">"Balanced (salvo bóveda)"</option>
                    <option value="all">"All"</option>
                </select>
            </label>
            <label>
                "Sensitive TTL: "
                <select prop:value=ttl_value on:change=on_ttl>
                    <option value="1800">"30 min"</option>
                    <option value="7200">"2 h"</option>
                    <option value="28800">"8 h"</option>
                    <option value="off">"No limit"</option>
                </select>
            </label>
            <label>
                "Search: "
                <input
                    id="search-input"
                    type="text"
                    placeholder="filter history..."
                    prop:value=move || search.get()
                    on:input=on_search
                />
            </label>
            <span class="spacer"></span>
            <button on:click=on_clear>"Clear history"</button>
        </div>

        <ul>
            {move || {
                items
                    .get()
                    .into_iter()
                    .map(|it| {
                        let id_copy = it.id.clone();
                        let id_del = it.id.clone();
                        let id_pin = it.id.clone();
                        let id_tag = it.id.clone();
                        let id_save = it.id.clone();
                        let id_vault = it.id.clone();
                        let is_editing = editing.get().as_deref() == Some(it.id.as_str());
                        let is_armed = arming.get().as_deref() == Some(it.id.as_str());
                        let title_now = it.title.clone();
                        let pinned = it.pinned;
                        let vaulted = it.vaulted;
                        let item_max = it.clone();
                        let sens = it.sensitivity.is_sensitive();
                        let li_class = if sens { "item sens" } else { "item" };
                        let tag_class = format!("tag s-{}", it.sensitivity.label());
                        // Images show a thumbnail; Markdown gets a mini rich preview (clipped by li height);
                        // everything else plain text. This makes MD render visible in the live list
                        // (RustyBoard behavior) while keeping things fast.
                        let dc = it.display_content.clone();
                        let img_label = if it.content_type == "image" {
                            match it.size {
                                Some(s) if s > 0 => format!("Image ({} bytes)", s),
                                _ => "Image".to_string(),
                            }
                        } else {
                            it.detected_type.label().to_string()
                        };
                        let content_node = if it.content_type == "image" {
                            // Use full display PNG for now (CSS .thumb downsizes it).
                            // thumbnail (RGBA) is available in it.thumbnail for future small preview conversion.
                            view! { <img class="thumb" src=dc alt="image" /> }.into_any()
                        } else if it.detected_type == DetectedType::Markdown && !sens {
                            let preview_src = md_preview_src(&dc);
                            let html = render_markdown(&preview_src);
                            view! { <div class="md-mini" inner_html=html></div> }.into_any()
                        } else if it.detected_type == DetectedType::Svg && !sens {
                            // Render SVG as safe <img> thumbnail in the list too (like modal).
                            let url = svg_data_url(&dc);
                            view! { <img class="thumb" src=url alt="svg" /> }.into_any()
                        } else if it.detected_type == DetectedType::Json && !sens {
                            let pretty = pretty_json(&dc).unwrap_or_else(|| dc.clone());
                            view! { <pre class="code-mini">{pretty}</pre> }.into_any()
                        } else {
                            view! { <span>{dc}</span> }.into_any()
                        };
                        view! {
                            <li class=li_class>
                                <div class="body">
                                    {if is_editing {
                                        view! {
                                            <input
                                                class="title-edit"
                                                placeholder="Nombre del item…"
                                                autofocus
                                                value=title_now.clone().unwrap_or_default()
                                                on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                    if ev.key() == "Escape" { set_editing.set(None); return; }
                                                    if ev.key() != "Enter" { return; }
                                                    let id = id_save.clone();
                                                    let title = event_target_value(&ev);
                                                    set_editing.set(None);
                                                    spawn_local(async move {
                                                        let _ = bindings::set_item_title(&id, &title).await;
                                                    });
                                                }
                                            />
                                        }.into_any()
                                    } else {
                                        title_now
                                            .filter(|t: &String| !t.is_empty())
                                            .map(|t| view! { <div class="item-title">{t}</div> })
                                            .into_any()
                                    }}
                                    <div class="content">{content_node}</div>
                                    <div class="meta">
                                        {vaulted.then(|| view! { <span class="pin-flag" title="En bóveda">"🗄"</span> })}
                                        {pinned.then(|| view! { <span class="pin-flag" title="Persistente">"📌"</span> })}
                                        <span class=tag_class>{it.sensitivity.label()}</span>
                                        " · "
                                        {img_label}
                                        " · " {time_ago(it.timestamp)}
                                    </div>
                                </div>
                                <div class="row-actions">
                                    <button
                                        title="Copy"
                                        on:click=move |_| {
                                            let id = id_copy.clone();
                                            spawn_local(async move {
                                                let _ = bindings::copy_item(&id).await;
                                            });
                                        }
                                    >"⧉"</button>
                                    <button
                                        title="Maximize"
                                        on:click=move |_| {
                                            set_export.set(None);
                                            set_view_raw.set(false);
                                            set_detail.set(Some(item_max.clone()));
                                        }
                                    >"⤢"</button>
                                    {(!sens && it.content_type != "image").then(|| {
                                        let id_secret = it.id.clone();
                                        view! {
                                            <button
                                                title="Es un secreto: enmascarar y recordar"
                                                on:click=move |_| {
                                                    let id = id_secret.clone();
                                                    spawn_local(async move {
                                                        let _ = bindings::mark_secret(&id).await;
                                                    });
                                                }
                                            >"🔒"</button>
                                        }
                                    })}
                                    <button
                                        title="Ponerle un nombre (Enter guarda, Esc cancela)"
                                        on:click=move |_| set_editing.set(Some(id_tag.clone()))
                                    >"🏷"</button>
                                    <button
                                        title=if pinned {
                                            "Persistente: no lo borra el límite de historial. Click para soltarlo"
                                        } else {
                                            "Hacerlo persistente (no aplica a secretos: siguen expirando por TTL)"
                                        }
                                        class=if pinned { "pinned" } else { "" }
                                        on:click=move |_| {
                                            let id = id_pin.clone();
                                            spawn_local(async move {
                                                let _ = bindings::toggle_pin(&id).await;
                                            });
                                        }
                                    >{if pinned { "📌" } else { "📍" }}</button>
                                    <button
                                        class=if vaulted { "vaulted" } else if is_armed { "arming" } else { "" }
                                        title=if vaulted {
                                            "En bóveda: guardado en disco aunque el modo lo prohíba. Click para sacarlo (se borra ya si el modo no lo permite)"
                                        } else if is_armed {
                                            "Confirmá: esto queda escrito en disco (cifrado) aunque estés en Paranoia"
                                        } else {
                                            "Guardar en bóveda: persiste aunque el modo lo prohíba"
                                        }
                                        on:click=move |_| {
                                            let id = id_vault.clone();
                                            // Arm sensitive items first; anything else (and any
                                            // un-vaulting) goes through on the first click.
                                            if sens && !vaulted && !is_armed {
                                                set_arming.set(Some(id));
                                                return;
                                            }
                                            set_arming.set(None);
                                            spawn_local(async move {
                                                let _ = bindings::toggle_vault(&id).await;
                                            });
                                        }
                                    >{if vaulted { "🗄" } else if is_armed { "⚠" } else { "💾" }}</button>
                                    <button
                                        title="Delete"
                                        on:click=move |_| {
                                            let id = id_del.clone();
                                            spawn_local(async move {
                                                let _ = bindings::delete_item(&id).await;
                                                set_items.update(move |v| v.retain(|x| x.id != id));
                                            });
                                        }
                                    >"✕"</button>
                                </div>
                            </li>
                        }
                    })
                    .collect_view()
            }}
        </ul>

        <div id="empty" style:display=move || if items.get().is_empty() { "block" } else { "none" }>
            "Copy something to see it appear here…"
        </div>

        // ---- detail / export modal ----
        {move || {
            detail
                .get()
                .map(|item| {
                    let id_copy = item.id.clone();
                    let id_export = item.id.clone();
                    let id_plugin = item.id.clone();
                    let id_mermaid = item.id.clone();
                    let tag_class = format!("tag s-{}", item.sensitivity.label());
                    // Per-type rendering inputs. Sensitive items carry only the
                    // redacted placeholder, so they're never "rich".
                    let dc = item.display_content.clone();
                    let dt = item.detected_type;
                    let is_image = item.content_type == "image";
                    let sens = item.sensitivity.is_sensitive();
                    let is_rich = !sens
                        && !is_image
                        && matches!(
                            dt,
                            DetectedType::Svg
                                | DetectedType::Markdown
                                | DetectedType::Json
                                | DetectedType::Mermaid
                        );
                    view! {
                        <div class="overlay" on:click=move |_| set_detail.set(None)>
                            <div class="modal" on:click=move |ev| ev.stop_propagation()>
                                <h2>
                                    <span class=tag_class>{item.sensitivity.label()}</span>
                                    {if is_image { "Image" } else { item.detected_type.label() }}
                                    <button class="close" on:click=move |_| set_detail.set(None)>
                                        "✕"
                                    </button>
                                </h2>
                                <div class="body-render">
                                    {
                                        let dc = dc.clone();
                                        let id_mermaid = id_mermaid.clone();
                                        move || {
                                            // Images render as <img>; the data-URL is never shown as text.
                                            if is_image {
                                                return view! {
                                                    <div class="image-view">
                                                        <img src=dc.clone() alt="imagen" />
                                                    </div>
                                                }
                                                .into_any();
                                            }
                                            // Raw view or sensitive (redacted) → plain text box.
                                            if sens || view_raw.get() {
                                                return view! { <div class="full">{dc.clone()}</div> }
                                                    .into_any();
                                            }
                                            match dt {
                                                DetectedType::Svg => view! {
                                                    <div class="svg-preview">
                                                        <img src=svg_data_url(&dc) alt="SVG" />
                                                    </div>
                                                }
                                                .into_any(),
                                                DetectedType::Markdown => {
                                                    let html = render_markdown(&dc);
                                                    view! { <div class="markdown-body" inner_html=html></div> }
                                                        .into_any()
                                                }
                                                DetectedType::Json => {
                                                    let pretty =
                                                        pretty_json(&dc).unwrap_or_else(|| dc.clone());
                                                    view! { <pre class="code-pretty">{pretty}</pre> }.into_any()
                                                }
                                                DetectedType::Mermaid => {
                                                    // Live render: drop the bare diagram source into a
                                                    // container, then ask mermaid.js (index.html) to
                                                    // replace it with SVG after the node is mounted. If
                                                    // the bundle is missing, the source stays visible.
                                                    let code = extract_mermaid_code(&dc);
                                                    let cid = format!("mermaid-{id_mermaid}");
                                                    let cid_raf = cid.clone();
                                                    let code_raf = code.clone();
                                                    request_animation_frame(move || {
                                                        bindings::render_mermaid(&cid_raf, &code_raf)
                                                    });
                                                    view! {
                                                        <div class="mermaid-wrap">
                                                            <div id=cid class="mermaid">{code}</div>
                                                        </div>
                                                    }
                                                    .into_any()
                                                }
                                                _ => view! { <div class="full">{dc.clone()}</div> }.into_any(),
                                            }
                                        }
                                    }
                                </div>
                                <div class="actions">
                                    {move || {
                                        if is_rich {
                                            view! {
                                                <button on:click=move |_| set_view_raw.update(|r| *r = !*r)>
                                                    {move || if view_raw.get() { "👁 View" } else { "📝 Raw" }}
                                                </button>
                                            }
                                            .into_any()
                                        } else {
                                            ().into_any()
                                        }
                                    }}
                                    <button
                                        class="primary"
                                        on:click=move |_| {
                                            let id = id_copy.clone();
                                            spawn_local(async move {
                                                let _ = bindings::copy_item(&id).await;
                                            });
                                        }
                                    >"Copy"</button>
                                    <button on:click=move |_| {
                                        let id = id_export.clone();
                                        spawn_local(async move {
                                            if let Ok(res) = bindings::export_item(&id).await {
                                                set_export.set(Some(res));
                                            }
                                        });
                                    }>"Export"</button>
                                    <select on:change=move |ev| set_sel_plugin.set(event_target_value(&ev))>
                                        <option value="">"— plugin —"</option>
                                        {move || {
                                            plugins
                                                .get()
                                                .into_iter()
                                                .map(|p| view! { <option value=p.id.clone()>{p.name.clone()}</option> })
                                                .collect_view()
                                        }}
                                    </select>
                                    <button on:click=move |_| {
                                        let pid = sel_plugin.get();
                                        if pid.is_empty() {
                                            return;
                                        }
                                        let item_id = id_plugin.clone();
                                        spawn_local(async move {
                                            if let Ok(newit) = bindings::run_plugin(&pid, &item_id).await {
                                                set_items.update(|v| {
                                                    v.retain(|x| x.id != newit.id);
                                                    v.insert(0, newit);
                                                });
                                            }
                                        });
                                    }>"Run"</button>
                                </div>

                                {move || {
                                    export
                                        .get()
                                        .map(|res| {
                                            let threats = res.threats.clone();
                                            let findings = if threats.is_empty() {
                                                view! {
                                                    <div class="threats-ok">"✓ No threats detected."</div>
                                                }
                                                    .into_any()
                                            } else {
                                                view! {
                                                    <div class="threats">
                                                        {threats
                                                            .into_iter()
                                                            .map(|t| {
                                                                view! {
                                                                    <div class=format!("threat {}", t.severity.css())>
                                                                        <span class="ico">{t.severity.icon()}</span>
                                                                        <span>{t.message.clone()}</span>
                                                                    </div>
                                                                }
                                                            })
                                                            .collect_view()}
                                                    </div>
                                                }
                                                    .into_any()
                                            };
                                            view! {
                                                <div>
                                                    {findings}
                                                    <div class="hint">"Content to export (raw):"</div>
                                                    <div class="full">{res.content.clone()}</div>
                                                </div>
                                            }
                                        })
                                }}
                            </div>
                        </div>
                    }
                })
        }}
    }
}
