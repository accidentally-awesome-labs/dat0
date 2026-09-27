//! What only the page knows: where keyboard focus is, and where an element
//! sits on screen.
//!
//! Dioxus hands a component its element's handle on mount, but a roving
//! focus moves to a sibling that did not just mount, and a popover opened
//! from the keyboard has no pointer to hang off. Both ask the webview by
//! `data-a11y-id`, the handle every surface already carries.
//!
//! The headless harness has no document, so both are no-ops there: its focus
//! is its own model (`tests/support`), and it has no layout to measure.

use std::rc::Rc;

use dioxus::prelude::*;

/// Whether a renderer supplied a document. `document::eval` without one logs
/// an error per call.
pub fn has_document() -> bool {
    try_consume_context::<Rc<dyn dioxus::document::Document>>().is_some()
}

/// Move keyboard focus to the element with this `data-a11y-id`.
pub fn focus(a11y_id: &str) {
    if !has_document() {
        return;
    }
    let _ = document::eval(&format!(
        r#"document.querySelector('[data-a11y-id="{a11y_id}"]')?.focus();"#
    ));
}

/// Where a popover opened for the element with this `data-a11y-id` hangs:
/// its horizontal middle, at its bottom edge, in client pixels — what a click
/// on it would have given. `None` with no document, or no such element.
pub async fn anchor_below(a11y_id: &str) -> Option<(f64, f64)> {
    if !has_document() {
        return None;
    }
    let found = document::eval(&format!(
        r#"const r = document.querySelector('[data-a11y-id="{a11y_id}"]')?.getBoundingClientRect();
return r ? [r.left + r.width / 2, r.bottom] : null;"#
    ))
    .await
    .ok()?;
    let [x, y]: [f64; 2] = serde_json::from_value(found).ok()?;
    Some((x, y))
}
