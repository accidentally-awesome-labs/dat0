//! Each tab's view — its sort, its filters and the rest of its transformation
//! stack — and the grid source that shows it.
//!
//! [`ViewModel`] is dat0-core's per-tab undo zipper. Its mutators are pure and
//! hand back a [`ViewChange`], and [`start_view_change`] drives the change
//! through the engine and returns the source to show. This module keeps one
//! model per tab and the source each tab is bound to. The header's sort and
//! funnel zones, the filter popover, the pipeline bar and Undo/Redo all come
//! through here; before it existed, none of them reached a model (PD-023).
//!
//! The grid reads [`Views::bound`], never a model's `active_view`: a model
//! names its next view the moment it changes, before the engine has created
//! it.

use std::collections::HashMap;
use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::{CellCoord, SelectionModel};
use dat0_core::view::column_view::reorder_payload;
use dat0_core::view::distinct_values::fetch_top_n;
use dat0_core::view::filter_popover::{ColumnType, Outcome};
use dat0_core::view::{ViewChange, ViewModel, fold_columns, route_outcome, start_view_change};
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DuckDBEngine, SortDirection, Transformation, quote_ident};

use crate::state::Workspace;

/// The source each tab shows, by the tab's table, with the table or view it
/// reads.
pub type Bound = Signal<HashMap<String, (String, Arc<GridDataSource>)>>;

/// What the grid shows: the tab's table, and its source or why there is none.
/// The table travels with the source because a tab switch changes the active
/// tab at once, and the source only once the new one is built.
pub type Shown = Resource<Option<(String, Result<Arc<GridDataSource>, String>)>>;

/// The funnel whose popover is open.
#[derive(Clone, PartialEq, Debug)]
pub struct Funnel {
    pub column: String,
    pub column_type: ColumnType,
    /// The column's filter, when it has one: the popover opens on it.
    pub existing: Option<Transformation>,
    /// Where the popover hangs: the funnel's client coordinates.
    pub at: (f64, f64),
    /// The column's most common values, once they arrive.
    pub candidates: Vec<String>,
    pub total_distinct: u64,
}

/// What one column's header shows: its sort rank and direction, and whether
/// it is filtered.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Mark {
    pub sort: Option<(usize, SortDirection)>,
    pub filtered: bool,
}

/// The views of this window's tabs.
#[derive(Clone, Copy)]
pub struct Views {
    ws: Workspace,
    models: Signal<HashMap<String, ViewModel>>,
    pub bound: Bound,
    pub funnel: Signal<Option<Funnel>>,
}

impl Views {
    /// This window's views. A hook: call it once, from the shell's body.
    pub fn use_new(ws: Workspace) -> Self {
        Self {
            ws,
            models: use_signal(HashMap::new),
            bound: use_signal(HashMap::new),
            funnel: use_signal(|| None),
        }
    }

    /// `table`'s columns as the header shows them: its base columns through the
    /// stack's reorders, renames and hidden columns.
    pub fn columns(&self, table: &str, base: &[String]) -> Vec<ProjectionColumn> {
        match self.models.read().get(table) {
            Some(vm) => fold_columns(base, vm.active()),
            None => base
                .iter()
                .map(|n| ProjectionColumn {
                    source: n.clone(),
                    display: n.clone(),
                })
                .collect(),
        }
    }

    /// Each column's mark, for `table`'s header.
    pub fn marks(&self, table: &str, columns: &[ProjectionColumn]) -> Vec<Mark> {
        let models = self.models.read();
        let Some(vm) = models.get(table) else {
            return vec![Mark::default(); columns.len()];
        };
        let sort = vm.current_sort_as_active();
        columns
            .iter()
            .map(|c| Mark {
                sort: sort.find(&c.source),
                filtered: vm.find_filter_for(&c.source).is_some(),
            })
            .collect()
    }

    /// `table`'s stack and how much of it is active, for the pipeline bar.
    pub fn stack(&self, table: &str) -> (Vec<Transformation>, usize) {
        self.models
            .read()
            .get(table)
            .map(|vm| (vm.stack().to_vec(), vm.cursor()))
            .unwrap_or_default()
    }

    /// A click in `column`'s sort zone: `none → asc → desc → none`. With
    /// `extend` (Shift), the column joins the sort instead of replacing it.
    pub fn sort(&self, column: &str, extend: bool) {
        self.change(|vm| {
            let now = vm.current_sort_as_active();
            let next = if extend {
                now.shift_click(column)
            } else {
                now.click(column)
            };
            if !next.keys().is_empty() {
                return Some(vm.set_sort(next.keys().to_vec()));
            }
            // Cycled off: drop the sort rather than keep an empty one.
            let at = vm
                .active()
                .iter()
                .position(|op| matches!(op, Transformation::Sort { .. }))?;
            Some(vm.remove_at(at))
        });
    }

    /// Open `column`'s funnel at `at`, on the column's filter if it has one,
    /// and fetch its most common values for the list.
    pub fn open_funnel(&self, column: String, column_type: ColumnType, at: (f64, f64)) {
        let Some(table) = self.ws.active_tab().map(|t| t.table) else {
            return;
        };
        let existing = self
            .models
            .peek()
            .get(&table)
            .and_then(|vm| vm.find_filter_for(&column).cloned());
        let mut funnel = self.funnel;
        funnel.set(Some(Funnel {
            column: column.clone(),
            column_type,
            existing,
            at,
            candidates: Vec::new(),
            total_distinct: 0,
        }));
        let Some(engine) = engine(&self.ws) else {
            return;
        };
        spawn(async move {
            let Ok((values, total)) = fetch_top_n(engine, &table, &column).await else {
                return;
            };
            // Only if that funnel is still the open one.
            let mut open = funnel.write();
            if let Some(f) = open.as_mut().filter(|f| f.column == column) {
                f.candidates = values.into_iter().map(|v| v.value).collect();
                f.total_distinct = total;
            }
        });
    }

