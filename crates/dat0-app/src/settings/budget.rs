//! Read-once helper that converts the persisted `memory_budget_mb` to bytes
//! for the engine constructor. Used at every window-open site so the
//! hardcoded 1 GiB literal lives in exactly one place.

use crate::settings::store::SettingsStore;

/// Bytes for `MemoryBudget`, read from the persisted setting (falls back to
/// the 1024 MB default on any load error — never panics at window open).
pub fn memory_budget_bytes(store: &SettingsStore) -> u64 {
    let mb = store
        .load_or_default()
        .map(|s| s.memory_budget_mb)
        .unwrap_or(1024);
    mb as u64 * 1024 * 1024
}
