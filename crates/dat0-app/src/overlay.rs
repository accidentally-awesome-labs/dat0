//! Modal overlay host (UI redesign B1) — full-window scrim, centered elevation
//! card, and a hand-rolled Tab focus trap.
//!
//! ## Why the trap is built from actions, not `on_key_down`
//!
//! gpui dispatches action bindings BEFORE `on_key_down` listeners
//! (`gpui-0.2.2/src/window.rs:3833-3848`: the binding loop `return`s as soon as
//! one binding consumes, and only a fully-unconsumed keystroke reaches
//! `finish_dispatch_key_event` → `dispatch_key_down_up_event`). gpui-component's
//! `Root` binds `tab`/`shift-tab` as actions under key context "Root"
//! (`crates/ui/src/root.rs:21-22`) and consumes them, so no `on_key_down`
//! handler in dat0 ever sees a Tab keystroke — including the shell's own, which
//! already handles Escape and the arrow keys.
//!
//! Those upstream action TYPES are not nameable — gpui-component's `root`
//! module is private (`crates/ui/src/lib.rs:11` is `mod root;`, and only
//! `Root`/`WindowExt` are re-exported) — so dat0 declares its own and binds them
//! to the same keystrokes under a DEEPER key context. gpui's keymap sorts
//! matched bindings by context depth, deepest first
//! (`gpui-0.2.2/src/keymap.rs:165`), and `Window::context_stack` builds the
//! stack root-first, so `Dat0Modal` — mounted below `Root` — wins.
//!
//! `escape` reuses the EXISTING `gpui_component::input::Escape` action rather
//! than declaring a new one, so `NamePrompt`'s current `on_action(Escape)`
//! handler catches it unchanged. Upstream binds `escape` only under key context
//! "Input" (`crates/ui/src/input/state.rs:120`), which is why Escape used to do
//! nothing once focus left a modal's text field.
//!
//! ## Why the trap hangs off the SHELL ROOT, not the scrim
//!
//! gpui resolves a keystroke in two independent lookups:
//!
//! 1. keystroke → action, using the KEY-CONTEXT STACK
//!    (`Keymap::bindings_for_input`), built from the focused node's path;
//! 2. action → handler, using the DISPATCH PATH
//!    (`Window::dispatch_action_on_node`), walked from the focused node upward.
//!
//! Putting the context and the handlers on the scrim satisfies neither lookup
//! for focus that sits OUTSIDE the modal: the scrim is a SIBLING of the shell's
//! content, not an ancestor of it. Measured, not assumed — with the trap on the
//! scrim, focus staged onto a background hero button walked to the next hero
//! button on a real Tab, because the binding matched, dispatched to no handler,
//! left `propagate_event` true, and `Root`'s Tab binding won the fallthrough.
//!
//! So [`modal_trap`] is applied to the shell ROOT (an ancestor of everything,
//! including the scrim) and [`modal_host`] is purely visual. The one case no
//! element-scoped context can recover is focus set to NOTHING
//! (`window.blur()`), where the dispatch path is the window root alone.

use gpui::{
    AnyElement, App, FocusHandle, InteractiveElement, IntoElement, KeyBinding, ParentElement as _,
    SharedString, Styled as _, Window, div,
};
use gpui_component::ActiveTheme as _;

use crate::a11y::{A11yExt as _, AccessRole};
use crate::theme::tokens::{Elevation, ElevationStyled as _};

gpui::actions!(dat0_modal, [ModalTab, ModalTabPrev]);

/// Key context carried by the shell root while a modal is mounted. Every focus
/// stop in the window then sits below it, so the modal-scoped bindings outrank
/// `Root`'s.
pub const MODAL_CONTEXT: &str = "Dat0Modal";

/// Bind the modal-scoped keys.
///
/// MUST be called by production (`run_app`) **and** by every test binary's
/// `init_components` — the test harness calls only `gpui_component::init`, so a
/// prod-only binding is invisible to tests and a green suite can hide a dead
/// production key path (the carve-out #7 Escape-ladder lesson).
pub fn register_modal_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", ModalTab, Some(MODAL_CONTEXT)),
        KeyBinding::new("shift-tab", ModalTabPrev, Some(MODAL_CONTEXT)),
        KeyBinding::new("escape", gpui_component::input::Escape, Some(MODAL_CONTEXT)),
    ]);
}