    /// The open funnel's answer.
    pub fn funnel_outcome(&self, outcome: Outcome) {
        let mut funnel = self.funnel;
        funnel.set(None);
        self.change(|vm| route_outcome(vm, outcome));
    }

    /// A header dragged from `from` to `to`, in `columns` as the header showed
    /// them. Display-only: the rows are not read again.
    pub fn reorder(&self, columns: &[ProjectionColumn], from: usize, to: usize) {
        let order = reorder_payload(columns, from, to);
        self.change(|vm| Some(vm.apply(Transformation::Reorder { columns: order })));
    }

    /// The pipeline bar: keep the first `k` ops.
    pub fn jump(&self, k: usize) {
        self.change(|vm| Some(vm.jump_to(k)));
    }

    /// The pipeline bar: drop op `i`.
    pub fn remove(&self, i: usize) {
        self.change(|vm| Some(vm.remove_at(i)));
    }

    pub fn undo(&self) {
        self.change(ViewModel::undo);
    }

    pub fn redo(&self) {
        self.change(ViewModel::redo);
    }

    /// A console run replaced `table`'s rows: show `source`, and start its
    /// view over, since the stack was built on the rows it replaced.
    pub fn replaced(&self, table: String, source: Arc<GridDataSource>) {
        let (mut models, mut bound) = (self.models, self.bound);
        models.write().remove(&table);
        bound.write().insert(table.clone(), (table, source));
    }

    /// Change the active tab's model, and drive the change it returns.
    fn change(&self, f: impl FnOnce(&mut ViewModel) -> Option<ViewChange>) {
        if let Some(table) = self.ws.active_tab().map(|t| t.table) {
            self.change_on(table, f);
        }
    }

    /// Change `table`'s model, and drive the change it returns. Named rather
    /// than the active tab for a verb that reads rows first: the user may have
    /// switched tabs by the time it is ready.
    pub(super) fn change_on(
        &self,
        table: String,
        f: impl FnOnce(&mut ViewModel) -> Option<ViewChange>,
    ) {
        let Some(engine) = engine(&self.ws) else {
            return;
        };
        let change = {
            let mut models = self.models;
            let mut models = models.write();
            let vm = models
                .entry(table.clone())
                .or_insert_with(|| ViewModel::new(table.clone(), quote_ident(&table)));
            f(vm)
        };
        let Some(change) = change else {
            return;
        };
        // What the new source reads: the new view, or the table itself when
        // the stack emptied.
        let reads = change
            .new_active_view
            .clone()
            .unwrap_or_else(|| table.clone());
        // Claimed now, not when the future is polled: see `start_view_change`.
        let started = start_view_change(engine, table.clone(), change);
        let mut bound = self.bound;
        spawn(async move {
            // `None` is a display-only change, a superseded one, or a failure
            // already bannered: in every case the grid keeps what it shows.
            if let Some(source) = started.await {
                bound.write().insert(table, (reads, source));
            }
        });
    }
}

/// Keep the grid's widths and selection fitted to what it shows.
///
/// A width belongs to its column, not its place: hiding a column, dragging
/// one, or undoing either leaves every column its width, and another table
/// starts at the default. The selection is sized to the rows and columns
/// shown. While the same table stays up it is kept whole if the shape holds —
/// an edit rebinds the view, and the cursor must not jump back to the first
/// cell — and otherwise shrinks to its active cell, clamped.
pub fn use_fit(
    views: Views,
    shown: Shown,
    mut widths: Signal<Vec<f64>>,
    mut selection: Signal<SelectionModel>,
) {
    // The table, and the columns `widths` is laid out for.
    let mut fitted = use_signal(|| (String::new(), Vec::<String>::new()));
    use_effect(move || {
        let Some((table, Ok(src))) = shown.read().clone().flatten() else {
            return;
        };
        let now: Vec<String> = views
            .columns(&table, &src.visible_column_names())
            .into_iter()
            .map(|c| c.source)
            .collect();
        let (was_table, was) = fitted.peek().clone();
        let same = was_table == table;
        if !same || was != now {
            let old = widths.peek().clone();
            let next = now
                .iter()
                .map(|c| {
                    was.iter()
                        .position(|w| w == c)
                        .filter(|_| same)
                        .and_then(|i| old.get(i).copied())
                        .unwrap_or(super::COL_W_DEFAULT)
                })
                .collect();
            widths.set(next);
        }

        let rows = usize::try_from(src.row_count).unwrap_or(usize::MAX).max(1);
        let cols = now.len().max(1);
        let (kept, active) = {
            let s = selection.peek();
            (same && s.rows() == rows && s.cols() == cols, s.active())
        };
        if !kept {
            let mut next = SelectionModel::new(rows, cols);
            if same {
                next.click(CellCoord {
                    row: active.row.min(rows - 1),
                    col: active.col.min(cols - 1),
                });
            }
            selection.set(next);
        }
        fitted.set((table, now));
    });
}

fn engine(ws: &Workspace) -> Option<Arc<DuckDBEngine>> {
    ws.session
        .peek()
        .ready()
        .map(|slot| Arc::clone(&slot.lock().engine))
}
