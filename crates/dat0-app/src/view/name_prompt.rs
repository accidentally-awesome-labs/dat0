//! Reusable single-line name-prompt modal (P5b). Emits the entered name on
//! confirm, or Cancelled. Used by Save Query + Save as Table.
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable as _, ParentElement, SharedString,
    Styled, Subscription, Window, div,
};
use gpui_component::input::{Escape, Input, InputEvent, InputState};

#[derive(Debug, Clone)]
pub enum NamePromptEvent {
    Confirm(String),
    Cancel,
}

pub struct NamePrompt {
    title: SharedString,
    input: Entity<InputState>,
    /// Focus stops for the OK/Cancel buttons (SQL-input-nav slice) — so the
    /// modal is fully keyboard-operable, not just click-operable.
    ok_focus: FocusHandle,
    cancel_focus: FocusHandle,
    /// Keeps the `input`→`PressEnter` subscription alive for the prompt's life
    /// (Enter in the field submits, mirroring the OK button).
    _enter_sub: Subscription,
}

impl NamePrompt {
    pub fn new(
        title: impl Into<SharedString>,
        initial: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // `default_value` seeds the field eagerly at build time (no later
        // `set_value` + `&mut Window` juggling). Empty seed → behaves exactly as
        // before for the Save-query / Save-as-table flows.
        let initial = initial.into();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("name")
                .default_value(initial)
        });
        // Enter in the single-line field submits. `enter()` emits `PressEnter`
        // and `cx.propagate()`s; nothing consumed it before (the prompt was
        // mouse-only). Subscribing here fixes Enter-submit for ALL 5 call sites.
        let enter_sub = cx.subscribe(&input, |this, _input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                let v = this.value(cx);
                cx.emit(NamePromptEvent::Confirm(v));
            }
        });
        // Focus the field on open so a keyboard user can type immediately and
        // Tab/Escape work. `new` holds `&mut Window`; the pending focus lands on
        // the input when it next renders with `.track_focus`.
        window.focus(&input.read(cx).focus_handle(cx));
        Self {
            title: title.into(),
            input,
            ok_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            _enter_sub: enter_sub,
        }
    }

    fn value(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// The prompt's focus stops in VISUAL order — the source of truth for
    /// `overlay::modal_trap`'s Tab cycle (B1). A render change that reorders the
    /// buttons must update this; `prompt_focus_order_is_field_ok_cancel` in
    /// `tests/modal_trap_nav.rs` guards the head of the list.
    pub fn focus_order(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        vec![
            self.input.read(cx).focus_handle(cx),
            self.ok_focus.clone(),
            self.cancel_focus.clone(),
        ]
    }

    /// The prompt's title, used as the accessible name of the modal's `Dialog`
    /// node (B1).
    pub fn title(&self) -> SharedString {
        self.title.clone()
    }
}

#[cfg(feature = "a11y-capture")]
impl NamePrompt {
    /// Whether the prompt's text field currently holds focus (proves
    /// focus-on-open).
    pub fn input_focused_for_test(&self, window: &Window, cx: &gpui::App) -> bool {
        self.input.read(cx).focus_handle(cx).is_focused(window)
    }

    /// The field's `FocusHandle` — lets a test re-focus INTO the modal or assert
    /// the head of [`focus_order`](Self::focus_order).
    pub fn input_focus_handle_for_test(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    /// Seed the field's text without keystrokes (so a test can assert the
    /// submitted value round-trips through `Confirm`).
    pub fn seed_value_for_test(&self, value: &str, window: &mut Window, cx: &mut gpui::App) {
        let value = value.to_string();
        self.input
            .update(cx, |s, cx| s.set_value(value, window, cx));
    }
}

impl EventEmitter<NamePromptEvent> for NamePrompt {}

/// B2: the shell mounts, traps and counts every modal from one list, keyed on
/// this trait. The inherent [`title`](Self::title) /
/// [`focus_order`](Self::focus_order) stay as the implementation so B1's call
/// sites and tests are untouched.
impl crate::overlay::ModalContent for NamePrompt {
    fn modal_title(&self, _cx: &gpui::App) -> SharedString {
        self.title()
    }
    fn modal_focus_order(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        self.focus_order(cx)
    }
}

impl Render for NamePrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ok_fh = self.ok_focus.clone();
        let cancel_fh = self.cancel_focus.clone();
        // B2: both buttons come from the shared `overlay::modal_button`, so the
        // export dialog and this prompt cannot drift apart visually. Ids and
        // accessible names are unchanged — `tests/modal_trap_nav.rs` navigates
        // by them. `modal_button` supplies BOTH `focus_stop` and `a11y`, so do
        // not add a second `.a11y` here (both helpers PUSH a node).
        let entity_ok = cx.entity();
        let ok_btn = crate::overlay::modal_button(
            "name-prompt-ok",
            SharedString::from("Save"),
            &ok_fh,
            crate::overlay::ModalButton::Primary,
            cx,
            move |_window, app| {
                entity_ok.update(app, |this, cx| {
                    let v = this.value(cx);
                    cx.emit(NamePromptEvent::Confirm(v));
                });
            },
        );
        let entity_cancel = cx.entity();
        let cancel_btn = crate::overlay::modal_button(
            "name-prompt-cancel",
            SharedString::from("Cancel"),
            &cancel_fh,
            crate::overlay::ModalButton::Ghost,
            cx,
            move |_window, app| {
                entity_cancel.update(app, |_this, cx| cx.emit(NamePromptEvent::Cancel));
            },
        );
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            // Escape cancels. A single-line `escape()` is a no-op that
            // `cx.propagate()`s, so this ancestor `on_action` catches it.
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(NamePromptEvent::Cancel);
            }))
            .child(self.title.clone())
            .child(Input::new(&self.input))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(ok_btn)
                    .child(cancel_btn),
            )
    }
}
