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
    Context, EventEmitter, FocusHandle, IntoElement, ParentElement, Render, Styled, Window, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::{
    button::{Button, ButtonVariants as _},
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

        // ONE tab stop for the whole group. `focus_stop`'s Enter/Space
        // activation is a deliberate no-op: on a radiogroup the selection IS
        // the state, and a second submit path from inside a group would
        // surprise. Arrow selection arrives in T2.
        let format_stop = div()
            .focus_stop(
                "export-format-group",
                &self.format_focus,
                0,
                ring,
                |_ev, _window, _app| {},
            )
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
            .a11y(
                "export-scope-group",
                AccessRole::Button,
                dat0_i18n::t("export.scope"),
            )
            .child(scope_group);

        // ── Export / Cancel buttons ────────────────────────────────────────
        let entity_run = cx.entity();
        let export_btn = Button::new("export-run")
            .label(dat0_i18n::t("export.run"))
            .primary()
            .on_click(move |_ev, _window, cx| {
                entity_run.update(cx, |this, cx| {
                    cx.emit(ExportEvent::Export {
                        scope: this.scope(),
                        format: this.format(),
                    });
                });
            });

        let entity_cancel = cx.entity();
        let cancel_btn = Button::new("export-cancel")
            .label(dat0_i18n::t("export.cancel"))
            .ghost()
            .on_click(move |_ev, _window, cx| {
                entity_cancel.update(cx, |_this, cx| {
                    cx.emit(ExportEvent::Cancel);
                });
            });

        // ── Assemble ───────────────────────────────────────────────────────
        v_flex()
            .gap_3()
            .p_4()
            .min_w(gpui::px(320.))
            .child(Label::new(dat0_i18n::t("export.format")))
            .child(format_stop)
            .child(Label::new(dat0_i18n::t("export.scope")))
            .child(scope_stop)
            .child(h_flex().gap_2().child(export_btn).child(cancel_btn))
    }
}
