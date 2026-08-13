//! Read-once helper that converts the persisted `memory_budget_mb` to bytes
//! for the engine constructor. Used at every window-open site so the
//! hardcoded 1 GiB literal lives in exactly one place.

use crate::settings::store::SettingsStore;

/// The 1 GiB fallback as consts, so a caller that cannot even construct a
/// `SettingsStore` (no config dir) lands on the same number instead of
/// duplicating the literal this module's own doc says lives here alone.
pub const DEFAULT_MEMORY_BUDGET_MB: u32 = 1024;
pub const DEFAULT_MEMORY_BUDGET_BYTES: u64 = DEFAULT_MEMORY_BUDGET_MB as u64 * 1024 * 1024;

/// Bytes for `MemoryBudget`, read from the persisted setting (falls back to
/// the 1024 MB default on any load error — never panics at window open).
pub fn memory_budget_bytes(store: &SettingsStore) -> u64 {
    let mb = store
        .load_or_default()
        .map(|s| s.memory_budget_mb)
        .unwrap_or(DEFAULT_MEMORY_BUDGET_MB);
    mb as u64 * 1024 * 1024
}

/// The configured memory budget, resolved from `settings.toml` in the OS config
/// directory.
///
/// Read on every call rather than cached, so a new window picks up an edited
/// setting without an app restart.
///
/// Degrades rather than panics: this used to be `.expect("config dir")` on a
/// path that runs on EVERY window spawn. `memory_budget_bytes` already falls
/// back to the default on a missing or unparseable file, so an unresolvable
/// config dir routes into the same fallback — and raises a banner, because a
/// silently ignored memory setting is its own bug report.
pub fn configured() -> u64 {
    let Ok(dir) = crate::platform::config_dir() else {
        tracing::warn!("config_dir unavailable; using the default memory budget");
        crate::error_ux::push(crate::error_ux::Banner::warning(dat0_i18n::t(
            "settings.config_dir_unavailable",
        )));
        return DEFAULT_MEMORY_BUDGET_BYTES;
    };
    let store = crate::settings::store::SettingsStore::with_path(dir.join("settings.toml"));
    memory_budget_bytes(&store)
}
