//! How a transform reads in the pipeline bar.
//!
//! The bar is a widget; the label is prose derived from a `Transformation`, and
//! both renderers want the same words.

use dat0_engine::{SortDirection, Transformation};

/// Human-readable one-line label for a pill / timeline row.
pub fn describe_transform(t: &Transformation) -> String {
    match t {
        Transformation::Filter { column, .. } => format!("Filter {column}"),
        Transformation::Sort { keys } => {
            if let Some(k) = keys.first() {
                let arrow = match k.direction {
                    SortDirection::Asc => "↑",
                    SortDirection::Desc => "↓",
                };
                let more = if keys.len() > 1 {
                    format!(" +{}", keys.len() - 1)
                } else {
                    String::new()
                };
                format!("Sort {}{}{}", k.column, arrow, more)
            } else {
                "Sort".into()
            }
        }
        Transformation::Edit { cells } => format!("Edit {} cell(s)", cells.len()),
        Transformation::RowDelete { rows } => format!("Delete {} row(s)", rows.len()),
        Transformation::Reorder { .. } => "Reorder columns".into(),
        Transformation::Rename { column, to } => format!("Rename {column}→{to}"),
        Transformation::DeleteColumn { columns } => {
            format!("Delete col {}", columns.join(", "))
        }
    }
}
