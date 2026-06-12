//! ViewModel — one per open table tab. Owns the active Transformation stack
//! (a past/present/future undo zipper) and current temp-view name. Mutators are
//! pure; engine round-trips happen in the caller, driven by the returned
//! ViewChange.

use dat0_engine::{SortKey, Transformation, compile_view_sql};

use crate::view::filter_popover_entity::Outcome;
use crate::view::sort_header::ActiveSort;

/// Maximum number of undo snapshots retained per tab (bounds `past`). On
/// overflow, the oldest snapshot is evicted.
pub const HISTORY_CAP: usize = 200;

/// Per-tab state.
#[derive(Debug, Clone)]
pub struct ViewModel {
    tab_id: String,
    base_table: String, // already-quoted, e.g. "\"main\".\"orders\""
    /// The active transform stack (what the view shows). In the zipper this is
    /// the "present"; there is no in-stack redo tail (redo lives in `future`).
    present: Vec<Transformation>,
    /// Undo snapshots (most-recent last), bounded to HISTORY_CAP.
    past: Vec<Vec<Transformation>>,
    /// Redo snapshots (most-recent-undone last).
    future: Vec<Vec<Transformation>>,
    active_view: Option<String>,
    /// Data SQL that produced `active_view` — lets `regenerate_view` detect a
    /// display-only change (projection op) and skip the engine round-trip.
    active_view_sql: Option<String>,
    nonce_seq: u32,
}

/// One side-effect bundle for the caller to apply (engine round-trip + grid rebind).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewChange {
    /// View name the grid should rebind to. `None` = rebind to base table.
    pub new_active_view: Option<String>,
    /// Previous view name to drop after rebind completes (avoids stale page reads).
    pub previous_active_view: Option<String>,
    /// SQL to feed `engine.create_or_replace_view(new_active_view, sql)`.
    /// `None` when new_active_view is None (base-table bind needs no view).
    pub sql: Option<String>,
}

impl ViewChange {
    /// A change that needs no engine round-trip: the data SQL is unchanged
    /// (a projection op) but the visible columns/header changed. The caller
    /// refreshes the grid `ColumnView` + re-renders, without rebinding a view.
    pub fn is_display_only(&self) -> bool {
        self.new_active_view.is_some() && self.sql.is_none()
    }
}

impl ViewModel {
    pub fn new(tab_id: String, base_table: String) -> Self {
        Self {
            tab_id,
            base_table,
            present: Vec::new(),
            past: Vec::new(),
            future: Vec::new(),
            active_view: None,
            active_view_sql: None,
            nonce_seq: 0,
        }
    }

    // --- Accessors ---

    pub fn tab_id(&self) -> &str {
        &self.tab_id
    }

    pub fn base_table(&self) -> &str {
        &self.base_table
    }

    /// The active transform stack (alias of `active()`; kept for the session
    /// layer + tests that persisted the full stack pre-P4c).
    pub fn stack(&self) -> &[Transformation] {
        &self.present
    }

    /// Active op count (== `active().len()`; replaces the old `cursor`).
    pub fn cursor(&self) -> usize {
        self.present.len()
    }

    pub fn active_view(&self) -> Option<&str> {
        self.active_view.as_deref()
    }

    // --- Pure predicates ---

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// The active slice of transformations (== `stack()` in the zipper model).
    pub fn active(&self) -> &[Transformation] {
        &self.present
    }

    // --- Mutations (each returns ViewChange for the caller to drive) ---

    /// Snapshot `present` onto the undo stack and clear redo. Called by every
    /// structural edit (apply / jump_to / remove_at / clear).
    fn checkpoint(&mut self) {
        self.past.push(self.present.clone());
        if self.past.len() > HISTORY_CAP {
            let overflow = self.past.len() - HISTORY_CAP;
            self.past.drain(0..overflow);
        }
        self.future.clear();
    }

    /// Apply a new transformation as one undo step.
    pub fn apply(&mut self, t: Transformation) -> ViewChange {
        self.checkpoint();
        self.present.push(t);
        self.regenerate_view()
    }

    /// Apply a cell-edit transform (T6 — inline cell editor / bulk edits).
    /// Wraps `cells` in a [`Transformation::Edit`] and pushes it as a normal
    /// undo step via [`Self::apply`]. Each call is one undo step.
    pub fn edit_cells(&mut self, cells: Vec<dat0_engine::CellEdit>) -> ViewChange {
        self.apply(dat0_engine::Transformation::Edit { cells })
    }

