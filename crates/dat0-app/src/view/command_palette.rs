//! Command palette modal — the VIEW half (UI redesign B4).
//!
//! The MODEL — ranking, and which descriptors are fit to show — lives in
//! [`crate::command_palette`]; this file renders it. That split is what lets the
//! ranking be unit-tested with no `Window`, the same reason B1 extracted
//! `overlay::next_index`.
//!
//! ## Why the arrows are palette-scoped ACTIONS
//!
//! `up`/`down` are bound to dat0's own `PaletteUp`/`PaletteDown` under key
//! context [`PALETTE_CONTEXT`](crate::command_palette::PALETTE_CONTEXT), and
//! this file handles them with a plain `on_action`. Both halves were measured by
//! the B4 T0 gate; neither is obvious:
//!
//! - **With focus in the query field**, two bindings match: upstream's
//!   `MoveDown` under context "Input" (deeper, so gpui chooses it first —
//!   `keymap.rs:165`) and ours. `MoveDown` finds NO handler, because
//!   `Input::render` registers `InputState::up`/`down` only
//!   `.when(state.mode.is_multi_line(), …)` (`input/input.rs:309-311`) and this
//!   field is single-line. An unhandled action leaves `propagate_event` true, so
//!   the next-best binding — ours — wins.
//! - **With focus on the results list**, the "Input" context is not in the stack
//!   at all, so `down` produces `PaletteDown` directly.
//!
//! An earlier draft of the design intercepted upstream's `MoveDown` with
//! `capture_action` instead. That works from the field and is DEAD on the list,
//! where no `MoveDown` is ever produced — the kind of hole every test written
//! against the first stop would have missed.
//!
//! ⚠ The in-field half rests on that single-line registration guard. If this
//! field ever became multi-line, `MoveDown` would find a handler, consume, and
//! arrows would die in the field while still working on the list.
//! `arrows_move_the_active_row_and_clamp_at_both_ends` drives a real keystroke
//! with the field focused, so that regression fails a test rather than shipping.
//!
//! Enter and Escape need none of this: `enter()` emits `InputEvent::PressEnter`
//! and `escape()` propagates on a single-line field, exactly as `NamePrompt`
//! already relies on.
//!
//! ## Virtualised rows
//!
//! The results are a `uniform_list`, so only the visible window is built — the
//! T0 gate measured 6 of 30 rows reaching the a11y capture tree in a 120 px box.
//! Assertions must therefore target rows the list actually renders; arrowing to
//! a row scrolls it in and only then is it present.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable as _, ParentElement, ScrollStrategy,
    SharedString, Styled, Subscription, UniformListScrollHandle, Window, div, uniform_list,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{Escape, Input, InputEvent, InputState};

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};
use crate::command_palette::{PALETTE_CONTEXT, PaletteDown, PaletteUp, visible_items};
use crate::theme::tokens::{Dat0Theme as _, Sp, SpStyled as _, TextRole, TypoStyled as _};

/// What the palette asks the shell to do. The shell owns every dispatch.
#[derive(Debug, Clone)]
pub enum CommandPaletteEvent {
    /// Run this action, then dismiss. The shell dismisses FIRST — a routed
    /// action may open its own modal, and two mounted modals trip the
    /// single-modal `debug_assert!`.
    Run(ActionId),
    /// Dismiss without running anything.
    Cancel,
}

pub struct CommandPalette {
    /// The registry, by value. Held rather than read from
    /// `window_registry::action_registry()` so a test can hand this entity a
    /// small probe registry with no `OnceCell` install (T0 gate G4).
    reg: ActionRegistry,
    input: Entity<InputState>,
    /// Ranked, visibility-filtered snapshot for the CURRENT query. Rebuilt on
    /// change, never per-frame: `ActionRegistry::iter` clones every descriptor
    /// (an `Arc` per dispatch closure), and a render-time rebuild would pay that
    /// on every frame.
    items: Vec<ActionDescriptor>,
    /// Keyboard-selected row. Clamped, never wrapped — list surfaces clamp,
    /// only radio groups wrap (`empty_state.rs`).
    active: usize,
    /// The results list is ONE tab stop; arrows move `active` within it. This is
    /// the LISTBOX pattern (`saved_query_picker.rs` is the worked example) —
    /// never a focus handle per row.
    list_focus: FocusHandle,
    close_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _change_sub: Subscription,
    _enter_sub: Subscription,
}

impl CommandPalette {
    pub fn new(reg: ActionRegistry, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder(dat0_i18n::t("palette.placeholder")));
        let change_sub = cx.subscribe(&input, |this, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let q = input.read(cx).value().to_string();
                this.items = visible_items(&this.reg, &q);
                // Reset rather than clamp: after a narrowing keystroke, row 2 of
                // the OLD list is a different command than row 2 of the new one,
                // and Enter would run it.
                this.active = 0;
                cx.notify();
            }
        });
        let enter_sub = cx.subscribe(&input, |this, _input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.run_active(cx);
            }
        });
        let items = visible_items(&reg, "");
        Self {
            reg,
            input,
            items,
            active: 0,
            list_focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            _change_sub: change_sub,
            _enter_sub: enter_sub,
        }
    }

    /// The query field's stop — the modal's FIRST stop, and the one the open
    /// path focuses so a user can type immediately.
    pub fn input_focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    fn run_active(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.items.get(self.active) {
            cx.emit(CommandPaletteEvent::Run(d.id.clone()));
        }
    }

    /// Move the selection by `delta`, clamped, and keep the active row on
    /// screen — without `scroll_to_item` the ring walks off the fold and a
    /// keyboard user loses track of it on a keyboard-first surface.
    fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() - 1;
        self.active = (self.active as isize + delta).clamp(0, last as isize) as usize;
        self.scroll.scroll_to_item(self.active, ScrollStrategy::Top);
        cx.notify();
    }

    fn group_label(group: ActionGroup) -> SharedString {
        let key = match group {
            ActionGroup::Navigation => "palette.group.navigation",
            ActionGroup::Theme => "palette.group.theme",
            ActionGroup::File => "palette.group.file",
            ActionGroup::Settings => "palette.group.settings",
            ActionGroup::Recovery => "palette.group.recovery",
            ActionGroup::Import => "palette.group.import",
            ActionGroup::Edit => "palette.group.edit",
        };
        dat0_i18n::t(key).into()
    }
}

