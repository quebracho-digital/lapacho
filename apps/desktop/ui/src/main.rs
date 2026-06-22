//! Lapacho desktop frontend (Leptos CSR, mounted by Trunk into `index.html`).

mod app;
mod bindings;
mod types;

use app::App;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App /> });
}
