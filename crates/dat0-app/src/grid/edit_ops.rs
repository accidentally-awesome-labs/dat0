//! Grid edit / clipboard / projection handlers for [`WorkspaceShell`] (P4c T14).
//!
//! These methods were extracted **verbatim** from `crate::window` to keep that
//! module under its size budget; the move is byte-behaviour-neutral (the full
//! `dat0-app` suite stays green with zero test changes). They form a second
//! `impl crate::window::WorkspaceShell` block and reach the shell's
//! `pub(crate)` fields + helpers (`column_name`, `spawn_rebind`,
//! `refresh_column_view`, the free `bounding_rect`).
//!
//! Scope (the mutating / projection seam):
//!   * inline cell edit: [`WorkspaceShell::begin_cell_edit`] /
//!     [`WorkspaceShell::commit_cell_edit`]
//!   * clipboard: [`WorkspaceShell::copy_selection`] /
//!     [`WorkspaceShell::cut_selection`] / [`WorkspaceShell::paste_clipboard`]
//!   * bulk ops: [`WorkspaceShell::fill_down`] /
//!     [`WorkspaceShell::set_null_selection`] /
//!     [`WorkspaceShell::set_value_selection`] /
//!     [`WorkspaceShell::delete_selected_rows`]
//!   * projection: [`WorkspaceShell::on_reorder_columns`] /
//!     [`WorkspaceShell::begin_column_rename`] /
//!     [`WorkspaceShell::commit_column_rename`] /
//!     [`WorkspaceShell::delete_column`] / [`WorkspaceShell::route_change`]
//!   * header/row click-select: [`WorkspaceShell::select_column_at`] /
//!     [`WorkspaceShell::select_row_at`]
//!
//! View-state methods (sort/funnel/filter, `spawn_rebind`, `apply_view_change`,
//! `refresh_column_view`, pipeline_*, run_export) stay in `crate::window`.

use gpui::{Context, Window, prelude::*};

use crate::window::{WorkspaceShell, bounding_rect};

// ── Inline cell editor: mount + commit (T6) ──────────────────────────────────

/// Direction the active cell advances after an Enter commit (P4c T14).
///
/// `Enter` commits the inline edit then moves the cursor one row DOWN and
/// re-opens the editor on the new cell (spreadsheet semantics). `Tab` → RIGHT
/// is PD-020 (the gpui-component `Input` does not surface a Tab event, so a
/// Tab-advance cannot be wired without focus contention — see the editor's
/// module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitAdvance {
    /// Move the active cell down one row and re-open the editor there.
    Down,
}

impl WorkspaceShell {
    /// Hybrid write-path hook (P6a T12) for a multi-cell / bulk DATA mutation
    /// (paste, cut, delete-rows, fill-down, set-null, set-value). Drives a
    /// STRUCTURAL inspector-profile refresh for the live target table. The bare
    /// table name is the inspector target (set bare by `open_table_tab`). No-op
    /// when no table is open / no inspector target is set.
    fn refresh_inspector_after_bulk_mutation(&mut self, cx: &mut Context<Self>) {
        if let Some(table) = self.inspector.target_table.clone() {
            self.on_table_mutated_structural(&table, cx);
        }
    }

