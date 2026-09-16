//! A minimal Leptos component compiled to WebAssembly.
//!
//! The Next.js app loads the `wasm-pack --target web` output at runtime and calls
//! [`run_wasm`], which mounts the [`HelloWorld`] component into an existing `<div>`.

use leptos::mount::mount_to;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The text this module renders into the host element.
///
/// Kept as a plain function so it can be unit-tested with `cargo test` on the
/// host target, without needing a browser or `wasm-bindgen-test` runner.
pub fn greeting() -> &'static str {
    "Hello World!"
}

/// Runs once when the module is instantiated, before any exported function is
/// called. Routes Rust panics to the browser console instead of an opaque trap.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// The Leptos view that gets mounted into the host element.
#[component]
fn HelloWorld() -> impl IntoView {
    view! { <p class="wasm-hello">{greeting()}</p> }
}

/// Mount the [`HelloWorld`] component into the element with the given `id`.
///
/// Returns `Err` (which surfaces as a thrown JS `Error`) when no such element
/// exists or it is not an `HTMLElement`, so the caller can show a real message
/// rather than failing silently.
#[wasm_bindgen]
pub fn run_wasm(target_id: &str) -> Result<(), JsValue> {
    let document = web_sys::window()
        .ok_or_else(|| JsValue::from_str("no `window` available"))?
        .document()
        .ok_or_else(|| JsValue::from_str("no `document` available"))?;

    let target = document
        .get_element_by_id(target_id)
        .ok_or_else(|| JsValue::from_str(&format!("no element with id `{target_id}` found")))?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| JsValue::from_str(&format!("`#{target_id}` is not an HTMLElement")))?;

    // Clearing first keeps repeat clicks idempotent - without this every click
    // would append another copy of the view.
    target.set_inner_html("");

    // `mount_to` returns an `UnmountHandle` that unmounts the view when dropped,
    // so it has to be leaked for the view to stay on screen.
    mount_to(target, HelloWorld).forget();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::greeting;

    #[test]
    fn greeting_is_hello_world() {
        assert_eq!(greeting(), "Hello World!");
    }
}
