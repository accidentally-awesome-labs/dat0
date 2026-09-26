//! The grid's edit verbs: a typed cell, copy, cut, paste, fill down, set NULL,
//! set a value, delete rows, and delete (hide) columns.
//!
//! Each becomes one step on the tab's view (`grid::views`) — an `Edit` or a
//! `RowDelete` laid over the table, or a hidden column — never a write to the
//! table itself. Undo takes a step back, and saving the view as a table is
//! what keeps it. Before this module the context menu offered these verbs
//! disabled, and the cell editor's commit went nowhere (PD-023).
//!
//! A cell is addressed by its row's `__dat0_rowid`, read from the page that
//! holds it. A selection reaching past the pages in memory has those pages
//! read first, so no verb quietly skips rows that are off screen. A selection
//! larger than [`MAX_CELLS`] ([`MAX_COPY_CELLS`] to copy) is refused, with a
//! banner, rather than walked.

use std::collections::BTreeMap;
use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::actions::builtin::ids;
use dat0_core::error_ux::Banner;
use dat0_core::grid::clipboard::{CoerceResult, coerce_cell};
use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::edit_ops::{mutation_blocked, parse_cell_text};
use dat0_core::grid::selection::{CellCoord, SelectionModel};
use dat0_core::view::filter_popover::ColumnType;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{CellEdit, RowKey, Scalar, Transformation};
use dat0_i18n::t;

use super::views::{Shown, Views};
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Modal, Workspace};

/// The most cells one edit writes, or rows one delete removes. Each edited
/// cell is a branch in the view's SQL, which every read of the view walks.
pub const MAX_CELLS: usize = 10_000;

/// The most cells one copy reads.
pub const MAX_COPY_CELLS: usize = 100_000;

/// The grid's edit verbs, for one window.
#[derive(Clone, Copy)]
pub struct Edits {
    ws: Workspace,
    views: Views,
    selection: Signal<SelectionModel>,
    shown: Shown,
}

/// What the grid shows, read once per verb.
struct Grid {
    table: String,
    src: Arc<GridDataSource>,
    columns: Vec<ProjectionColumn>,
}

impl Grid {
    /// The source column under display column `col`.
    fn source(&self, col: usize) -> Option<&str> {
        self.columns.get(col).map(|c| c.source.as_str())
    }
}

impl Edits {
    pub fn new(
        ws: Workspace,
        views: Views,
        selection: Signal<SelectionModel>,
        shown: Shown,
    ) -> Self {
        Self {
            ws,
            views,
            selection,
            shown,
        }
    }

    /// Run the grid verb `id`, from the palette, a menu or the context menu.
    /// `false` when `id` is not one of them.
    pub fn perform(&self, id: &str) -> bool {
        match id {
            ids::VIEW_COPY => self.copy(false),
            ids::VIEW_CUT => self.copy(true),
            ids::VIEW_PASTE => self.paste(),
            ids::VIEW_FILL_DOWN => self.fill_down(),
            ids::VIEW_SET_NULL => self.set_all(None),
            ids::VIEW_SET_VALUE => self.ask_value(),
            ids::VIEW_DELETE_ROWS => self.delete_rows(),
            ids::VIEW_DELETE_COLUMN => self.delete_columns(),
            _ => return false,
        }
        true
    }

    /// The cell editor's commit: `text` typed into `cell`.
    pub fn commit(&self, cell: CellCoord, text: String) {
        if self.refused() {
            return;
        }
        let Some(grid) = self.grid() else {
            return;
        };
        let Some(column) = grid.source(cell.col).map(str::to_string) else {
            return;
        };
        // Leaving the editor commits too: a cell left as it was writes
        // nothing, rather than an edit that changes nothing.
        if grid
            .src
            .cell_display_for_source(cell.row, &column)
            .as_deref()
            == Some(text.as_str())
        {
            return;
        }
        let ty = grid
            .src
            .column_type_for_source(&column)
            .unwrap_or(ColumnType::String);
        // The editor refuses text that does not parse; this only guards a
        // caller that did not ask it.
        let Some(value) = parse_cell_text(ty, &text) else {
            return;
        };
        self.write(grid, vec![(cell.row, column, value)]);
    }

    /// Copy the selection as TSV, over its bounding rectangle: a gap in a
    /// scattered selection copies as an empty cell. Cut then sets the
    /// selected cells to NULL.
    fn copy(&self, cut: bool) {
        if cut && self.refused() {
            return;
        }
        let sel = self.selection.peek().clone();
        let Some(b) = sel.bounds() else {
            return;
        };
        let area = (b.r1 - b.r0 + 1).saturating_mul(b.c1 - b.c0 + 1);
        if self.too_many(area, if cut { MAX_CELLS } else { MAX_COPY_CELLS }) {
            return;
        }
        let Some(grid) = self.grid() else {
            return;
        };
        if cut && !grid.src.has_row_ids() {
            return self.no_row_ids();
        }
        let this = *self;
        spawn(async move {
            let mut block = Vec::with_capacity(b.r1 - b.r0 + 1);
            for row in b.r0..=b.r1 {
                let mut line = Vec::with_capacity(b.c1 - b.c0 + 1);
                for col in b.c0..=b.c1 {
                    let text = match grid.source(col).filter(|_| sel.contains(row, col)) {
                        Some(source) => {
                            read_row(&grid.src, row, |s| s.cell_display_for_source(row, source))
                                .await
                        }
                        None => Some(String::new()),
                    };
                    let Some(text) = text else {
                        return this.unreadable();
                    };
                    line.push(text);
                }
                block.push(line);
            }
            if !crate::clipboard::copy_cells(&block) {
                return this.ws.push_banner(Banner::warning(t("grid.copy.failed")));
            }
            if cut {
                let cells = sel
                    .resolved_cells()
                    .filter_map(|(r, c)| grid.source(c).map(|s| (r, s.to_string(), Scalar::Null)))
                    .collect();
                this.write(grid, cells);
            }
        });
    }

