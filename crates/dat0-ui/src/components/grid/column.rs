//! Sorting and filtering the cursor's column (PD-040).
//!
//! A column's sort and funnel zones are the pointer's way to sort and filter
//! it, and until these commands they were the only way: neither the palette
//! nor the grid's context menu could. Sort Ascending, Sort Descending and
//! Filter Column… act on the column the grid's cursor is in, from both.

use dioxus::prelude::*;

use dat0_core::actions::builtin::ids;
use dat0_core::grid::selection::SelectionModel;
use dat0_core::view::filter_popover::ColumnType;
use dat0_engine::SortDirection;

use super::views::{Shown, Views};

/// Perform `id` on the cursor's column. False for an id this does not own.
///
/// With no table shown it does nothing, as Copy does with nothing selected:
/// the commands are always offered, and "sort nothing" is a nothing.
pub fn perform(id: &str, views: Views, shown: Shown, selection: Signal<SelectionModel>) -> bool {
    let direction = match id {
        ids::VIEW_SORT_ASC => Some(SortDirection::Asc),
        ids::VIEW_SORT_DESC => Some(SortDirection::Desc),
        ids::VIEW_FILTER => None,
        _ => return false,
    };
    let Some((ix, column, ty)) = cursor_column(views, shown, selection) else {
        return true;
    };
    match direction {
        Some(direction) => views.sort_by(&column, direction),
        None => {
            // Hung where a click on the column's funnel would have put it.
            spawn(async move {
                let at = crate::dom::anchor_below(&format!("col-funnel-{ix}"))
                    .await
                    .unwrap_or_default();
                views.open_funnel(column, ty, at);
            });
        }
    }
    true
}

/// The column under the grid's cursor: its place as the header shows it, its
/// source name and its type.
fn cursor_column(
    views: Views,
    shown: Shown,
    selection: Signal<SelectionModel>,
) -> Option<(usize, String, ColumnType)> {
    let (table, Ok(src)) = shown.read_unchecked().clone().flatten()? else {
        return None;
    };
    let ix = selection.peek().active().col;
    let column = views
        .columns(&table, &src.visible_column_names())
        .get(ix)?
        .source
        .clone();
    let ty = src
        .column_type_for_source(&column)
        .unwrap_or(ColumnType::String);
    Some((ix, column, ty))
}