    /// Apply a row-delete transform (T6/T8). Wraps `rows` in a
    /// [`Transformation::RowDelete`] and pushes it as one undo step.
    pub fn delete_rows(&mut self, rows: Vec<dat0_engine::RowKey>) -> ViewChange {
        self.apply(dat0_engine::Transformation::RowDelete { rows })
    }

    /// Whether the active stack carries any in-place edits (`Edit` /
    /// `RowDelete`). Used by the tab-strip dirty indicator (T10). Filter/sort
    /// ops are not "dirty" — only mutations of the underlying data are.
    pub fn is_dirty(&self) -> bool {
        self.active().iter().any(|t| {
            matches!(
                t,
                dat0_engine::Transformation::Edit { .. }
                    | dat0_engine::Transformation::RowDelete { .. }
            )
        })
    }

    /// Undo: restore the previous snapshot. None if nothing to undo.
    pub fn undo(&mut self) -> Option<ViewChange> {
        let prev = self.past.pop()?;
        self.future.push(std::mem::replace(&mut self.present, prev));
        Some(self.regenerate_view())
    }

    /// Redo: re-apply the next snapshot. None if nothing to redo.
    pub fn redo(&mut self) -> Option<ViewChange> {
        let next = self.future.pop()?;
        self.past.push(std::mem::replace(&mut self.present, next));
        Some(self.regenerate_view())
    }

    /// Clear all active ops (undoable). Rebinds to the base table.
    pub fn clear(&mut self) -> ViewChange {
        if self.present.is_empty() {
            return self.regenerate_view();
        }
        self.checkpoint();
        self.present.clear();
        self.regenerate_view()
    }

    /// PipelineBar scrubber: keep the first `k` ops (0..=len), as one undo step.
    pub fn jump_to(&mut self, k: usize) -> ViewChange {
        let k = k.min(self.present.len());
        if k == self.present.len() {
            return self.regenerate_view();
        }
        self.checkpoint();
        self.present.truncate(k);
        self.regenerate_view()
    }

    /// PipelineBar per-transform remove: drop `present[i]`, as one undo step.
    pub fn remove_at(&mut self, i: usize) -> ViewChange {
        if i >= self.present.len() {
            return self.regenerate_view();
        }
        self.checkpoint();
        self.present.remove(i);
        self.regenerate_view()
    }

    /// Replace the top active op (`present[len - 1]`) in place (no new history
    /// entry). Used by filter-popover edit + by `set_sort` when an existing
    /// Sort op is present. No-op on the stack if `present` is empty; still
    /// returns a ViewChange. In the zipper this is not a discrete undo step, so
    /// it pushes nothing onto `past`; `future` is cleared to keep redo
    /// consistent with the mutated present.
    pub fn replace_at_cursor(&mut self, t: Transformation) -> ViewChange {
        if let Some(last) = self.present.last_mut() {
            *last = t;
            self.future.clear();
        }
        self.regenerate_view()
    }

    /// Upsert a Sort op into the active stack:
    /// - If a `Sort` exists in `present`, replace it in place (not a discrete
    ///   undo step: no `past` push, but clear `future` to keep redo consistent).
    /// - Otherwise, append a new `Sort` via `apply`.
    pub fn set_sort(&mut self, keys: Vec<SortKey>) -> ViewChange {
        if let Some(idx) = self
            .present
            .iter()
            .position(|op| matches!(op, Transformation::Sort { .. }))
        {
            self.present[idx] = Transformation::Sort { keys };
            self.future.clear();
            self.regenerate_view()
        } else {
            self.apply(Transformation::Sort { keys })
        }
    }