/// Pure index arithmetic for the trap, extracted so it is unit-testable without
/// a `Window`. `cur == None` means focus is currently OUTSIDE the modal — the
/// next Tab pulls it back in rather than letting it wander.
fn next_index(len: usize, cur: Option<usize>, delta: isize) -> usize {
    match cur {
        Some(i) => (i as isize + delta).rem_euclid(len as isize) as usize,
        None if delta > 0 => 0,
        None => len - 1,
    }
}

/// Move focus one stop along `handles`, wrapping. Never propagates: this is a
/// trap, not a wrap-around convenience.
fn cycle(handles: &[FocusHandle], delta: isize, window: &mut Window, cx: &App) {
    if handles.is_empty() {
        return;
    }
    let cur = window
        .focused(cx)
        .and_then(|f| handles.iter().position(|h| *h == f));
    window.focus(&handles[next_index(handles.len(), cur, delta)]);
}

/// Install the Tab trap on `el`, which MUST be an ancestor of everything the
/// trap needs to recapture focus from — in practice the shell root. Applied
/// only while a modal is mounted; with no modal open this is never called, so
/// the key context is absent and normal Tab navigation is untouched.
///
/// `focus_order` is the modal's stops in VISUAL order and is the trap's only
/// source of truth — gpui's `tab_index` is global rather than sibling-scoped
/// (every dat0 `focus_stop` passes 0 and relies on paint order), so the cycle
/// cannot be expressed as tab-index ordering.
pub fn modal_trap<E: InteractiveElement>(el: E, focus_order: Vec<FocusHandle>) -> E {
    let forward = focus_order.clone();
    let backward = focus_order;
    el.key_context(MODAL_CONTEXT)
        .on_action(move |_: &ModalTab, window, app| cycle(&forward, 1, window, app))
        .on_action(move |_: &ModalTabPrev, window, app| cycle(&backward, -1, window, app))
}

/// Wrap `content` in a scrim + centered elevation card. Purely visual plus the
/// `Dialog` a11y node — the keyboard trap lives on the shell root, see
/// [`modal_trap`] and the module docs.
///
/// `a11y_id` must be `&'static str`: `a11y()` records into the click-id side-map
/// and chains `debug_selector`.
pub fn modal_host(
    a11y_id: &'static str,
    title: SharedString,
    content: AnyElement,
    cx: &App,
) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        // Inert scrim: `occlude` blocks the mouse from everything behind it, so
        // the obscured shell cannot be operated while a modal is up, but
        // clicking the scrim does NOT dismiss. All three prompts hold typed text
        // (a query name, an API key, a MotherDuck token) that a stray click must
        // not discard.
        .bg(cx.theme().overlay)
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .elevation(Elevation::Modal, cx.theme())
                .a11y(a11y_id, AccessRole::Dialog, title.to_string())
                .child(content),
        )
}

#[cfg(test)]
mod tests {
    use super::next_index;

    #[test]
    fn next_index_cycles_forward_with_wrap() {
        assert_eq!(next_index(3, Some(0), 1), 1);
        assert_eq!(next_index(3, Some(1), 1), 2);
        assert_eq!(next_index(3, Some(2), 1), 0, "last wraps to first");
    }

    #[test]
    fn next_index_cycles_backward_with_wrap() {
        assert_eq!(next_index(3, Some(2), -1), 1);
        assert_eq!(next_index(3, Some(0), -1), 2, "first wraps to last");
    }

    #[test]
    fn next_index_snaps_back_when_focus_is_outside() {
        assert_eq!(
            next_index(3, None, 1),
            0,
            "Tab from outside enters at the first stop"
        );
        assert_eq!(
            next_index(3, None, -1),
            2,
            "Shift-Tab from outside enters at the last stop"
        );
    }
}