    /// Commit the in-flight cell edit (T6). Resolves the active selection cell
    /// to a [`dat0_engine::RowKey::Surrogate`] + column name via the active
    /// `GridDataSource`, builds a single-cell [`dat0_engine::CellEdit`], pushes
    /// it through [`crate::view::ViewModel::edit_cells`], and drives the engine
    /// round-trip via [`WorkspaceShell::spawn_rebind`].
    ///
    /// No-op (graceful) when no data source / view model is mounted, when the
    /// selected row's page isn't cached (so `row_key` returns `None`), or when
    /// the column index is out of range.
    ///
    /// Called by the inline [`crate::grid::cell_editor::CellEditor`] on commit
    /// (Enter / focus-out). T11 wires the Enter/F2 keystroke that mounts the
    /// editor over the active cell.
    pub fn commit_cell_edit(&mut self, value: dat0_engine::Scalar, cx: &mut Context<Self>) {
        let (Some(ds), Some(_vm)) = (self.data_source.as_ref(), self.view_model.as_ref()) else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let active = selection.active();
        let Some(key) = ds.row_key(active.row) else {
            return;
        };
        // Resolve the SCREEN column → its SOURCE identity via the ColumnView
        // (P4c T5) so an edit addresses the right column even after a
        // display-only reorder. With no projection ops this equals
        // `ds.column_name(active.col)`.
        let Some(col) = self.column_name(active.col) else {
            return;
        };
        let cell = dat0_engine::CellEdit {
            row: dat0_engine::RowKey::Surrogate { id: key },
            column: col,
            value,
        };
        // `view_model` is `Some` per the guard above; `unwrap` mirrors the plan.
        let change = self
            .view_model
            .as_mut()
            .expect("view_model checked above")
            .edit_cells(vec![cell]);
        // Drive the engine round-trip FIRST (spawn_rebind does not touch
        // `cell_editor` / `cell_editor_sub`), then tear down the editor so the
        // teardown order is: submit → dismiss (defensive; avoids any hypothetical
        // future dependency on the editor still being mounted at rebind time).
        self.spawn_rebind(change, cx);
        self.cell_editor = None;
        self.cell_editor_sub = None;
        // Hybrid write path (P6a T12): a single cell changed → refresh the
        // inspector profile. We use the STRUCTURAL strategy (not the single-column
        // `on_cell_mutated` patch) deliberately: in this architecture an `Edit` is
        // a display-layer OVERLAY compiled into the active view's SQL
        // (`SELECT * REPLACE (CASE … END)`, see engine `render.rs`) — the BASE
        // table is never mutated in place. So a single-column re-profile of the
        // bare base table (`SELECT col FROM "orders"`) would NOT reflect the edit
        // in `CurrentView` mode and would be a no-op in `WholeTable` mode. The
        // structural refresh re-runs `load_inspector_profile`, which in
        // `CurrentView` re-compiles the live view SQL (overlay included) and in
        // `WholeTable` re-profiles the base table (correctly unchanged). Bare
        // table name = the inspector target (set bare by `open_table_tab`).
        self.refresh_inspector_after_bulk_mutation(cx);
    }

    /// Commit the in-flight cell edit, then advance the active cell and re-open
    /// the inline editor on the new cell (P4c T14 Enter-advance).
    ///
    /// `Enter` in the inline editor emits
    /// [`crate::grid::cell_editor::CellEditorEvent::CommitAndMove`], which the
    /// `begin_cell_edit` subscription routes here. We commit through
    /// [`Self::commit_cell_edit`] (which tears the editor down + spawns the
    /// rebind), move the selection cursor one row down via the `SelectionModel`,
    /// then re-mount the editor on the new cell — spreadsheet "Enter walks down a
    /// column" semantics. No-op move when no selection model is mounted.
    pub fn commit_cell_edit_and_advance(
        &mut self,
        value: dat0_engine::Scalar,
        advance: CommitAdvance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_cell_edit(value, cx);
        let (dr, dc) = match advance {
            CommitAdvance::Down => (1isize, 0isize),
        };
        if let Some(sel) = self.selection.as_mut() {
            sel.move_active(dr, dc);
        } else {
            // No selection model → nothing to advance onto; the commit already ran.
            return;
        }
        // Re-open the editor on the freshly-advanced cell so the user can keep
        // typing down the column. `begin_cell_edit` is a no-op if no data source
        // / selection is available, so this is safe.
        self.begin_cell_edit(window, cx);
    }

    /// Mount the inline cell editor over the active selection cell (T6).
    ///
    /// Constructs a [`crate::grid::cell_editor::CellEditor`] entity typed for
    /// the active column's Arrow type, subscribes to its commit/cancel events
    /// (the subscription is **stored** in `self.cell_editor_sub` — a dropped
    /// `Subscription` deregisters silently, the P4a T10b trap), and asks the
    /// view to re-render so the editor mounts as an overlay.
    ///
    /// No-op when no data source / selection is available. T11 wires the
    /// Enter/F2 keystroke that calls this; T6 provides the editor + commit path.
    pub fn begin_cell_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::grid::cell_editor::{CellEditor, CellEditorEvent};

        // Guard: both the filter popover and the cell editor render as absolute
        // overlay children.  Mounting the editor while a popover is open would
        // stack two overlays and leave no obvious dismiss path for the popover.
        // The user must close the popover before starting an edit.
        if self.active_popover.is_some() {
            return;
        }

        let Some(ds) = self.data_source.as_ref() else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let active = selection.active();
        // Type the inline editor off the SOURCE column (resolved via the
        // ColumnView) so a display-only reorder edits with the right type
        // (P4c T5). Identity with no projection ops.
        let column_type = self
            .column_name(active.col)
            .and_then(|source| ds.column_type_for_source(&source))
            .unwrap_or(crate::view::filter_popover::ColumnType::String);