    /// Upsert a `Filter` op into the active stack, keyed by *column*:
    /// - If a `Filter` on the SAME column exists in `present`, replace it
    ///   in place (single undo step, no new history entry).
    /// - Otherwise, append the filter via `apply`.
    ///
    /// This is the filter-popover edit-apply path (mirrors [`Self::set_sort`]).
    /// Unlike [`Self::replace_at_cursor`] (which replaces only the TOP entry),
    /// this finds the existing filter wherever it sits in the active slice — so
    /// re-editing a column whose filter is buried under a later sort/filter
    /// replaces the right entry instead of stacking a second predicate.
    ///
    /// Guard: only a `Transformation::Filter` carries a column to key on. If a
    /// non-Filter is passed (not expected from the popover), fall back to
    /// `apply` so the op is never silently dropped.
    pub fn set_filter(&mut self, t: Transformation) -> ViewChange {
        let Transformation::Filter { column, .. } = &t else {
            debug_assert!(false, "set_filter called with a non-Filter transformation");
            return self.apply(t);
        };
        let column = column.clone();
        if let Some(idx) = self
            .present
            .iter()
            .position(|op| matches!(op, Transformation::Filter { column: c, .. } if *c == column))
        {
            // In-place replace: not a discrete undo step (no `past` push), but
            // clear `future` to keep the redo stack consistent with the present.
            self.present[idx] = t;
            self.future.clear();
            self.regenerate_view()
        } else {
            self.apply(t)
        }
    }

    /// Replace the active stack with `ops` and force a full grid rebind, used by
    /// live re-import refresh (P7c). Unlike `apply`/`jump_to`, this ALWAYS emits
    /// a data-rebinding [`ViewChange`] (never the display-only fast path) because
    /// the underlying base-table data changed even when the compiled SQL is
    /// identical — so [`Self::regenerate_view`]'s SQL-equality short-circuit must
    /// not skip the engine round-trip.
    ///
    /// Clears undo/redo history: the re-imported base is a new data epoch, so the
    /// pre-refresh history (which may reference dropped `__dat0_rowid` edits) is
    /// no longer meaningful.
    pub fn reset_to_replayed(&mut self, ops: Vec<Transformation>) -> ViewChange {
        self.past.clear();
        self.future.clear();
        self.present = ops;
        // Force `regenerate_view` off its display-only fast path: the cached SQL
        // may be byte-identical to the new base's SQL, but the DATA changed, so
        // the grid must re-read. Clearing the cached SQL makes the equality guard
        // (`active_view_sql == Some(body_sql)`) fail → a fresh view name + Some(sql).
        self.active_view_sql = None;
        self.regenerate_view()
    }

    // --- Sort query helpers ---

    /// Return the current Sort op (if any) as an [`ActiveSort`] for the header
    /// click handler to mutate.
    ///
    /// Scans `present` (the active stack) in reverse and returns the first
    /// `Transformation::Sort` found. If no Sort op is active, returns an
    /// empty `ActiveSort`.
    ///
    /// Used by the grid sort-zone click handler (T12): read current state →
    /// mutate via `ActiveSort::click` / `shift_click` → call `set_sort`.
    pub fn current_sort_as_active(&self) -> ActiveSort {
        let keys = self
            .present
            .iter()
            .rev()
            .find_map(|op| match op {
                Transformation::Sort { keys } => Some(keys.clone()),
                _ => None,
            })
            .unwrap_or_default();
        ActiveSort::new(keys)
    }

    // --- Filter query helpers ---

    /// Find the most recent `Transformation::Filter` on `column` in the active
    /// stack (i.e., within `present`). Returns the last (most recent) matching
    /// entry, or `None` if no filter on that column is active.
    ///
    /// Used by the filter-popover edit flow to pre-populate the popover when
    /// the user re-clicks the funnel on a column with an existing filter.
    pub fn find_filter_for(&self, column: &str) -> Option<&Transformation> {
        self.present.iter().rfind(|op| match op {
            Transformation::Filter { column: c, .. } => c == column,
            _ => false,
        })
    }

    // --- Private helpers ---

