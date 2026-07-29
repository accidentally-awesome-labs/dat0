//! Export dialog: format (CSV/JSON/Parquet) + scope (current view / full table),
//! native save panel, streaming COPY. The SELECT-build logic is pure + tested.
//!
//! # Split (mirrors the filter-popover T10/T10b pattern)
//!
//! [`build_export`] is pure logic with no GPUI import — it is the unit-tested
//! kernel (`tests/export_select_build.rs`), composed with
//! [`dat0_engine::render::render_export_select`] to produce the final
//! surrogate-stripped projection SELECT. [`ExportDialog`] is the GPUI [`Entity`]
//! that mounts the two radio groups + Export/Cancel buttons and emits
//! [`ExportEvent`]; `WorkspaceShell` subscribes to it and drives the native
//! save panel + engine COPY in `run_export`.

use dat0_engine::transform::ProjectionColumn;
use dat0_engine::types::ExportFormat;

use gpui::{
    Context, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement, ParentElement,
    Render, Styled, Window, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::Escape;
use gpui_component::{
    h_flex,
    label::Label,
    radio::{Radio, RadioGroup},
    v_flex,
};

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::theme::tokens::Dat0Theme as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportScope {
    CurrentView,
    FullTable,
}

/// Build (inner_sql, projection cols) for an export.
/// - `base_table`: already-quoted base relation (e.g. `"main"."orders"`).
/// - `active_view`: the active view's (already-quoted) name, or None at cursor 0.
/// - `column_view`: folded visible columns (source→display) for the current view.
/// - `base_columns`: source columns of the base table (surrogate excluded).
///
/// Current view → inner reads the active view (or base if none) and cols apply
/// the projection. Full table → inner reads base and cols are identity (raw).
pub fn build_export(
    scope: ExportScope,
    base_table: &str,
    active_view: Option<&str>,
    column_view: &[ProjectionColumn],
    base_columns: &[String],
) -> (String, Vec<ProjectionColumn>) {
    match scope {
        ExportScope::CurrentView => {
            let inner = match active_view {
                Some(v) => format!("SELECT * FROM {}", v),
                None => format!("SELECT * FROM {}", base_table),
            };
            (inner, column_view.to_vec())
        }
        ExportScope::FullTable => {
            let inner = format!("SELECT * FROM {}", base_table);
            let cols = base_columns
                .iter()
                .map(|s| ProjectionColumn {
                    source: s.clone(),
                    display: s.clone(),
                })
                .collect();
            (inner, cols)
        }
    }
}

// ---------------------------------------------------------------------------
// ExportDialog entity (format radio + scope radio + Export/Cancel)
// ---------------------------------------------------------------------------

/// Cycle an index within `len`, wrapping in both directions.
///
/// Radio groups WRAP (the WAI-ARIA radiogroup convention); the list surfaces
/// deliberately clamp instead (`empty_state.rs:436-439` uses `.min(len-1)` /
/// `saturating_sub`). A 2- or 3-item group that dead-ends is worse than one
/// that cycles.
fn cycle_ix(cur: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (cur as isize + delta).rem_euclid(len as isize) as usize
}

/// Event emitted by [`ExportDialog`] when the user presses Export or Cancel.
///
/// `WorkspaceShell` subscribes via `cx.subscribe` (the subscription is STORED
/// in `export_dialog_sub` — a dropped `Subscription` deregisters the callback
/// silently, the P4a T10b trap). On `Export` it drives
/// [`crate::window::WorkspaceShell::run_export`]; on `Cancel` it tears the
/// dialog down.
#[derive(Debug, Clone)]
pub enum ExportEvent {
    Export {
        scope: ExportScope,
        format: ExportFormat,
    },
    Cancel,
}

/// GPUI entity for the File → Export… dialog.
///
/// Holds the two radio selections (format + scope) as plain state, plus the
/// four focus handles the B2 modal trap cycles over. The handles must be
/// dat0-owned: gpui-component's `Button` and `Radio` build theirs with
/// `window.use_keyed_state`, which is keyed by the GLOBAL element-id path, so
/// they are unreachable from `WorkspaceShell::render` where the trap's
/// `Vec<FocusHandle>` is assembled.
///
/// Still needs no `&mut Window` at construction (the `RadioGroup` widget is
/// `RenderOnce`), only a `Context` for the handles:
/// `cx.new(|cx| ExportDialog::new(cx))`.
pub struct ExportDialog {
    /// 0 = CSV, 1 = JSON, 2 = Parquet (index into [`Self::FORMATS`]).
    format_ix: usize,
    /// 0 = Current view, 1 = Full table (index into [`Self::SCOPES`]).
    scope_ix: usize,
    /// The format radio GROUP is one tab stop; arrows move the selection
    /// within it (the WAI-ARIA radiogroup pattern).
    format_focus: FocusHandle,
    /// Ditto for the scope group.
    scope_focus: FocusHandle,
    /// The Export button.
    run_focus: FocusHandle,
    /// The Cancel button.
    cancel_focus: FocusHandle,
}

impl ExportDialog {
    const FORMATS: [ExportFormat; 3] =
        [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Parquet];
    const SCOPES: [ExportScope; 2] = [ExportScope::CurrentView, ExportScope::FullTable];

    /// Construct a fresh dialog defaulting to CSV + Current view.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            format_ix: 0,
            scope_ix: 0,
            format_focus: cx.focus_handle(),
            scope_focus: cx.focus_handle(),
            run_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
        }
    }

    /// The format group's focus stop — the modal's FIRST stop, and the one the
    /// render-drain focuses on open.
    pub fn format_focus_handle(&self) -> FocusHandle {
        self.format_focus.clone()
    }
    /// The scope group's focus stop.
    pub fn scope_focus_handle(&self) -> FocusHandle {
        self.scope_focus.clone()
    }
    /// The Export button's focus stop.
    pub fn run_focus_handle(&self) -> FocusHandle {
        self.run_focus.clone()
    }
    /// The Cancel button's focus stop.
    pub fn cancel_focus_handle(&self) -> FocusHandle {
        self.cancel_focus.clone()
    }

    /// The currently-selected export format.
    pub fn format(&self) -> ExportFormat {
        Self::FORMATS[self.format_ix]
    }

    /// The currently-selected export scope.
    pub fn scope(&self) -> ExportScope {
        Self::SCOPES[self.scope_ix]
    }
}

