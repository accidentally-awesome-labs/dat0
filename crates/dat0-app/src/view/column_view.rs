//! Projection fold: derive the visible `(source, display)` column list from a
//! transform stack. Reorder/Rename/DeleteColumn are display-only (design
//! Option B); this fold is the single source of truth for the grid's header
//! labels, column order, screen-col→source mapping, and the export projection.

use std::collections::HashSet;

use dat0_engine::transform::{ProjectionColumn, Transformation};

/// Fold the projection transforms in `ops` (stack order) over the source
/// columns `base` (visible base columns; the surrogate is already excluded by
/// the caller). Filter/Sort/Edit/RowDelete are ignored (they don't change the
/// visible column set). Returns the visible columns in display order, deleted
/// columns omitted.
pub fn fold_columns(base: &[String], ops: &[Transformation]) -> Vec<ProjectionColumn> {
    let mut cols: Vec<ProjectionColumn> = base
        .iter()
        .map(|s| ProjectionColumn {
            source: s.clone(),
            display: s.clone(),
        })
        .collect();
    let mut deleted: HashSet<String> = HashSet::new();

    for op in ops {
        match op {
            Transformation::Rename { column, to } => {
                if let Some(c) = cols.iter_mut().find(|c| &c.source == column) {
                    c.display = to.clone();
                }
            }
            Transformation::DeleteColumn { columns } => {
                deleted.extend(columns.iter().cloned());
            }
            Transformation::Reorder { columns } => {
                // `columns` is the full visible source order after this step.
                // Reorder `cols` to match; any source not listed (defensive)
                // keeps relative tail order so a column is never silently lost.
                let mut next: Vec<ProjectionColumn> = Vec::with_capacity(cols.len());
                for src in columns {
                    if let Some(pos) = cols.iter().position(|c| &c.source == src) {
                        next.push(cols[pos].clone());
                    }
                }
                for c in &cols {
                    if !columns.contains(&c.source) && !next.iter().any(|n| n.source == c.source) {
                        next.push(c.clone());
                    }
                }
                cols = next;
            }
            _ => {}
        }
    }

    cols.into_iter()
        .filter(|c| !deleted.contains(&c.source))
        .collect()
}
