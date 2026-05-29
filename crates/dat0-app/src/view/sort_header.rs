//! Sort-zone state machine (cycle + shift-click multi-sort). Full
//! implementation lands in T12; T9 ships the type names so the header
//! renderer can reference them.

use dat0_engine::{SortDirection, SortKey};

/// Active sort state per column, kept alongside the ViewModel.
/// Indexed by column name → (rank, direction). rank is 1-based.
#[derive(Debug, Default, Clone)]
pub struct ActiveSort {
    pub keys: Vec<SortKey>,
}

impl ActiveSort {
    pub fn find(&self, column: &str) -> Option<(usize, SortDirection)> {
        self.keys
            .iter()
            .enumerate()
            .find(|(_, k)| k.column == column)
            .map(|(i, k)| (i + 1, k.direction))
    }
}
