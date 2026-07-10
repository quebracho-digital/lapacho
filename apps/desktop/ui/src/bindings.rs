//! Typed wrappers over the Tauri command bridge.
//!
//! `withGlobalTauri` is on, so `window.__TAURI__.core.invoke` and
//! `window.__TAURI__.event.listen` exist. Each backend command gets a small
//! async helper here so the components never touch `JsValue` directly. Command
//! arguments are passed camelCase (Tauri 2's default mapping to snake_case Rust
//! params).

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::types::{ExportResult, PluginDef, UIClipboardItem};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], catch)]
    async fn listen(event: &str, handler: &js_sys::Function) -> Result<JsValue, JsValue>;

    // `window.renderMermaid(elementId, code)` is defined in index.html. It is a
    // no-op if the mermaid bundle failed to load, so calling it is always safe.
    #[wasm_bindgen(js_namespace = window, js_name = renderMermaid)]
    fn render_mermaid_raw(element_id: &str, code: &str);
}

/// Renders a Mermaid diagram into the DOM element `element_id`, replacing its
/// contents with the generated (mermaid-sanitized) SVG. If the bundle is
/// missing, the element keeps showing the diagram source as plain text.
pub fn render_mermaid(element_id: &str, code: &str) {
    render_mermaid_raw(element_id, code);
}

/// Turn a rejected-promise value into a readable error string.
fn js_err(v: JsValue) -> String {
    serde_wasm_bindgen::from_value::<String>(v.clone())
        .or_else(|_| v.as_string().ok_or(()))
        .unwrap_or_else(|_| "unknown error".to_string())
}

fn args(value: &impl Serialize) -> JsValue {
    serde_wasm_bindgen::to_value(value).unwrap_or(JsValue::NULL)
}

#[derive(Serialize)]
struct IdArgs<'a> {
    id: &'a str,
}

#[derive(Serialize)]
struct LevelArgs<'a> {
    level: &'a str,
}

#[derive(Serialize)]
struct TtlArgs {
    secs: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunPluginArgs<'a> {
    plugin_id: &'a str,
    item_id: &'a str,
}

// --- queries ---------------------------------------------------------------

pub async fn get_history() -> Vec<UIClipboardItem> {
    match invoke("get_history", JsValue::NULL).await {
        Ok(v) => serde_wasm_bindgen::from_value(v).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub async fn search_history(query: &str) -> Vec<UIClipboardItem> {
    #[derive(Serialize)]
    struct Q {
        query: String,
    }
    match invoke("search_history", args(&Q { query: query.to_string() })).await {
        Ok(v) => serde_wasm_bindgen::from_value(v).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub async fn get_persist_level() -> String {
    match invoke("get_persist_level", JsValue::NULL).await {
        Ok(v) => v.as_string().unwrap_or_else(|| "none".to_string()),
        Err(_) => "none".to_string(),
    }
}

pub async fn get_sensitive_ttl() -> Option<u64> {
    match invoke("get_sensitive_ttl", JsValue::NULL).await {
        Ok(v) => serde_wasm_bindgen::from_value(v).unwrap_or(None),
        Err(_) => None,
    }
}

pub async fn list_plugins() -> Vec<PluginDef> {
    match invoke("list_plugins", JsValue::NULL).await {
        Ok(v) => serde_wasm_bindgen::from_value(v).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

// --- mutations -------------------------------------------------------------

pub async fn copy_item(id: &str) -> Result<(), String> {
    invoke("copy_item", args(&IdArgs { id }))
        .await
        .map(|_| ())
        .map_err(js_err)
}

pub async fn delete_item(id: &str) -> Result<(), String> {
    invoke("delete_item", args(&IdArgs { id }))
        .await
        .map(|_| ())
        .map_err(js_err)
}

pub async fn clear_history() -> Result<(), String> {
    invoke("clear_history", JsValue::NULL)
        .await
        .map(|_| ())
        .map_err(js_err)
}

pub async fn set_persist_level(level: &str) -> Result<(), String> {
    invoke("set_persist_level", args(&LevelArgs { level }))
        .await
        .map(|_| ())
        .map_err(js_err)
}

pub async fn set_sensitive_ttl(secs: Option<u64>) -> Result<(), String> {
    invoke("set_sensitive_ttl", args(&TtlArgs { secs }))
        .await
        .map(|_| ())
        .map_err(js_err)
}

pub async fn export_item(id: &str) -> Result<ExportResult, String> {
    let v = invoke("export_item", args(&IdArgs { id }))
        .await
        .map_err(js_err)?;
    serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string())
}

pub async fn run_plugin(plugin_id: &str, item_id: &str) -> Result<UIClipboardItem, String> {
    let v = invoke("run_plugin", args(&RunPluginArgs { plugin_id, item_id }))
        .await
        .map_err(js_err)?;
    serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string())
}

// --- events ----------------------------------------------------------------

/// Subscribes to a Tauri event for the lifetime of the app. The handler closure
/// is leaked on purpose (it must outlive this call); the app never unsubscribes.
pub fn listen_event<F: FnMut(JsValue) + 'static>(event: &'static str, handler: F) {
    let cb = Closure::<dyn FnMut(JsValue)>::new(handler);
    let func: &js_sys::Function = cb.as_ref().unchecked_ref();
    let func = func.clone();
    wasm_bindgen_futures::spawn_local(async move {
        // Do not swallow errors: a silent listen failure freezes the live list.
        if let Err(e) = listen(event, &func).await {
            web_sys::console::error_2(
                &JsValue::from_str(&format!("lapacho: listen({event}) failed")),
                &e,
            );
            // One immediate retry after a microtask (withGlobalTauri race).
            let _ = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(
                &JsValue::UNDEFINED,
            ))
            .await;
            if let Err(e2) = listen(event, &func).await {
                web_sys::console::error_2(
                    &JsValue::from_str(&format!("lapacho: listen({event}) retry failed")),
                    &e2,
                );
            }
        }
    });
    cb.forget();
}