    /// Recompute active_view name + SQL from the current `present` stack.
    ///
    /// Display-only fast path: when the recompiled data SQL is byte-identical to
    /// the SQL backing the current `active_view` (a projection op, or an
    /// undo/redo that recompiles to the same SQL), keep the view and emit a
    /// `ViewChange` with `sql: None` so the caller refreshes headers without an
    /// engine round-trip. That change carries `previous_active_view: None`
    /// because the view is unchanged — nothing must be dropped.
    fn regenerate_view(&mut self) -> ViewChange {
        let previous = self.active_view.clone();

        if self.present.is_empty() {
            self.active_view = None;
            self.active_view_sql = None;
            return ViewChange {
                new_active_view: None,
                previous_active_view: previous,
                sql: None,
            };
        }

        let body_sql = compile_view_sql(&self.base_table, &self.present)
            .expect("compile_view_sql must succeed — UI must validate before apply");

        // Display-only fast path: data SQL unchanged (a projection op, or an
        // undo/redo whose new stack recompiles to identical SQL) → keep the
        // current view, signal the caller to refresh headers without an engine
        // hop. `previous_active_view` is None because the view stays bound to the
        // same SQL: nothing is created and nothing must be dropped.
        if self.active_view.is_some() && self.active_view_sql.as_deref() == Some(body_sql.as_str())
        {
            return ViewChange {
                new_active_view: self.active_view.clone(),
                previous_active_view: None,
                sql: None,
            };
        }

        self.nonce_seq = self.nonce_seq.wrapping_add(1);
        let new_name = format!(
            "v_{}_{}",
            sanitize_for_view_name(&self.tab_id),
            self.nonce_seq
        );
        self.active_view = Some(new_name.clone());
        self.active_view_sql = Some(body_sql.clone());
        ViewChange {
            new_active_view: Some(new_name),
            previous_active_view: previous,
            sql: Some(body_sql),
        }
    }
}

/// Pure outcome→[`ViewChange`] decision for the filter popover (T0 / PD-016).
///
/// This is the single source of truth for routing a popover [`Outcome`] into
/// the ViewModel. Both `WorkspaceShell::route_filter_outcome` (which then drives
/// the GPUI engine round-trip on the returned `Some(change)`) and the
/// `click_wiring` integration test call this function, so the test exercises
/// production routing rather than a duplicate match.
///
/// - `Apply(t)` → [`ViewModel::set_filter`] (column-aware upsert: replaces an
///   existing filter on the same column, else appends — correct for both the
///   new-filter and edit-existing flows).
/// - `Clear { pre_populated: true }` → [`ViewModel::clear`].
/// - `Clear { pre_populated: false }` / `Cancel` → no ViewChange.
pub fn route_outcome(vm: &mut ViewModel, outcome: Outcome) -> Option<ViewChange> {
    match outcome {
        Outcome::Apply(t) => Some(vm.set_filter(t)),
        Outcome::Clear {
            pre_populated: true,
        } => Some(vm.clear()),
        Outcome::Clear {
            pre_populated: false,
        }
        | Outcome::Cancel => None,
    }
}

/// Sanitize an arbitrary tab id into a fragment safe for a DuckDB identifier
/// (alphanumeric + underscore). Non-matching chars are stripped, not escaped,
/// to keep view names short and predictable.
fn sanitize_for_view_name(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{FilterOp, FilterValue, Scalar};

    /// A `qty > 0` filter, the canonical structural op used across these tests.
    fn qty_gt_zero() -> Transformation {
        Transformation::Filter {
            column: "qty".into(),
            op: FilterOp::Gt,
            value: FilterValue::Scalar {
                value: Scalar::Float(0.0),
            },
        }
    }

    #[test]
    fn reset_to_replayed_forces_rebind_even_when_sql_matches() {
        let mut vm = ViewModel::new("orders".into(), "\"main\".\"orders\"".into());
        // Apply a filter so there's an active view + cached SQL.
        let f = qty_gt_zero();
        let _ = vm.apply(f.clone());
        assert!(vm.active_view().is_some(), "filter produced a view");

        // Re-import refreshed the base under the SAME filter stack. Replay must
        // emit a NON-display-only change (sql Some + a fresh view name) so the
        // grid re-reads the new data even though the compiled SQL is identical.
        let change = vm.reset_to_replayed(vec![f]);
        assert!(
            change.sql.is_some(),
            "must force a real rebind, not display-only"
        );
        assert!(!change.is_display_only());
        assert_eq!(vm.stack().len(), 1);
    }

    #[test]
    fn reset_to_replayed_empty_binds_base() {
        let mut vm = ViewModel::new("orders".into(), "\"main\".\"orders\"".into());
        let _ = vm.apply(qty_gt_zero());
        let change = vm.reset_to_replayed(vec![]);
        assert_eq!(vm.stack().len(), 0);
        assert!(
            change.new_active_view.is_none(),
            "empty stack rebinds to base"
        );
    }
}
