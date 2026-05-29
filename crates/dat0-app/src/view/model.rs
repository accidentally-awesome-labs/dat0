//! ViewModel — one per open table tab. Owns the active Transformation stack,
//! undo cursor, and current temp-view name. Mutators are pure; engine
//! round-trips happen in the caller, driven by the returned ViewChange.

use dat0_engine::{SortKey, Transformation, compile_view_sql};

/// Maximum number of ops retained per tab. On overflow, oldest is dropped
/// and the cursor decrements to stay aligned with surviving history.
pub const HISTORY_CAP: usize = 200;

/// Per-tab state.
#[derive(Debug, Clone)]
pub struct ViewModel {
    tab_id: String,
    base_table: String, // already-quoted, e.g. "\"main\".\"orders\""
    stack: Vec<Transformation>,
    cursor: usize,               // active ops = stack[..cursor]
    active_view: Option<String>, // None when cursor == 0
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

impl ViewModel {
    pub fn new(tab_id: String, base_table: String) -> Self {
        Self {
            tab_id,
            base_table,
            stack: Vec::new(),
            cursor: 0,
            active_view: None,
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

    pub fn stack(&self) -> &[Transformation] {
        &self.stack
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn active_view(&self) -> Option<&str> {
        self.active_view.as_deref()
    }

    // --- Pure predicates ---

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.stack.len()
    }

    /// The active slice of transformations (stack[..cursor]).
    pub fn active(&self) -> &[Transformation] {
        &self.stack[..self.cursor]
    }

    // --- Mutations (each returns ViewChange for the caller to drive) ---

    /// Apply a new transformation: truncate redo history, push, bump cursor.
    /// On history overflow, drop stack[0] and decrement cursor.
    pub fn apply(&mut self, t: Transformation) -> ViewChange {
        // Drop lost-redo entries.
        self.stack.truncate(self.cursor);
        self.stack.push(t);
        self.cursor += 1;

        // Enforce history cap.
        if self.stack.len() > HISTORY_CAP {
            let overflow = self.stack.len() - HISTORY_CAP;
            self.stack.drain(0..overflow);
            self.cursor -= overflow;
        }

        self.regenerate_view()
    }

    /// Undo: decrement cursor. Returns None if already at the bottom.
    pub fn undo(&mut self) -> Option<ViewChange> {
        if !self.can_undo() {
            return None;
        }
        self.cursor -= 1;
        Some(self.regenerate_view())
    }

    /// Redo: increment cursor. Returns None if already at the top.
    ///
    /// Special case: when called from `cursor == 0` (i.e., after a `clear`),
    /// jumps directly to `stack.len()` so that the entire cleared stack is
    /// restored in a single step ("one undo restores", design §5).
    pub fn redo(&mut self) -> Option<ViewChange> {
        if !self.can_redo() {
            return None;
        }
        if self.cursor == 0 {
            self.cursor = self.stack.len();
        } else {
            self.cursor += 1;
        }
        Some(self.regenerate_view())
    }

    /// Clear all active ops (set cursor to 0). Stack is preserved so redo can
    /// restore. Returns ViewChange that rebinds to base table.
    pub fn clear(&mut self) -> ViewChange {
        self.cursor = 0;
        self.regenerate_view()
    }

    /// Replace `stack[cursor - 1]` in place (no new history entry). Used by
    /// filter-popover edit + by `set_sort` when an existing Sort op is present.
    /// No-op on the stack if cursor == 0; still returns a ViewChange.
    pub fn replace_at_cursor(&mut self, t: Transformation) -> ViewChange {
        if self.cursor > 0 {
            self.stack[self.cursor - 1] = t;
        }
        self.regenerate_view()
    }

    /// Upsert a Sort op into the active stack:
    /// - If a `Sort` exists in stack[..cursor], replace it in place (single undo step).
    /// - Otherwise, append a new `Sort` via `apply`.
    pub fn set_sort(&mut self, keys: Vec<SortKey>) -> ViewChange {
        if let Some(idx) = self.stack[..self.cursor]
            .iter()
            .position(|op| matches!(op, Transformation::Sort { .. }))
        {
            self.stack[idx] = Transformation::Sort { keys };
            self.regenerate_view()
        } else {
            self.apply(Transformation::Sort { keys })
        }
    }

    // --- Filter query helpers ---

    /// Find the most recent `Transformation::Filter` on `column` in the active
    /// stack (i.e., within `stack[..cursor]`). Returns the last (most recent)
    /// matching entry, or `None` if no filter on that column is active.
    ///
    /// Used by the filter-popover edit flow to pre-populate the popover when
    /// the user re-clicks the funnel on a column with an existing filter.
    pub fn find_filter_for(&self, column: &str) -> Option<&Transformation> {
        self.stack[..self.cursor].iter().rfind(|op| match op {
            Transformation::Filter { column: c, .. } => c == column,
            _ => false,
        })
    }

    // --- Private helpers ---

    /// Recompute active_view name + SQL given the current cursor.
    fn regenerate_view(&mut self) -> ViewChange {
        let previous = self.active_view.take();

        if self.cursor == 0 {
            // Bind to base; no view exists.
            return ViewChange {
                new_active_view: None,
                previous_active_view: previous,
                sql: None,
            };
        }

        self.nonce_seq = self.nonce_seq.wrapping_add(1);
        let new_name = format!(
            "v_{}_{}",
            sanitize_for_view_name(&self.tab_id),
            self.nonce_seq
        );

        let ops = &self.stack[..self.cursor];
        let body_sql = compile_view_sql(&self.base_table, ops)
            .expect("compile_view_sql must succeed — UI must validate before apply");

        self.active_view = Some(new_name.clone());
        ViewChange {
            new_active_view: Some(new_name),
            previous_active_view: previous,
            sql: Some(body_sql),
        }
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
