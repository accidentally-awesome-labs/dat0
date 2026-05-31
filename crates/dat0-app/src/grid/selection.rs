//! Ephemeral grid selection state (design §7). Pure-logic, never persisted.
//! Screen-space coordinates over the active (filtered/sorted) view; resolved
//! to RowKey at copy/edit time via the hidden key column (see grid::data_source).

/// A (row, col) coordinate in screen space (zero-based, over the active view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellCoord {
    pub row: usize,
    pub col: usize,
}

/// An inclusive rectangular range of cells in screen space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRange {
    pub r0: usize,
    pub c0: usize,
    pub r1: usize,
    pub c1: usize,
}

impl CellRange {
    fn contains(&self, r: usize, c: usize) -> bool {
        r >= self.r0.min(self.r1)
            && r <= self.r0.max(self.r1)
            && c >= self.c0.min(self.c1)
            && c <= self.c0.max(self.c1)
    }
}

/// Ephemeral, screen-space grid selection.
///
/// Supports:
/// - Single-click (`click`) — clears selection, sets a single-cell range.
/// - Cmd/Ctrl-click (`add_click`) — appends a new single-cell range (discontiguous).
/// - Shift-extend (`extend_to`) — stretches the last range from the anchor to the
///   given coord (like a typical spreadsheet Shift+click).
/// - Row / column / all-cell selection helpers.
/// - `move_active` / `extend_active` — keyboard navigation (T11 wires keys → these).
///
/// ## Zero-row / zero-col invariant
/// `SelectionModel::new(0, _)` or `new(_, 0)` is treated as an empty grid.
/// `move_active` / `extend_active` guard against the `rows as isize - 1 == -1`
/// panic by clamping to `(0, 0)` when the grid has no rows/cols.
/// A `debug_assert!` in `new` documents that callers are expected to construct
/// models only for non-empty grids; the code is still correct for 0×0.
#[derive(Debug, Clone)]
pub struct SelectionModel {
    rows: usize,
    cols: usize,
    ranges: Vec<CellRange>,
    anchor: CellCoord,
    active: CellCoord,
}

impl SelectionModel {
    /// Create a new, empty selection over a grid with `rows` rows and `cols` columns.
    ///
    /// `rows` and `cols` are used only for clamping in `move_active` /
    /// `extend_active`. A grid with zero rows or columns is legal (the model just
    /// won't be able to select anything), but in practice callers should only
    /// construct a `SelectionModel` after they have live data dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        debug_assert!(
            rows > 0 && cols > 0,
            "SelectionModel expects a non-empty grid"
        );
        let z = CellCoord { row: 0, col: 0 };
        Self {
            rows,
            cols,
            ranges: Vec::new(),
            anchor: z,
            active: z,
        }
    }

    /// The "active" (cursor) cell — the moving end of the last range.
    pub fn active(&self) -> CellCoord {
        self.active
    }

    /// Remove all ranges.  Anchor and active are left in place (they are
    /// meaningless without ranges but harmless to keep).
    pub fn clear(&mut self) {
        self.ranges.clear();
    }

    /// Single-click: clear all ranges and start a new single-cell selection.
    /// Sets both the anchor and active to `c`.
    pub fn click(&mut self, c: CellCoord) {
        self.ranges.clear();
        self.anchor = c;
        self.active = c;
        self.ranges.push(CellRange {
            r0: c.row,
            c0: c.col,
            r1: c.row,
            c1: c.col,
        });
    }

    /// Cmd/Ctrl+click: add a new discontiguous single-cell range without clearing
    /// existing ranges.  The anchor for the next `extend_to` is moved to `c`.
    pub fn add_click(&mut self, c: CellCoord) {
        self.anchor = c;
        self.active = c;
        self.ranges.push(CellRange {
            r0: c.row,
            c0: c.col,
            r1: c.row,
            c1: c.col,
        });
    }

    /// Shift-extend: replace the *last* range with a rectangle from the current
    /// anchor to `c`.  All earlier ranges are untouched.
    pub fn extend_to(&mut self, c: CellCoord) {
        self.active = c;
        if let Some(last) = self.ranges.last_mut() {
            *last = CellRange {
                r0: self.anchor.row,
                c0: self.anchor.col,
                r1: c.row,
                c1: c.col,
            };
        } else {
            self.ranges.push(CellRange {
                r0: self.anchor.row,
                c0: self.anchor.col,
                r1: c.row,
                c1: c.col,
            });
        }
    }

    /// Select all cells in row `r`.
    pub fn select_row(&mut self, r: usize) {
        self.click(CellCoord { row: r, col: 0 });
        if let Some(last) = self.ranges.last_mut() {
            last.c1 = self.cols.saturating_sub(1);
        }
    }

    /// Select all cells in column `c`.
    pub fn select_column(&mut self, c: usize) {
        self.click(CellCoord { row: 0, col: c });
        if let Some(last) = self.ranges.last_mut() {
            last.r1 = self.rows.saturating_sub(1);
        }
    }

    /// Select every cell in the grid.
    pub fn select_all(&mut self) {
        self.ranges.clear();
        self.ranges.push(CellRange {
            r0: 0,
            c0: 0,
            r1: self.rows.saturating_sub(1),
            c1: self.cols.saturating_sub(1),
        });
    }

    /// Returns `true` if `(r, c)` is covered by any range.
    pub fn contains(&self, r: usize, c: usize) -> bool {
        self.ranges.iter().any(|x| x.contains(r, c))
    }

    /// Deduped `(row, col)` pairs across all ranges, in row-major order.
    ///
    /// Uses a `BTreeSet` for deterministic ordering and deduplication;
    /// acceptable because selection sizes are always small (screen-space).
    pub fn resolved_cells(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let mut set = std::collections::BTreeSet::new();
        for rg in &self.ranges {
            for r in rg.r0.min(rg.r1)..=rg.r0.max(rg.r1) {
                for c in rg.c0.min(rg.c1)..=rg.c0.max(rg.c1) {
                    set.insert((r, c));
                }
            }
        }
        set.into_iter()
    }

    /// Move the active cell by `(dr, dc)`, clamping to grid bounds, and set a
    /// new single-cell selection at the result.  Analogous to arrow-key navigation.
    ///
    /// Safe for zero-row/zero-col grids: clamps to `(0, 0)` in that case.
    pub fn move_active(&mut self, dr: isize, dc: isize) {
        let max_r = self.rows.saturating_sub(1) as isize;
        let max_c = self.cols.saturating_sub(1) as isize;
        let r = (self.active.row as isize + dr).clamp(0, max_r) as usize;
        let c = (self.active.col as isize + dc).clamp(0, max_c) as usize;
        self.click(CellCoord { row: r, col: c });
    }

    /// Extend the active cell by `(dr, dc)`, clamping to grid bounds, without
    /// clearing previous ranges.  Analogous to Shift+arrow-key navigation.
    ///
    /// Safe for zero-row/zero-col grids: clamps to `(0, 0)` in that case.
    pub fn extend_active(&mut self, dr: isize, dc: isize) {
        let max_r = self.rows.saturating_sub(1) as isize;
        let max_c = self.cols.saturating_sub(1) as isize;
        let r = (self.active.row as isize + dr).clamp(0, max_r) as usize;
        let c = (self.active.col as isize + dc).clamp(0, max_c) as usize;
        self.extend_to(CellCoord { row: r, col: c });
    }
}