#[cfg(feature = "a11y-capture")]
impl CommandPalette {
    pub fn active_for_test(&self) -> usize {
        self.active
    }
    pub fn item_count_for_test(&self) -> usize {
        self.items.len()
    }
    /// The results list's stop, so a test can move focus to the palette's SECOND
    /// stop — where the "Input" key context is absent and any arrow mechanism
    /// keyed on upstream's `MoveDown` would be dead.
    pub fn list_focus_handle_for_test(&self) -> FocusHandle {
        self.list_focus.clone()
    }
    /// Set the query without keystrokes. Goes through `set_value`, so the
    /// `Change` subscription — the code under test — still runs.
    pub fn seed_query_for_test(&self, q: &str, window: &mut Window, cx: &mut App) {
        let q = q.to_string();
        self.input.update(cx, |s, cx| s.set_value(q, window, cx));
    }
}

impl EventEmitter<CommandPaletteEvent> for CommandPalette {}

/// B2: the shell mounts, traps and counts every modal from one list keyed on
/// this trait. The order here IS the Tab cycle.
impl crate::overlay::ModalContent for CommandPalette {
    fn modal_title(&self, _cx: &App) -> SharedString {
        dat0_i18n::t("palette.title").into()
    }
    fn modal_focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        vec![
            self.input_focus_handle(cx),
            self.list_focus.clone(),
            self.close_focus.clone(),
        ]
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ring = cx.theme().d0().focus_ring;
        let muted = cx.theme().muted_foreground;
        let active = self.active;
        let items = self.items.clone();
        let count = items.len();

        let entity_close = cx.entity();
        let close_btn = crate::overlay::modal_button(
            "palette-close",
            dat0_i18n::t("common.close").into(),
            &self.close_focus,
            crate::overlay::ModalButton::Ghost,
            cx,
            move |_window, app| {
                entity_close.update(app, |_this, cx| cx.emit(CommandPaletteEvent::Cancel));
            },
        );

        // Enter/Space on the LIST runs the active row — `focus_stop` supplies
        // this half of the keyboard contract for a user who Tabbed off the field.
        let activate = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            this.run_active(cx);
        });

        let rows = uniform_list("palette-results", count, move |range, _window, _app| {
            range
                .map(|i| {
                    let d = &items[i];
                    let mut row = div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_sp(Sp::S8)
                        .px_sp(Sp::S8)
                        .py_sp(Sp::S4)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_sp(Sp::S8)
                                .child(SharedString::from(d.title.clone()))
                                .child(
                                    div()
                                        .text_role(TextRole::Caption)
                                        .text_color(muted)
                                        .child(Self::group_label(d.group)),
                                ),
                        )
                        .a11y_label(AccessRole::Button, d.title.clone());
                    if let Some(k) = &d.keybinding {
                        row = row.child(
                            div()
                                .text_role(TextRole::Caption)
                                .text_color(muted)
                                .child(SharedString::from(k.to_string())),
                        );
                    }
                    if i == active {
                        row = row.border_1().border_color(ring);
                    }
                    row
                })
                .collect::<Vec<_>>()
        })
        // The height must be on the LIST, not only on its wrapper: a
        // `uniform_list` derives its visible range from its OWN bounds, and with
        // no height it renders exactly one item (measured — the first draft of
        // `rows_render_their_titles_as_a11y_content` saw only row 0).
        .h_full()
        .track_scroll(self.scroll.clone());

        div()
            .flex()
            .flex_col()
            .min_w(gpui::px(520.))
            // This context is what makes the palette-scoped `up`/`down` bindings
            // match — see the module docs.
            .key_context(PALETTE_CONTEXT)
            .on_action(cx.listener(|this, _: &PaletteDown, _window, cx| {
                this.move_active(1, cx);
            }))
            .on_action(cx.listener(|this, _: &PaletteUp, _window, cx| {
                this.move_active(-1, cx);
            }))
            // Escape cancels from any stop. `overlay::register_modal_keys` binds
            // `escape` under the `Dat0Modal` context that `modal_trap` installs
            // on the shell root, and a single-line Input propagates its own
            // `escape()`, so this ancestor handler catches both.
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(CommandPaletteEvent::Cancel);
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_sp(Sp::S8)
                    .py_sp(Sp::S4)
                    .child(
                        div()
                            .text_role(TextRole::Title)
                            .child(dat0_i18n::t("palette.title")),
                    )
                    .child(close_btn),
            )
            .child(
                div()
                    .px_sp(Sp::S8)
                    .py_sp(Sp::S4)
                    .child(Input::new(&self.input)),
            )
            .child(
                div()
                    .h(gpui::px(320.))
                    .focus_stop("palette-results", &self.list_focus, 0, ring, activate)
                    .a11y(
                        "palette-results",
                        AccessRole::Button,
                        dat0_i18n::t("palette.title"),
                    )
                    .child(rows),
            )
            .when(count == 0, |d| {
                d.child(
                    div()
                        .px_sp(Sp::S8)
                        .py_sp(Sp::S4)
                        .text_color(muted)
                        .child(dat0_i18n::t("palette.no_results")),
                )
            })
    }
}