    /// Paste the clipboard's block with its top-left at the active cell. A
    /// cell past the grid's edge, or not of its column's type, is skipped and
    /// counted; the rest are written as one step. TSV has no NULL, and a NULL
    /// copies out as an empty cell, so an empty cell pastes back as NULL.
    fn paste(&self) {
        if self.refused() {
            return;
        }
        let at = {
            let sel = self.selection.peek();
            if !sel.has_selection() {
                return;
            }
            sel.active()
        };
        // `None` for an empty clipboard. A block of empty cells is not one:
        // it is how a range of NULLs copies out.
        let Some(block) = crate::clipboard::paste_cells() else {
            return;
        };
        if self.too_many(block.iter().map(Vec::len).sum(), MAX_CELLS) {
            return;
        }
        let Some(grid) = self.grid() else {
            return;
        };
        let rows = usize::try_from(grid.src.row_count).unwrap_or(usize::MAX);
        let mut cells = Vec::new();
        let mut skipped = 0usize;
        for (dr, line) in block.iter().enumerate() {
            for (dc, text) in line.iter().enumerate() {
                let (row, col) = (at.row + dr, at.col + dc);
                let value = grid.source(col).filter(|_| row < rows).and_then(|source| {
                    if text.is_empty() {
                        return Some((source.to_string(), Scalar::Null));
                    }
                    let ty = grid.src.column_arrow_type_for_source(source)?;
                    match coerce_cell(text, &ty) {
                        CoerceResult::Ok(v) => Some((source.to_string(), v)),
                        CoerceResult::Skip => None,
                    }
                });
                match value {
                    Some((source, v)) => cells.push((row, source, v)),
                    None => skipped += 1,
                }
            }
        }
        if skipped > 0 {
            self.ws.push_banner(Banner::warning_with_body(
                t("grid.paste.skipped"),
                t("grid.paste.skipped.body"),
            ));
        }
        self.write(grid, cells);
    }

    /// Give each selected column's lower cells the value of its top one. The
    /// value, not its display text: a float keeps its digits.
    fn fill_down(&self) {
        if self.refused() {
            return;
        }
        let sel = self.selection.peek().clone();
        if !sel.has_selection() || self.too_many(sel.selected_cell_count(), MAX_CELLS) {
            return;
        }
        let Some(grid) = self.grid() else {
            return;
        };
        if !grid.src.has_row_ids() {
            return self.no_row_ids();
        }
        // Each column's selected rows, top first: the cells come row-major.
        let mut by_col: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (r, c) in sel.resolved_cells() {
            by_col.entry(c).or_default().push(r);
        }
        let this = *self;
        spawn(async move {
            let mut cells = Vec::new();
            for (col, rows) in by_col {
                let (Some(source), Some((&top, below))) = (grid.source(col), rows.split_first())
                else {
                    continue;
                };
                let Some(value) =
                    read_row(&grid.src, top, |s| s.cell_scalar_for_source(top, source)).await
                else {
                    return this.unreadable();
                };
                cells.extend(
                    below
                        .iter()
                        .map(|&r| (r, source.to_string(), value.clone())),
                );
            }
            this.write(grid, cells);
        });
    }

    /// Ask for a value, then set every selected cell to it.
    fn ask_value(&self) {
        if self.refused() || !self.selection.peek().has_selection() {
            return;
        }
        let this = *self;
        let mut modal = self.ws.modal;
        modal.set(Some(Modal::NamePrompt {
            title: t("grid.set_value"),
            initial: String::new(),
            placeholder: None,
            confirm_label: Some(t("grid.set_value.confirm")),
            secret: false,
            reply: ModalReply::new(move |outcome| {
                if let ModalOutcome::Named(text) = outcome {
                    this.set_all(Some(text));
                }
            }),
        }));
    }

