//! Closing a data tab (PD-038).
//!
//! The tab strip only activated tabs, and no command closed one, so tabs
//! accumulated until the only way out was a new window. A tab now closes from
//! its ✕, from File → Close Tab and from the palette.
//!
//! A tab closes with its view: the steps laid over its table — sorts, filters,
//! edits — and the view the engine built from them. The table stays in the
//! session: the console still reads it, a saved query or chart can still name
//! it, a package still carries it, and opening its file again brings it back.
//!
//! Cell edits and deleted rows live only in the view, so when the view has
//! any, the dialog says how many and asks first, as Live Refresh does.

use dioxus::prelude::*;

use dat0_engine::Transformation;

use super::views::Views;
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Modal, TabView, Workspace};

/// Close the active tab: File → Close Tab and the palette.
pub fn close_active(ws: Workspace, views: Views) {
    // Read out before closing: a guard held in the `if let` would still be
    // borrowing `active` when the close moves it.
    let active = *ws.active.peek();
    if let Some(i) = active {
        close(ws, views, i);
    }
}

/// Close the tab at `i`, asking first when its view holds work the table
/// does not.
pub fn close(ws: Workspace, views: Views, i: usize) {
    let Some(tab) = ws.tabs.peek().get(i).cloned() else {
        return;
    };
    let (stack, _) = views.stack(&tab.table);
    let (edits, deletes) = discarded(&stack);
    if edits + deletes == 0 {
        return discard(ws, views, &tab);
    }
    let mut modal = ws.modal;
    modal.set(Some(Modal::CloseTab {
        tab: tab.title().to_string(),
        edits,
        deletes,
        reply: ModalReply::new(move |outcome| {
            if matches!(outcome, ModalOutcome::Confirmed) {
                discard(ws, views, &tab);
            }
        }),
    }));
}

/// The cells edited and the rows deleted in `ops`: what closing the view
/// loses. Cells and rows, not steps — a paste over four hundred cells is one
/// step, and the user deciding needs to know it is four hundred.
fn discarded(ops: &[Transformation]) -> (usize, usize) {
    ops.iter().fold((0, 0), |(edits, deletes), op| match op {
        Transformation::Edit { cells } => (edits + cells.len(), deletes),
        Transformation::RowDelete { rows } => (edits, deletes + rows.len()),
        _ => (edits, deletes),
    })
}

/// Take `tab` out of the strip, and its view with it once no other tab shows
/// the table.
///
/// Found again by value rather than trusted by index: while the dialog was up
/// a run may have opened a tab, or a detach closed one.
fn discard(ws: Workspace, views: Views, tab: &TabView) {
    let Some(i) = ws.tabs.peek().iter().position(|t| t == tab) else {
        return;
    };
    let was_active = *ws.active.peek() == Some(i);
    ws.close_tab(i);
    if was_active {
        // A funnel hangs off the closed tab's header, and its answer would
        // filter whichever tab is active now.
        let mut funnel = views.funnel;
        funnel.set(None);
    }
    if ws.tabs.peek().iter().any(|t| t.table == tab.table) {
        return;
    }
    views.forget(&tab.table);
    // "The file changed" is about a tab that is gone, and its Refresh would
    // read whichever tab is active now.
    if let Some(path) = &tab.path {
        super::refresh::clear_changed_banner(ws, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::transform::{CellEdit, RowKey};
    use dat0_engine::{Scalar, SortDirection, SortKey};

    fn edit(n: usize) -> Transformation {
        Transformation::Edit {
            cells: (0..n)
                .map(|i| CellEdit {
                    row: RowKey::Surrogate { id: i as i64 },
                    column: "a".into(),
                    value: Scalar::Null,
                })
                .collect(),
        }
    }

    #[test]
    fn what_closing_loses_is_counted_in_cells_and_rows() {
        let ops = vec![
            Transformation::Sort {
                keys: vec![SortKey {
                    column: "a".into(),
                    direction: SortDirection::Asc,
                }],
            },
            edit(400),
            edit(1),
            Transformation::RowDelete {
                rows: vec![RowKey::Surrogate { id: 1 }, RowKey::Surrogate { id: 2 }],
            },
        ];
        assert_eq!(discarded(&ops), (401, 2));
    }

    #[test]
    fn a_view_of_sorts_and_filters_loses_nothing_the_table_lacks() {
        let ops = vec![Transformation::Sort {
            keys: vec![SortKey {
                column: "a".into(),
                direction: SortDirection::Desc,
            }],
        }];
        assert_eq!(discarded(&ops), (0, 0));
    }
}
