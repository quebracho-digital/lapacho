//! Lapacho desktop frontend (Leptos CSR, mounted by Trunk into `index.html`).

mod app;
mod bindings;
mod spotlight;
mod types;

use app::App;
use leptos::prelude::*;
use spotlight::Spotlight;

fn main() {
    console_error_panic_hook::set_once();
    // Both windows load this same bundle; the label picks the face.
    if bindings::window_label() == "spotlight" {
        mount_to_body(|| view! { <Spotlight /> });
    } else {
        mount_to_body(|| view! { <App /> });
    }
}