    /// Set every selected cell to NULL, or to `text` read as its column's
    /// type. A cell whose column cannot hold `text` is skipped and counted.
    fn set_all(&self, text: Option<String>) {
        if self.refused() {
            return;
        }
        let sel = self.selection.peek().clone();
        if !sel.has_selection() || self.too_many(sel.selected_cell_count(), MAX_CELLS) {
            return;
        }
        let Some(grid) = self.grid() else {
            return;
        };
        let mut skipped = 0usize;
        let cells = sel
            .resolved_cells()
            .filter_map(|(r, c)| {
                let source = grid.source(c)?;
                let value = match &text {
                    None => Scalar::Null,
                    Some(text) => {
                        let ty = grid
                            .src
                            .column_type_for_source(source)
                            .unwrap_or(ColumnType::String);
                        let Some(v) = parse_cell_text(ty, text) else {
                            skipped += 1;
                            return None;
                        };
                        v
                    }
                };
                Some((r, source.to_string(), value))
            })
            .collect();
        if skipped > 0 {
            self.ws.push_banner(Banner::warning_with_body(
                t("grid.set_value.skipped"),
                t("grid.set_value.skipped.body"),
            ));
        }
        self.write(grid, cells);
    }

    /// Delete every row holding a selected cell, as one step.
    fn delete_rows(&self) {
        if self.refused() {
            return;
        }
        let sel = self.selection.peek().clone();
        if !sel.has_selection() || self.too_many(sel.selected_row_count(), MAX_CELLS) {
            return;
        }
        let Some(grid) = self.grid() else {
            return;
        };
        if !grid.src.has_row_ids() {
            return self.no_row_ids();
        }
        let rows = sel.selected_rows();
        let this = *self;
        spawn(async move {
            let mut keys = Vec::with_capacity(rows.len());
            for row in rows {
                let Some(id) = read_row(&grid.src, row, |s| s.row_key(row)).await else {
                    return this.unreadable();
                };
                keys.push(RowKey::Surrogate { id });
            }
            this.views
                .change_on(grid.table, |vm| Some(vm.delete_rows(keys)));
        });
    }

    /// Hide every column holding a selected cell. The column stays in the
    /// table, so a filter or a sort on it still holds, and Undo shows it
    /// again.
    fn delete_columns(&self) {
        if self.refused() {
            return;
        }
        let cols = self.selection.peek().selected_cols();
        if cols.is_empty() {
            return;
        }
        let Some(grid) = self.grid() else {
            return;
        };
        let columns: Vec<String> = cols
            .into_iter()
            .filter_map(|c| grid.source(c).map(str::to_string))
            .collect();
        if columns.len() >= grid.columns.len() {
            self.ws
                .push_banner(Banner::warning(t("grid.delete_column.last")));
            return;
        }
        self.views.change_on(grid.table, |vm| {
            Some(vm.apply(Transformation::DeleteColumn { columns }))
        });
    }

    /// Write `cells`, as `(row, column, value)`, into the view as one step.
    fn write(&self, grid: Grid, cells: Vec<(usize, String, Scalar)>) {
        if cells.is_empty() {
            return;
        }
        if !grid.src.has_row_ids() {
            return self.no_row_ids();
        }
        let this = *self;
        spawn(async move {
            let mut edits = Vec::with_capacity(cells.len());
            for (row, column, value) in cells {
                let Some(id) = read_row(&grid.src, row, |s| s.row_key(row)).await else {
                    return this.unreadable();
                };
                edits.push(CellEdit {
                    row: RowKey::Surrogate { id },
                    column,
                    value,
                });
            }
            this.views
                .change_on(grid.table, |vm| Some(vm.edit_cells(edits)));
        });
    }

    /// What the grid shows now: its table, its source and its columns.
    fn grid(&self) -> Option<Grid> {
        let (table, src) = match self.shown.peek().clone().flatten()? {
            (table, Ok(src)) => (table, src),
            (_, Err(_)) => return None,
        };
        let columns = self.views.columns(&table, &src.visible_column_names());
        Some(Grid {
            table,
            src,
            columns,
        })
    }

    /// Refuse a change in a read-only workspace, and say so.
    fn refused(&self) -> bool {
        if !mutation_blocked(*self.ws.read_only.peek()) {
            return false;
        }
        self.ws.push_banner(Banner::warning(t("view.read_only")));
        true
    }

    /// Refuse a selection of `n` cells or rows when more than `most`, and say
    /// so.
    fn too_many(&self, n: usize, most: usize) -> bool {
        if n <= most {
            return false;
        }
        self.ws.push_banner(Banner::warning_with_body(
            t("grid.edit.too_many"),
            t("grid.edit.too_many.body"),
        ));
        true
    }

    /// A query's results have no row identity to address an edit by.
    fn no_row_ids(&self) {
        self.ws.push_banner(Banner::warning_with_body(
            t("grid.edit.no_row_ids"),
            t("grid.edit.no_row_ids.body"),
        ));
    }

    /// A page the verb needed could not be read.
    fn unreadable(&self) {
        self.ws.push_banner(Banner::warning(t("grid.page.failed")));
    }
}

/// `read` for `row`, reading the row's page first if it is not in memory.
async fn read_row<T>(
    src: &GridDataSource,
    row: usize,
    read: impl Fn(&GridDataSource) -> Option<T>,
) -> Option<T> {
    if let Some(v) = read(src) {
        return Some(v);
    }
    src.page_for(u64::try_from(row).ok()?).await.ok()?;
    read(src)
}