        let editor = cx.new(|_| CellEditor::new(column_type));

        // P4c T14: focus the editor's inner widget on mount so keystrokes land in
        // the input immediately (no click required). The handle lives on the
        // `CellEditor`; `focus()` no-ops gracefully before the first render (the
        // inner `InputState`/`SelectState` is lazily built on first paint, so the
        // editor re-focuses itself there too).
        editor.update(cx, |ed, cx| ed.focus(window, cx));

        // STORE the subscription — callbacks fire silently if the returned
        // Subscription is dropped (P4a T10b post-review lesson; mirrors
        // `on_funnel_click`'s `popover_sub`).
        let sub = cx.subscribe_in(
            &editor,
            window,
            move |ws: &mut Self, _editor, ev: &CellEditorEvent, window, cx| match ev {
                CellEditorEvent::Commit(value) => {
                    ws.commit_cell_edit(value.clone(), cx);
                }
                CellEditorEvent::CommitAndMove(value, advance) => {
                    let dir = match advance {
                        crate::grid::cell_editor::EditorAdvance::Down => CommitAdvance::Down,
                    };
                    ws.commit_cell_edit_and_advance(value.clone(), dir, window, cx);
                }
                CellEditorEvent::Cancel => {
                    ws.cell_editor = None;
                    ws.cell_editor_sub = None;
                    // Intentionally NOT clearing `ws.selection` here — cancel
                    // must leave the cursor on the cell the user was editing so
                    // they can immediately retry or navigate away.
                    cx.notify();
                }
            },
        );
        self.cell_editor_sub = Some(sub);
        self.cell_editor = Some(editor);
        cx.notify();
    }

    // ── Column reorder + change routing (P4c T6) ─────────────────────────────

    /// Apply a column reorder (grip drag). `from`/`to` are screen column indices.
    ///
    /// Builds the full new visible source order via [`crate::view::column_view::reorder_payload`],
    /// applies a display-only [`dat0_engine::Transformation::Reorder`] through the `ViewModel`,
    /// refreshes the `ColumnView`, and routes the change (display-only → `cx.notify()`; else
    /// engine round-trip). No-op when `from == to`, or when no `ViewModel` is mounted.
    pub fn on_reorder_columns(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to {
            return;
        }
        let columns = crate::view::column_view::reorder_payload(&self.column_view, from, to);
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.apply(dat0_engine::Transformation::Reorder { columns });
        self.refresh_column_view();
        self.route_change(change, cx);
    }

    /// Route a [`crate::view::ViewChange`]: display-only (projection) → just re-render;
    /// otherwise spawn the engine rebind. Always called after `refresh_column_view` has
    /// already been applied (projection ops) — the `notify()` re-renders with the updated
    /// `column_view` already in place.
    pub fn route_change(&mut self, change: crate::view::ViewChange, cx: &mut Context<Self>) {
        if change.is_display_only() {
            cx.notify();
        } else {
            self.spawn_rebind(change, cx);
        }
        // Hybrid write path (P6a T12): projection/transform applies routed here
        // (reorder, rename, delete-column, transform-apply) change the table's
        // schema/derivation → STRUCTURAL refresh of the inspector profile. A pure
        // display-only change leaves the WholeTable profile unchanged, but bumping
        // the epoch is harmless (at most one extra re-SUMMARIZE) and keeps a
        // CurrentView profile honest, so we refresh unconditionally. The bare
        // table name is the inspector target (set bare by `open_table_tab`).
        if let Some(table) = self.inspector.target_table.clone() {
            self.on_table_mutated_structural(&table, cx);
        }
    }

    // ── Inline column rename (P4c T7) ────────────────────────────────────────

    /// Commit a column rename. `col_ix` is the screen column index; `to` the new
    /// label entered by the user.
    ///
    /// Trims `to` and resolves the source column via the active `ColumnView`.
    /// No-ops (clears the editor) when `to` is empty or unchanged. Otherwise
    /// applies a display-only [`dat0_engine::Transformation::Rename`] through the
    /// `ViewModel`, refreshes the `ColumnView`, and routes the change (display-only
    /// → `cx.notify()`; the underlying data is untouched).
    pub fn commit_column_rename(&mut self, col_ix: usize, to: String, cx: &mut Context<Self>) {
        let to = to.trim().to_string();
        let Some(source) = self.column_name(col_ix) else {
            // No column at this screen index — dismiss editor cleanly.
            self.header_rename = None;
            self.header_rename_sub = None;
            cx.notify();
            return;
        };
        // No-op: empty label or label same as the source identity — just dismiss.
        if to.is_empty() || to == source {
            self.header_rename = None;
            self.header_rename_sub = None;
            cx.notify();
            return;
        }
        let Some(vm) = self.view_model.as_mut() else {
            self.header_rename = None;
            self.header_rename_sub = None;
            cx.notify();
            return;
        };
        let change = vm.apply(dat0_engine::Transformation::Rename { column: source, to });
        self.header_rename = None;
        self.header_rename_sub = None;
        self.refresh_column_view();
        self.route_change(change, cx);
    }

    /// Mount the inline column-header rename editor for `col_ix` (P4c T7).
    ///
    /// Constructs a [`crate::grid::cell_editor::HeaderRenameEditor`] seeded with
    /// the current DISPLAY label of the column, subscribes to its commit/cancel
    /// events (the subscription is **stored** in `self.header_rename_sub` — a
    /// dropped `Subscription` deregisters silently, the P4a T10b trap), and asks
    /// the view to re-render so the editor appears in-place inside `render_th`.
    ///
    /// No-op when no `ColumnView` entry exists for `col_ix`.
    pub fn begin_column_rename(&mut self, col_ix: usize, cx: &mut Context<Self>) {
        use crate::grid::cell_editor::{HeaderRenameEditor, HeaderRenameEvent};

        // Resolve the current DISPLAY label (not source) as the seed.
        let Some(display) = self.column_view.get(col_ix).map(|c| c.display.clone()) else {
            return;
        };

        let editor = cx.new(|_| HeaderRenameEditor::new(display));

        // STORE the subscription — callbacks fire silently if the returned
        // Subscription is dropped (P4a T10b post-review lesson; mirrors
        // `on_funnel_click`'s `popover_sub`).
        let sub = cx.subscribe(
            &editor,
            move |ws: &mut Self, _editor, ev: &HeaderRenameEvent, cx| match ev {
                HeaderRenameEvent::Commit(text) => {
                    ws.commit_column_rename(col_ix, text.clone(), cx);
                }
                HeaderRenameEvent::Cancel => {
                    ws.header_rename = None;
                    ws.header_rename_sub = None;
                    cx.notify();
                }
            },
        );
        self.header_rename_sub = Some(sub);
        self.header_rename = Some((col_ix, editor));
        cx.notify();
    }

    // ── Clipboard: copy / cut / paste (T7) ───────────────────────────────────
    //
    // T11 wires the Cmd+C / Cmd+X / Cmd+V (Ctrl on Linux) keybinds → these
    // handlers; T7 only exposes them. The pure-logic TSV codec + coerce live in
    // `crate::grid::clipboard`; these handlers are the thin GPUI glue (clipboard
    // I/O + ViewModel round-trip) — build+clippy verified, with the real Excel /
    // Sheets round-trip exercised in T14 manual UAT.

    /// Build the bounding-rectangle grid of the current selection's display
    /// values, serialize it as spreadsheet-TSV, and write it to the system
    /// clipboard (T7 copy). Gaps in a discontiguous selection become empty
    /// cells (the bounding-rect convention; mirrors Excel / Sheets). Records the
    /// copied range for the marching-ants border (rendered in T11/polish).
    ///
    /// No-op (graceful) when no data source / selection is mounted or the
    /// selection is empty. Cells whose page isn't cached read as empty strings
    /// (the synchronous-only contract — copy never blocks on a DuckDB fetch).
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let Some(tsv) = self.build_selection_tsv() else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(tsv));
        cx.notify();
    }

    /// Copy the selection, then clear every selected cell to NULL in a single
    /// undo step (T7 cut). The NULL edits are coerced through one
    /// [`ViewModel::edit_cells`] call (one undo step) + one
    /// [`Self::spawn_rebind`], exactly like paste.
    ///
    /// No-op when no data source / view model / selection is mounted.
    pub fn cut_selection(&mut self, cx: &mut Context<Self>) {
        // Copy first (sets the clipboard + marching-ants range).
        self.copy_selection(cx);

        let (Some(ds), Some(_vm)) = (self.data_source.as_ref(), self.view_model.as_ref()) else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        // One CellEdit setting each selected cell to NULL. Cells whose page
        // isn't cached (so `row_key` returns None) or whose column index is out
        // of range are skipped — they can't be addressed.
        let mut edits: Vec<dat0_engine::CellEdit> = Vec::new();
        for (row, col) in selection.resolved_cells() {
            // Resolve screen-col → source via the ColumnView (P4c T5).
            let (Some(id), Some(column)) = (ds.row_key(row), self.column_name(col)) else {
                continue;
            };
            edits.push(dat0_engine::CellEdit {
                row: dat0_engine::RowKey::Surrogate { id },
                column,
                value: dat0_engine::Scalar::Null,
            });
        }
        if edits.is_empty() {
            return;
        }

        let change = self
            .view_model
            .as_mut()
            .expect("view_model checked above")
            .edit_cells(edits);
        self.spawn_rebind(change, cx);
        // Multi-cell DATA mutation → structural inspector refresh (P6a T12).
        self.refresh_inspector_after_bulk_mutation(cx);
    }

    /// Paste the clipboard's TSV block, anchored at the active selection cell
    /// (T7). Parses the clipboard text, clamps the pasted block to the grid edge
    /// (`row_count` × visible-column-count — out-of-range cells are dropped),
    /// coerces each cell against its column's Arrow type (coerce-or-skip), and
    /// applies the `Ok` cells as ONE [`ViewModel::edit_cells`] undo step + one
    /// [`Self::spawn_rebind`]. If any cells were skipped (bad coercion) or
    /// dropped (clamped past the edge), raises a paste-reject [`Banner`].
    ///
    /// No-op when no data source / view model / selection is mounted, or when
    /// the clipboard holds no string (e.g. an image).
    pub fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let grid = crate::grid::clipboard::tsv_parse(&text);
        // `tsv_parse` always returns at least one row of one cell, so a truly
        // empty (or whitespace-only single-cell) clipboard payload decodes to
        // `[[""]]` — guard against pasting that as an unintended empty-string
        // write rather than relying on `grid.is_empty()` (which never fires).
        if grid.iter().all(|row| row.iter().all(String::is_empty)) {
            return;
        }

        let (Some(ds), Some(_vm), Some(selection)) = (
            self.data_source.as_ref(),
            self.view_model.as_ref(),
            self.selection.as_ref(),
        ) else {
            return;
        };

        let anchor = selection.active();
        let max_row = usize::try_from(ds.row_count).unwrap_or(usize::MAX);
        let max_col = ds.visible_column_count();

        let mut edits: Vec<dat0_engine::CellEdit> = Vec::new();
        let mut skipped: usize = 0;
        for (dr, paste_row) in grid.iter().enumerate() {
            let row = anchor.row + dr;
            for (dc, cell) in paste_row.iter().enumerate() {
                let col = anchor.col + dc;
                // Clamp-at-edge: drop cells that fall past the grid edge.
                if row >= max_row || col >= max_col {
                    skipped += 1;
                    continue;
                }
                // Resolve screen-col → source via the ColumnView (P4c T5), then
                // key the Arrow type off that source so paste coercion uses the
                // right column's type even after a display-only reorder.
                let (Some(id), Some(column)) = (ds.row_key(row), self.column_name(col)) else {
                    // Page not cached or index out of range — can't address it.
                    skipped += 1;
                    continue;
                };
                let Some(ty) = ds.column_arrow_type_for_source(&column) else {
                    skipped += 1;
                    continue;
                };
                match crate::grid::clipboard::coerce_cell(cell, &ty) {
                    crate::grid::clipboard::CoerceResult::Ok(value) => {
                        edits.push(dat0_engine::CellEdit {
                            row: dat0_engine::RowKey::Surrogate { id },
                            column,
                            value,
                        });
                    }
                    crate::grid::clipboard::CoerceResult::Skip => skipped += 1,
                }
            }
        }

        if !edits.is_empty() {
            let change = self
                .view_model
                .as_mut()
                .expect("view_model checked above")
                .edit_cells(edits);
            self.spawn_rebind(change, cx);
            // DATA changed → structural inspector refresh (P6a T12). Only when
            // some cells were actually applied (the empty branch is a pure
            // re-render and leaves the table untouched).
            self.refresh_inspector_after_bulk_mutation(cx);
        } else {
            // Nothing applied — still re-render (e.g. to clear any prior state).
            cx.notify();
        }

        // Paste dismisses the marching-ants border (T12): once data has been
        // pasted the dashed outline no longer conveys useful information.
        self.copied_range = None;

        if skipped > 0 {
            // Structured title + body (P3b Banner shape), not a flat string. The
            // banner is `dismissible` by default (the X closes it); a paste-reject
            // has no natural primary action, so none is wired (unlike the
            // recovery banner's "Review"). Surfaced via the boot-time pending
            // queue, drained by the banner host like every other Banner.
            crate::error_ux::push(crate::error_ux::Banner::error(
                format!(
                    "{skipped} cell{} couldn't be pasted",
                    if skipped == 1 { "" } else { "s" }
                ),
                "Values that don't match the column type (or fall outside the grid) \
                 were skipped. The rest were pasted."
                    .to_string(),
            ));
        }
    }

    // ── Bulk ops: fill-down / set-null / set-value / delete-rows (T8) ──────────
    //
    // T9 wires these to the context menu and T11 wires Ctrl+D / Delete keys;
    // T8 only exposes the `pub` handlers. Each resolves the current selection
    // → ONE transform → `spawn_rebind`. Empty / unresolvable selections are
    // silent no-ops (no empty Edit/RowDelete emitted — the render layer errors
    // on those).

    /// Fill every selected cell in each column with the value of the top-most
    /// selected cell in that column (T8 Ctrl+D behaviour).
    ///
    /// For each selected column:
    ///   1. Find the minimum selected row in that column — the "source" cell.
    ///   2. Read its display string via `ds.cell_display(top_row, col)` and
    ///      coerce it through `clipboard::coerce_cell(display, column_arrow_type)`
    ///      (same coercion path as paste, so the filled value matches the type).
    ///      If coercion returns `Skip` (e.g. the top cell is NULL / empty), the
    ///      fill value is `Scalar::Null`.
    ///   3. Apply the coerced value to every *lower* selected cell in that column
    ///      (the top cell itself is NOT overwritten — fill-down starts below the
    ///      source).
    ///
    /// All columns' fills are bundled into ONE [`ViewModel::edit_cells`] call →
    /// ONE undo step. No-op when no data source / view model / selection is
    /// mounted, or when the resolved set is empty.
    pub fn fill_down(&mut self, cx: &mut Context<Self>) {
        let (Some(ds), Some(_vm)) = (self.data_source.as_ref(), self.view_model.as_ref()) else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        // Collect all resolved (row, col) pairs.
        let cells: Vec<(usize, usize)> = selection.resolved_cells().collect();
        if cells.is_empty() {
            return;
        }

        // Group cells by column; find the top row per column.
        // BTreeMap preserves column order for deterministic behaviour.
        use std::collections::BTreeMap;
        let mut col_rows: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (row, col) in &cells {
            col_rows.entry(*col).or_default().push(*row);
        }

        let mut edits: Vec<dat0_engine::CellEdit> = Vec::new();
        for (col, mut rows) in col_rows {
            rows.sort_unstable();
            let top_row = rows[0];

            // Resolve the SCREEN column → its SOURCE identity once via the
            // ColumnView (P4c T5); all the per-column reads/writes below key off
            // this source so a display-only reorder fills the right column.
            let Some(source) = self.column_name(col) else {
                continue;
            };

            // Coerce the top cell's display value into a typed Scalar.
            let fill_value = if let (Some(display), Some(arrow_type)) = (
                ds.cell_display_for_source(top_row, &source),
                ds.column_arrow_type_for_source(&source),
            ) {
                match crate::grid::clipboard::coerce_cell(&display, &arrow_type) {
                    crate::grid::clipboard::CoerceResult::Ok(v) => v,
                    // Empty / NULL display or uncoercible → fill with NULL.
                    crate::grid::clipboard::CoerceResult::Skip => dat0_engine::Scalar::Null,
                }
            } else {
                // Page not cached or col out of range for the top cell: skip column.
                continue;
            };

            // Apply fill_value to every lower selected cell in this column.
            for row in rows.into_iter().skip(1) {
                let Some(id) = ds.row_key(row) else {
                    continue;
                };
                edits.push(dat0_engine::CellEdit {
                    row: dat0_engine::RowKey::Surrogate { id },
                    column: source.clone(),
                    value: fill_value.clone(),
                });
            }
        }

        if edits.is_empty() {
            return;
        }

        let change = self
            .view_model
            .as_mut()
            .expect("view_model checked above")
            .edit_cells(edits);
        self.spawn_rebind(change, cx);
        // Fill-down DATA mutation → structural inspector refresh (P6a T12).
        self.refresh_inspector_after_bulk_mutation(cx);
    }

    /// Set every selected cell to `Scalar::Null` in ONE undo step (T8).
    ///
    /// Resolves the current selection → one [`ViewModel::edit_cells`] call
    /// → one [`Self::spawn_rebind`]. Cells whose page isn't cached (so
    /// `row_key` returns `None`) or whose column index is out of range are
    /// skipped gracefully. No-op when no data source / view model / selection
    /// is mounted or the selection resolves to nothing.
    pub fn set_null_selection(&mut self, cx: &mut Context<Self>) {
        let (Some(ds), Some(_vm)) = (self.data_source.as_ref(), self.view_model.as_ref()) else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        let mut edits: Vec<dat0_engine::CellEdit> = Vec::new();
        for (row, col) in selection.resolved_cells() {
            // Resolve screen-col → source via the ColumnView (P4c T5).
            let (Some(id), Some(column)) = (ds.row_key(row), self.column_name(col)) else {
                continue;
            };
            edits.push(dat0_engine::CellEdit {
                row: dat0_engine::RowKey::Surrogate { id },
                column,
                value: dat0_engine::Scalar::Null,
            });
        }
        if edits.is_empty() {
            return;
        }

        let change = self
            .view_model
            .as_mut()
            .expect("view_model checked above")
            .edit_cells(edits);
        self.spawn_rebind(change, cx);
        // Set-null DATA mutation → structural inspector refresh (P6a T12).
        self.refresh_inspector_after_bulk_mutation(cx);
    }

    /// Set every selected cell to `value` in ONE undo step (T8).
    ///
    /// Resolves the current selection → one [`ViewModel::edit_cells`] call
    /// → one [`Self::spawn_rebind`]. Cells whose page isn't cached (so
    /// `row_key` returns `None`) or whose column index is out of range are
    /// skipped gracefully. No-op when no data source / view model / selection
    /// is mounted or the selection resolves to nothing.
    pub fn set_value_selection(&mut self, value: dat0_engine::Scalar, cx: &mut Context<Self>) {
        let (Some(ds), Some(_vm)) = (self.data_source.as_ref(), self.view_model.as_ref()) else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        let mut edits: Vec<dat0_engine::CellEdit> = Vec::new();
        for (row, col) in selection.resolved_cells() {
            // Resolve screen-col → source via the ColumnView (P4c T5).
            let (Some(id), Some(column)) = (ds.row_key(row), self.column_name(col)) else {
                continue;
            };
            edits.push(dat0_engine::CellEdit {
                row: dat0_engine::RowKey::Surrogate { id },
                column,
                value: value.clone(),
            });
        }
        if edits.is_empty() {
            return;
        }

        let change = self
            .view_model
            .as_mut()
            .expect("view_model checked above")
            .edit_cells(edits);
        self.spawn_rebind(change, cx);
        // Set-value DATA mutation → structural inspector refresh (P6a T12).
        self.refresh_inspector_after_bulk_mutation(cx);
    }

    /// Delete the distinct rows represented in the current selection in ONE
    /// undo step (T8).
    ///
    /// Semantics: any selected cell's row is a candidate for deletion (not
    /// just full-row selections). Distinct `RowKey`s are collected across all
    /// selected cells and issued as one [`ViewModel::delete_rows`] call →
    /// ONE undo step. Cells whose page isn't cached (so `row_key` returns
    /// `None`) are skipped. No-op when no data source / view model / selection
    /// is mounted or the selection resolves to nothing.
    pub fn delete_selected_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(ds), Some(_vm)) = (self.data_source.as_ref(), self.view_model.as_ref()) else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        // Collect distinct row IDs (one per selected row regardless of how
        // many columns are selected in that row).
        use std::collections::BTreeSet;
        let mut seen_ids: BTreeSet<i64> = BTreeSet::new();
        let mut keys: Vec<dat0_engine::RowKey> = Vec::new();
        for (row, _col) in selection.resolved_cells() {
            let Some(id) = ds.row_key(row) else {
                continue;
            };
            if seen_ids.insert(id) {
                keys.push(dat0_engine::RowKey::Surrogate { id });
            }
        }
        if keys.is_empty() {
            return;
        }

        let change = self
            .view_model
            .as_mut()
            .expect("view_model checked above")
            .delete_rows(keys);
        self.spawn_rebind(change, cx);
        // Rows removed → structural inspector refresh (P6a T12).
        self.refresh_inspector_after_bulk_mutation(cx);
    }

    /// Hide a column via `DeleteColumn` (display-only; the underlying data column
    /// is unchanged so filters/sorts that reference it still compile).
    ///
    /// `col_ix` is the screen column of the right-clicked header (used as the
    /// fallback when no column selection is active). If a column selection is
    /// active, all distinct selected columns are deleted instead.
    pub fn delete_column(&mut self, col_ix: usize, cx: &mut Context<Self>) {
        let mut columns: Vec<String> = Vec::new();
        if let Some(sel) = self.selection.as_ref() {
            use std::collections::BTreeSet;
            let mut seen: BTreeSet<usize> = BTreeSet::new();
            for (_row, c) in sel.resolved_cells() {
                if seen.insert(c) {
                    if let Some(src) = self.column_name(c) {
                        columns.push(src);
                    }
                }
            }
        }
        if columns.is_empty() {
            if let Some(src) = self.column_name(col_ix) {
                columns.push(src);
            }
        }
        if columns.is_empty() {
            return;
        }
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.apply(dat0_engine::Transformation::DeleteColumn { columns });
        self.refresh_column_view();
        self.route_change(change, cx);
    }

    // ── Header-click-to-select column / row (P4c T13) ─────────────────────────

    /// Select the whole screen column `col_ix` (P4c T13).
    ///
    /// Delegates to [`crate::grid::selection::SelectionModel::select_column`],
    /// which was built and unit-tested in P4b but was unreachable from the UI
    /// until this wiring task. No-op when no selection model is mounted yet.
    pub fn select_column_at(&mut self, col_ix: usize, cx: &mut Context<Self>) {
        if let Some(sel) = self.selection.as_mut() {
            sel.select_column(col_ix);
            cx.notify();
        }
    }

    /// Select the whole screen row `row_ix`.
    ///
    /// Delegates to [`crate::grid::selection::SelectionModel::select_row`].
    /// Reachable programmatically (tests, keyboard bindings); UI row-gutter
    /// click wiring is deferred — see PD-019 in `docs/deferrals.md`.
    /// No-op when no selection model is mounted yet.
    pub fn select_row_at(&mut self, row_ix: usize, cx: &mut Context<Self>) {
        if let Some(sel) = self.selection.as_mut() {
            sel.select_row(row_ix);
            cx.notify();
        }
    }

    /// Build the bounding-rect TSV blob of the current selection's display
    /// values, recording the copied range for marching-ants. Returns `None`
    /// when no data source / selection is mounted or the selection is empty.
    ///
    /// Shared by [`Self::copy_selection`] and [`Self::cut_selection`]; the
    /// bounding rectangle spans `min..=max` row / column over every selected
    /// cell, and each gap (a coordinate in the rect not in the selection, or a
    /// cell whose page isn't cached) becomes an empty string.
    fn build_selection_tsv(&mut self) -> Option<String> {
        let cells: Vec<(usize, usize)> = self.selection.as_ref()?.resolved_cells().collect();
        let (r0, c0, r1, c1) = bounding_rect(&cells)?;

        // Pre-resolve each screen column in the bounding rect to its SOURCE name
        // via the ColumnView (P4c T5) so the copy reads the right column even
        // after a display-only reorder. Resolved up-front (indexed by screen
        // column) to keep the inner render closures free of a `&self` borrow.
        let sources: Vec<Option<String>> = (c0..=c1).map(|col| self.column_name(col)).collect();

        let ds = self.data_source.as_ref()?;
        let selection = self.selection.as_ref()?;

        let grid: Vec<Vec<String>> = (r0..=r1)
            .map(|row| {
                (c0..=c1)
                    .map(|col| {
                        if selection.contains(row, col) {
                            // `sources` is indexed by `col - c0` over the rect.
                            sources
                                .get(col - c0)
                                .and_then(Option::as_deref)
                                .and_then(|source| ds.cell_display_for_source(row, source))
                                .unwrap_or_default()
                        } else {
                            // Gap inside the bounding rect → empty cell.
                            String::new()
                        }
                    })
                    .collect()
            })
            .collect();

        self.copied_range = Some(crate::grid::selection::CellRange { r0, c0, r1, c1 });
        Some(crate::grid::clipboard::tsv_serialize(&grid))
    }
}