impl EventEmitter<ExportEvent> for ExportDialog {}

/// B2: the shell mounts, traps and counts every modal from one list keyed on
/// this trait. The order here IS the Tab cycle — a render change that reorders
/// the controls must update it; `export_modal_tab_cycles_four_stops` in
/// `tests/modal_b2_nav.rs` guards it.
impl crate::overlay::ModalContent for ExportDialog {
    fn modal_title(&self, _cx: &gpui::App) -> gpui::SharedString {
        dat0_i18n::t("export.title").into()
    }
    fn modal_focus_order(&self, _cx: &gpui::App) -> Vec<FocusHandle> {
        vec![
            self.format_focus.clone(),
            self.scope_focus.clone(),
            self.run_focus.clone(),
            self.cancel_focus.clone(),
        ]
    }
}

#[cfg(feature = "a11y-capture")]
impl ExportDialog {
    /// The selected format, so a keyboard test can assert what the arrows did.
    pub fn format_for_test(&self) -> ExportFormat {
        self.format()
    }
    /// The selected scope, likewise.
    pub fn scope_for_test(&self) -> ExportScope {
        self.scope()
    }
}

impl Render for ExportDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let format_ix = self.format_ix;
        let scope_ix = self.scope_ix;
        let ring = cx.theme().d0().focus_ring;

        // ── Format radio group (CSV / JSON / Parquet) ──────────────────────
        //
        // The children are explicit `Radio`s rather than bare strings so they
        // can carry `.tab_stop(false)`: the GROUP is the tab stop, not the
        // individual radios. `RadioGroup::render` overwrites each child's id
        // with its index but leaves `tab_stop` alone (gpui-component
        // `radio.rs:333`), so the ids here are cosmetic.
        let format_group = RadioGroup::horizontal("export-format")
            .children([
                Radio::new("csv")
                    .label(dat0_i18n::t("export.format.csv"))
                    .tab_stop(false),
                Radio::new("json")
                    .label(dat0_i18n::t("export.format.json"))
                    .tab_stop(false),
                Radio::new("parquet")
                    .label(dat0_i18n::t("export.format.parquet"))
                    .tab_stop(false),
            ])
            .selected_index(Some(format_ix))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.format_ix = *ix;
                cx.notify();
            }));

        // ONE tab stop for the whole group; Left/Right move the selection.
        // `focus_stop`'s Enter/Space activation is a deliberate no-op: on a
        // radiogroup the selection IS the state, and a second submit path from
        // inside a group would surprise. Chaining a second `on_key_down` after
        // `focus_stop` is the established shape (`empty_state.rs:451-452`).
        let format_stop = div()
            .focus_stop(
                "export-format-group",
                &self.format_focus,
                0,
                ring,
                |_ev, _window, _app| {},
            )
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                let delta = match ev.keystroke.key.as_str() {
                    "left" => -1,
                    "right" => 1,
                    _ => return,
                };
                this.format_ix = cycle_ix(this.format_ix, Self::FORMATS.len(), delta);
                cx.notify();
            }))
            .a11y(
                "export-format-group",
                AccessRole::Button,
                dat0_i18n::t("export.format"),
            )
            .child(format_group);

        // ── Scope radio group (Current view / Full table) ──────────────────
        let scope_group = RadioGroup::vertical("export-scope")
            .children([
                Radio::new("current")
                    .label(dat0_i18n::t("export.scope.current"))
                    .tab_stop(false),
                Radio::new("full")
                    .label(dat0_i18n::t("export.scope.full"))
                    .tab_stop(false),
            ])
            .selected_index(Some(scope_ix))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.scope_ix = *ix;
                cx.notify();
            }));

        let scope_stop = div()
            .focus_stop(
                "export-scope-group",
                &self.scope_focus,
                0,
                ring,
                |_ev, _window, _app| {},
            )
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                let delta = match ev.keystroke.key.as_str() {
                    "up" => -1,
                    "down" => 1,
                    _ => return,
                };
                this.scope_ix = cycle_ix(this.scope_ix, Self::SCOPES.len(), delta);
                cx.notify();
            }))
            .a11y(
                "export-scope-group",
                AccessRole::Button,
                dat0_i18n::t("export.scope"),
            )
            .child(scope_group);

        // ── Export / Cancel buttons ────────────────────────────────────────
        //
        // `overlay::modal_button`, not `gpui_component::Button`: a `Button`
        // keys its focus handle off the global element-id path, so the handle
        // could never be collected into `modal_trap`'s `Vec<FocusHandle>`.
        // Shared with `NamePrompt`, so the two modals cannot drift apart.
        let entity_run = cx.entity();
        let export_btn = crate::overlay::modal_button(
            "export-run",
            dat0_i18n::t("export.run").into(),
            &self.run_focus,
            crate::overlay::ModalButton::Primary,
            cx,
            move |_window, app| {
                entity_run.update(app, |this, cx| {
                    cx.emit(ExportEvent::Export {
                        scope: this.scope(),
                        format: this.format(),
                    });
                });
            },
        );

        let entity_cancel = cx.entity();
        let cancel_btn = crate::overlay::modal_button(
            "export-cancel",
            dat0_i18n::t("export.cancel").into(),
            &self.cancel_focus,
            crate::overlay::ModalButton::Ghost,
            cx,
            move |_window, app| {
                entity_cancel.update(app, |_this, cx| {
                    cx.emit(ExportEvent::Cancel);
                });
            },
        );

        // ── Assemble ───────────────────────────────────────────────────────
        v_flex()
            .gap_3()
            .p_4()
            .min_w(gpui::px(320.))
            // Escape cancels from ANY stop. `overlay::register_modal_keys`
            // binds `escape` → `gpui_component::input::Escape` under the
            // `Dat0Modal` key context that `modal_trap` installs on the shell
            // root, so this ancestor handler catches it; upstream binds
            // `escape` only under key context "Input", which is why Escape used
            // to be dead once focus left a text field.
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(ExportEvent::Cancel);
            }))
            .child(Label::new(dat0_i18n::t("export.format")))
            .child(format_stop)
            .child(Label::new(dat0_i18n::t("export.scope")))
            .child(scope_stop)
            .child(h_flex().gap_2().child(export_btn).child(cancel_btn))
    }
}

#[cfg(test)]
mod tests {
    use super::cycle_ix;

    #[test]
    fn cycle_ix_wraps_both_ways() {
        assert_eq!(cycle_ix(0, 3, 1), 1);
        assert_eq!(cycle_ix(2, 3, 1), 0, "last wraps to first");
        assert_eq!(cycle_ix(0, 3, -1), 2, "first wraps to last");
        assert_eq!(cycle_ix(0, 0, 1), 0, "an empty group cannot panic");
    }
}
