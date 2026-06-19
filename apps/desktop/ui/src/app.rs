//! The Lapacho clipboard UI.
//!
//! Ports the original vanilla popup (live list, copy/delete/clear, persistence
//! selector) to Leptos and adds the item-decoupling actions the backend now
//! supports: maximize (full view), export (raw content + threat assessment),
//! and run-a-plugin (operates on raw, result becomes a new item).

use leptos::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

use crate::bindings;
use crate::types::{ExportResult, PluginDef, UIClipboardItem};

/// Relative time label, e.g. "hace 3m".
fn time_ago(ts: u64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as i64;
    let s = (now - ts as i64).max(0);
    if s < 60 {
        format!("hace {s}s")
    } else if s < 3600 {
        format!("hace {}m", s / 60)
    } else {
        format!("hace {}h", s / 3600)
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

    // Initial load (runs once at mount).
    spawn_local(async move {
        set_items.set(bindings::get_history().await);
        set_persist.set(bindings::get_persist_level().await);
        set_plugins.set(bindings::list_plugins().await);
        set_ttl.set(bindings::get_sensitive_ttl().await);
    });

    // Live capture: prepend new items as the monitor emits them.
    bindings::listen_event("clipboard-new", move |evt| {
        if let Ok(payload) = js_sys::Reflect::get(&evt, &JsValue::from_str("payload")) {
            if let Ok(item) = serde_wasm_bindgen::from_value::<UIClipboardItem>(payload) {
                set_items.update(|v| {
                    v.retain(|x| x.id != item.id);
                    v.insert(0, item);
                });
            }
        }
    });

    let on_persist = move |ev| {
        let level = event_target_value(&ev);
        set_persist.set(level.clone());
        spawn_local(async move {
            let _ = bindings::set_persist_level(&level).await;
            set_items.set(bindings::get_history().await);
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
        <header>"Lapacho " <small>"· portapapeles seguro"</small></header>

        <div id="controls">
            <label>
                "Persistencia: "
                <select prop:value=move || persist.get() on:change=on_persist>
                    <option value="none">"Paranoia"</option>
                    <option value="sensitive">"Balanceado"</option>
                    <option value="all">"Todo"</option>
                </select>
            </label>
            <label>
                "Sensibles: "
                <select prop:value=ttl_value on:change=on_ttl>
                    <option value="1800">"30 min"</option>
                    <option value="7200">"2 h"</option>
                    <option value="28800">"8 h"</option>
                    <option value="off">"Sin límite"</option>
                </select>
            </label>
            <span class="spacer"></span>
            <button on:click=on_clear>"Limpiar historial"</button>
        </div>

        <ul>
            {move || {
                items
                    .get()
                    .into_iter()
                    .map(|it| {
                        let id_copy = it.id.clone();
                        let id_del = it.id.clone();
                        let item_max = it.clone();
                        let sens = it.sensitivity.is_sensitive();
                        let li_class = if sens { "item sens" } else { "item" };
                        let tag_class = format!("tag s-{}", it.sensitivity.label());
                        view! {
                            <li class=li_class>
                                <div class="body">
                                    <div class="content">{it.display_content.clone()}</div>
                                    <div class="meta">
                                        <span class=tag_class>{it.sensitivity.label()}</span>
                                        " · " {it.detected_type.label()} " · " {time_ago(it.timestamp)}
                                    </div>
                                </div>
                                <div class="row-actions">
                                    <button
                                        title="Copiar"
                                        on:click=move |_| {
                                            let id = id_copy.clone();
                                            spawn_local(async move {
                                                let _ = bindings::copy_item(&id).await;
                                            });
                                        }
                                    >"⧉"</button>
                                    <button
                                        title="Maximizar"
                                        on:click=move |_| {
                                            set_export.set(None);
                                            set_detail.set(Some(item_max.clone()));
                                        }
                                    >"⤢"</button>
                                    <button
                                        title="Borrar"
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
            "Copiá algo para verlo aparecer aquí…"
        </div>

        // ---- detail / export modal ----
        {move || {
            detail
                .get()
                .map(|item| {
                    let id_copy = item.id.clone();
                    let id_export = item.id.clone();
                    let id_plugin = item.id.clone();
                    let tag_class = format!("tag s-{}", item.sensitivity.label());
                    view! {
                        <div class="overlay" on:click=move |_| set_detail.set(None)>
                            <div class="modal" on:click=move |ev| ev.stop_propagation()>
                                <h2>
                                    <span class=tag_class>{item.sensitivity.label()}</span>
                                    {item.detected_type.label()}
                                    <button class="close" on:click=move |_| set_detail.set(None)>
                                        "✕"
                                    </button>
                                </h2>
                                <div class="full">{item.display_content.clone()}</div>
                                <div class="actions">
                                    <button
                                        class="primary"
                                        on:click=move |_| {
                                            let id = id_copy.clone();
                                            spawn_local(async move {
                                                let _ = bindings::copy_item(&id).await;
                                            });
                                        }
                                    >"Copiar"</button>
                                    <button on:click=move |_| {
                                        let id = id_export.clone();
                                        spawn_local(async move {
                                            if let Ok(res) = bindings::export_item(&id).await {
                                                set_export.set(Some(res));
                                            }
                                        });
                                    }>"Exportar"</button>
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
                                    }>"Ejecutar"</button>
                                </div>

                                {move || {
                                    export
                                        .get()
                                        .map(|res| {
                                            let threats = res.threats.clone();
                                            let findings = if threats.is_empty() {
                                                view! {
                                                    <div class="threats-ok">"✓ Sin amenazas detectadas."</div>
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
                                                    <div class="hint">"Contenido a exportar (raw):"</div>
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
