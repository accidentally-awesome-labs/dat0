//! Saved-query picker modal (UI redesign B2).
//!
//! Replaces the window-level `saved_picker_open` flag plus the free
//! `query_library::render_saved_picker`, which was mouse-only, untested, and
//! rendered as a transparent bordered box pinned to the top-right corner.
//!
//! This is the LISTBOX pattern — ONE container `focus_stop` plus an active
//! index, never per-row focus handles — proven by the recents list
//! (`empty_state.rs:448`) and the catalog tree. B4's command palette is the
//! same shape, so this is its precedent rather than its guess.
//!
//! The picker READS the session live (`saved_queries()` on every render), so a
//! delete routed through the shell shrinks the list on the next frame. It never
//! mutates: `WorkspaceShell` owns `delete_named_query`, and the picker only
//! emits [`SavedQueryPickerEvent`].

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FocusHandle, ParentElement, SharedString, Styled, Window, div};
use gpui_component::ActiveTheme as _;
use gpui_component::input::Escape;
use parking_lot::Mutex;

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::session::Session;
use crate::session::queries::SavedQuery;
use crate::theme::tokens::{Dat0Theme as _, Sp, SpStyled as _};

/// What the picker asks the shell to do. The shell owns every mutation.
#[derive(Debug, Clone)]
pub enum SavedQueryPickerEvent {
    /// Load this SQL into a new console tab.
    Pick(String),
    /// Delete the saved query with this id.
    Delete(uuid::Uuid),
    /// Dismiss without doing anything.
    Cancel,
}

pub struct SavedQueryPicker {
    /// Read-only handle on the session; the list is re-read every render.
    session: Arc<Mutex<Session>>,
    /// Keyboard-selected row, in display order. Clamped at render time — a
    /// delete can leave it past the end.
    active: usize,
    /// The list container is ONE tab stop; arrows move `active` within it.
    list_focus: FocusHandle,
    /// The close button.
    close_focus: FocusHandle,
}

impl SavedQueryPicker {
    pub fn new(session: Arc<Mutex<Session>>, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            active: 0,
            list_focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
        }
    }

    /// The list container's focus stop — the modal's FIRST stop, and the one
    /// the open path focuses.
    pub fn list_focus_handle(&self) -> FocusHandle {
        self.list_focus.clone()
    }

    /// The close button's focus stop.
    pub fn close_focus_handle(&self) -> FocusHandle {
        self.close_focus.clone()
    }

    /// Live read — a delete routed through the shell shrinks this next frame.
    fn rows(&self) -> Vec<SavedQuery> {
        self.session.lock().saved_queries().to_vec()
    }
}

#[cfg(feature = "a11y-capture")]
impl SavedQueryPicker {
    /// The keyboard-selected row index, so a test can assert what the arrows
    /// did without going through pixels.
    pub fn active_for_test(&self) -> usize {
        self.active
    }
}

impl EventEmitter<SavedQueryPickerEvent> for SavedQueryPicker {}

/// B2: the shell mounts, traps and counts every modal from one list keyed on
/// this trait. The order here IS the Tab cycle.
impl crate::overlay::ModalContent for SavedQueryPicker {
    fn modal_title(&self, _cx: &gpui::App) -> SharedString {
        dat0_i18n::t("sql.load_query").into()
    }
    fn modal_focus_order(&self, _cx: &gpui::App) -> Vec<FocusHandle> {
        vec![self.list_focus.clone(), self.close_focus.clone()]
    }
}

impl Render for SavedQueryPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows();
        let len = rows.len();
        // A delete can leave `active` past the end; clamp before rendering so
        // the ring lands on a row that exists.
        let active = self.active.min(len.saturating_sub(1));
        self.active = active;
        let ring = cx.theme().d0().focus_ring;

        // Arrows CLAMP, matching the recents list (`empty_state.rs:436-439`).
        // Only radio groups wrap. Delete/Backspace removes the active row: the
        // shell performs the mutation and re-notifies us.
        let arrows = cx.listener(move |this, ev: &gpui::KeyDownEvent, _window, cx| {
            match ev.keystroke.key.as_str() {
                "down" => this.active = (this.active + 1).min(len.saturating_sub(1)),
                "up" => this.active = this.active.saturating_sub(1),
                "delete" | "backspace" => {
                    if let Some(q) = this.rows().get(this.active) {
                        cx.emit(SavedQueryPickerEvent::Delete(q.id));
                    }
                }
                _ => return,
            }
            cx.notify();
        });

        // Enter/Space on the container loads the ACTIVE row — `focus_stop`
        // supplies this half of the keyboard contract.
        let activate = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            if let Some(q) = this.rows().get(this.active) {
                cx.emit(SavedQueryPickerEvent::Pick(q.sql.clone()));
            }
        });

        let entity_close = cx.entity();
        let close_btn = crate::overlay::modal_button(
            "sql-saved-close",
            dat0_i18n::t("common.close").into(),
            &self.close_focus,
            crate::overlay::ModalButton::Ghost,
            cx,
            move |_window, app| {
                entity_close.update(app, |_this, cx| cx.emit(SavedQueryPickerEvent::Cancel));
            },
        );

        let mut list = div()
            .flex()
            .flex_col()
            .gap_sp(Sp::S2)
            .p_sp(Sp::S8)
            .focus_stop("sql-saved-list", &self.list_focus, 0, ring, activate)
            .on_key_down(arrows)
            .a11y(
                "sql-saved-list",
                AccessRole::Button,
                dat0_i18n::t("sql.load_query"),
            );

        for (i, q) in rows.into_iter().enumerate() {
            let sql = q.sql.clone();
            let id = q.id;
            let entity_pick = cx.entity();
            let entity_del = cx.entity();
            let mut row = div()
                .id(("saved-row", i))
                .flex()
                .flex_row()
                .justify_between()
                .gap_sp(Sp::S8)
                .px_sp(Sp::S8)
                .py_sp(Sp::S4)
                .cursor_pointer()
                .child(SharedString::from(q.name))
                .child(
                    div()
                        .id(("saved-del", i))
                        .cursor_pointer()
                        .a11y_label(AccessRole::Label, dat0_i18n::t("common.close"))
                        .child(gpui_component::Icon::new(gpui_component::IconName::Close))
                        .on_click(move |_ev, _w, app| {
                            entity_del
                                .update(app, |_t, cx| cx.emit(SavedQueryPickerEvent::Delete(id)));
                        }),
                )
                .on_click(move |_ev, _w, app| {
                    entity_pick.update(app, |_t, cx| {
                        cx.emit(SavedQueryPickerEvent::Pick(sql.clone()))
                    });
                });
            if i == active {
                row = row.border_1().border_color(ring);
            }
            list = list.child(row);
        }

        div()
            .flex()
            .flex_col()
            .min_w(gpui::px(420.))
            .max_h(gpui::px(320.))
            .overflow_hidden()
            // Escape cancels from either stop. `overlay::register_modal_keys`
            // binds `escape` under the `Dat0Modal` context that `modal_trap`
            // installs on the shell root, so this ancestor handler catches it.
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(SavedQueryPickerEvent::Cancel);
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_sp(Sp::S8)
                    .py_sp(Sp::S4)
                    .child(dat0_i18n::t("sql.load_query"))
                    .child(close_btn),
            )
            .child(list)
    }
}
