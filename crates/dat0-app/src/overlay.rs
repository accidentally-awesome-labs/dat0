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

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Div, FocusHandle, InteractiveElement, IntoElement, KeyBinding,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Window, div,
};
use gpui_component::ActiveTheme as _;

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::theme::tokens::{
    Dat0Theme as _, Elevation, ElevationStyled as _, Sp, SpStyled as _, TextRole, TypoStyled as _,
};

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

/// What a modal must tell the shell about itself.
///
/// Implemented by every modal body, so `window.rs` can mount, trap and count
/// modals from ONE list. B1 kept three hand-maintained places in sync instead
/// (an `or` chain, a count, and the mount site), which meant a new modal was
/// styled by [`modal_host`] but silently NOT trapped unless all three were
/// edited — two of them invisible to the compiler.
pub trait ModalContent {
    /// Accessible name of the `Dialog` node [`modal_host`] paints.
    fn modal_title(&self, cx: &App) -> SharedString;

    /// The modal's focus stops in VISUAL order — the trap's only source of
    /// truth, since gpui's `tab_index` is global rather than sibling-scoped.
    fn modal_focus_order(&self, cx: &App) -> Vec<FocusHandle>;
}

/// Visual weight of a [`modal_button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalButton {
    /// The affirmative action — theme `primary` fill.
    Primary,
    /// The dismissive action — no fill, `foreground` text.
    Ghost,
}

/// A modal's action button: a dat0-owned focus stop, activated by Enter/Space
/// *and* by click, styled from A2/A3 tokens.
///
/// Hand-rolled rather than `gpui_component::Button` because a `Button` builds
/// its focus handle with `window.use_keyed_state`, which is keyed by the GLOBAL
/// element-id path (`gpui-0.2.2/src/window.rs:2578` → `with_global_id`): the
/// handle resolves differently when read from anywhere else, so it can never be
/// collected into the `Vec<FocusHandle>` [`modal_trap`] needs. `Button::render`
/// also calls `track_focus` on its own base AFTER any builder chain, so a
/// chained `.track_focus(&ours)` is simply overwritten.
///
/// `Ghost` sets NO background rather than a transparent one: the
/// transparent-black constructor is banned by `tests/style_lint.rs` — which,
/// note, also matches the banned name in PROSE, so it cannot be spelled with
/// its call parens even inside a doc comment (same failure class as the CI
/// skip marker quoted in a commit body).
pub fn modal_button(
    id: &'static str,
    label: SharedString,
    fh: &FocusHandle,
    variant: ModalButton,
    cx: &App,
    on_activate: impl Fn(&mut Window, &mut App) + 'static + Clone,
) -> Stateful<Div> {
    let theme = cx.theme();
    let ring = theme.d0().focus_ring;
    let fg = match variant {
        ModalButton::Primary => theme.primary_foreground,
        ModalButton::Ghost => theme.foreground,
    };
    let primary_bg = theme.primary;
    let radius = theme.radius;
    let keyed = on_activate.clone();
    div()
        .id(id)
        .px_sp(Sp::S12)
        .py_sp(Sp::S4)
        .rounded(radius)
        .text_role(TextRole::Body)
        .text_color(fg)
        .cursor_pointer()
        .when(matches!(variant, ModalButton::Primary), |d| {
            d.bg(primary_bg)
        })
        .focus_stop(id, fh, 0, ring, move |_ev, window, app| keyed(window, app))
        .a11y(id, AccessRole::Button, label.to_string())
        .child(label)
        .on_click(move |_ev, window, app| on_activate(window, app))
}

/// A non-modal floating surface: elevation card + `occlude`, positioned by the
/// caller. No scrim and no trap — these overlays stay usable alongside the
/// shell, unlike [`modal_host`].
///
/// `occlude` additionally stops a click on the overlay's own padding from
/// falling through to the grid underneath.
pub fn anchored_overlay(cx: &App) -> Div {
    div().elevation(Elevation::Overlay, cx.theme()).occlude()
}

#[cfg(test)]
mod tests {
    use super::next_index;

    /// The two button weights are distinct arms — the styling difference itself
    /// is exercised by the nav suites (a button that is not a focus stop fails
    /// the Tab-cycle tests) and by the owed human glance.
    #[test]
    fn modal_button_variants_are_distinct() {
        assert_ne!(
            std::mem::discriminant(&super::ModalButton::Primary),
            std::mem::discriminant(&super::ModalButton::Ghost)
        );
    }

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
