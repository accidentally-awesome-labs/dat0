//! The Export… dialog's pure half: what a chosen scope means, and the SELECT
//! it compiles to.
//!
//! The dialog widget lives in the UI crate; this is the part a test can check
//! and a headless export can reuse.

use dat0_engine::transform::ProjectionColumn;

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
