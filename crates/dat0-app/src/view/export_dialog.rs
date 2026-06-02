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

use gpui::{Context, EventEmitter, IntoElement, ParentElement, Render, Styled, Window};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    label::Label,
    radio::RadioGroup,
    v_flex,
};

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
/// Holds the two radio selections (format + scope) as plain state. The radios
/// drive the selection via their `on_click` handlers; the Export/Cancel buttons
/// emit [`ExportEvent`] to the subscribed `WorkspaceShell`. No `&mut Window` is
/// required at construction (the `RadioGroup` widget is `RenderOnce`), so the
/// entity is safe to build from `cx.new(|_| ExportDialog::new())`.
pub struct ExportDialog {
    /// 0 = CSV, 1 = JSON, 2 = Parquet (index into [`Self::FORMATS`]).
    format_ix: usize,
    /// 0 = Current view, 1 = Full table (index into [`Self::SCOPES`]).
    scope_ix: usize,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ExportDialog {
    const FORMATS: [ExportFormat; 3] =
        [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Parquet];
    const SCOPES: [ExportScope; 2] = [ExportScope::CurrentView, ExportScope::FullTable];

    /// Construct a fresh dialog defaulting to CSV + Current view.
    pub fn new() -> Self {
        Self {
            format_ix: 0,
            scope_ix: 0,
        }
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

        // ── Format radio group (CSV / JSON / Parquet) ──────────────────────
        let format_group = RadioGroup::horizontal("export-format")
            .children([
                dat0_i18n::t("export.format.csv"),
                dat0_i18n::t("export.format.json"),
                dat0_i18n::t("export.format.parquet"),
            ])
            .selected_index(Some(format_ix))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.format_ix = *ix;
                cx.notify();
            }));

        // ── Scope radio group (Current view / Full table) ──────────────────
        let scope_group = RadioGroup::vertical("export-scope")
            .children([
                dat0_i18n::t("export.scope.current"),
                dat0_i18n::t("export.scope.full"),
            ])
            .selected_index(Some(scope_ix))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.scope_ix = *ix;
                cx.notify();
            }));

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
            .child(format_group)
            .child(Label::new(dat0_i18n::t("export.scope")))
            .child(scope_group)
            .child(h_flex().gap_2().child(export_btn).child(cancel_btn))
    }
}
