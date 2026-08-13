//! Sort-zone state machine: click cycle (none → asc → desc → none),
//! shift-click append/cycle within existing sort, rank shift on removal.

use dat0_engine::{SortDirection, SortKey};

#[derive(Debug, Default, Clone)]
pub struct ActiveSort {
    pub keys: Vec<SortKey>,
}

impl ActiveSort {
    pub fn new(keys: Vec<SortKey>) -> Self {
        Self { keys }
    }

    /// Borrow the current sort keys as a slice.
    ///
    /// Used by the grid sort-zone click handler (T0 / PD-016) to feed the
    /// cycled keys into [`crate::view::ViewModel::set_sort`]:
    /// `vm.set_sort(active.keys().to_vec())`.
    ///
    /// (A method, not just the `pub keys` field, so call sites read as
    /// `active.keys()` symmetric with the rest of the read API.)
    pub fn keys(&self) -> &[SortKey] {
        &self.keys
    }

    /// Find (1-based rank, direction) for `column`, or None if absent.
    pub fn find(&self, column: &str) -> Option<(usize, SortDirection)> {
        self.keys
            .iter()
            .enumerate()
            .find(|(_, k)| k.column == column)
            .map(|(i, k)| (i + 1, k.direction))
    }

    /// Click (no shift): this column becomes the sole sort, cycling
    /// `none → asc → desc → none`.
    pub fn click(mut self, column: &str) -> Self {
        let next_dir = match self.find(column).map(|(_, d)| d) {
            None => Some(SortDirection::Asc),
            Some(SortDirection::Asc) => Some(SortDirection::Desc),
            Some(SortDirection::Desc) => None,
        };
        self.keys = match next_dir {
            Some(d) => vec![SortKey {
                column: column.to_string(),
                direction: d,
            }],
            None => Vec::new(),
        };
        self
    }

    /// Shift+Click: append column if absent (asc); else cycle within the
    /// column's current rank (`asc → desc → remove`). Removal shifts later
    /// ranks up.
    pub fn shift_click(mut self, column: &str) -> Self {
        match self.keys.iter().position(|k| k.column == column) {
            None => {
                self.keys.push(SortKey {
                    column: column.to_string(),
                    direction: SortDirection::Asc,
                });
            }
            Some(idx) => {
                let new_dir = match self.keys[idx].direction {
                    SortDirection::Asc => Some(SortDirection::Desc),
                    SortDirection::Desc => None,
                };
                match new_dir {
                    Some(d) => {
                        self.keys[idx].direction = d;
                    }
                    None => {
                        self.keys.remove(idx);
                        // Later ranks shift up automatically via Vec::remove.
                    }
                }
            }
        }
        self
    }
}
