//! Settings → Updates section.
//!
//! Surfaces the `update_auto_check` toggle so users can opt out of the
//! launch-time update check introduced by P10a-2 T6.
//!
//! Follows the same pattern as `workspace.rs` (store-only persistence
//! handler + placeholder render; real widget binding deferred to when
//! the settings window is fully wired).

use super::SettingsSection;
use crate::settings::store::SettingsStore;

pub struct UpdatesSection;

/// Persist the `update_auto_check` toggle via the atomic settings write path.
/// Store-only half (no GPUI) — unit-testable like `set_treat_all_as_networked`.
pub fn set_update_auto_check(store: &SettingsStore, value: bool) -> anyhow::Result<()> {
    let mut settings = store.load_or_default()?;
    settings.update_auto_check = value;
    store.save(&settings)
}

impl SettingsSection for UpdatesSection {
    fn name_key(&self) -> &'static str {
        "settings.updates"
    }

    fn id(&self) -> &'static str {
        "updates"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_round_trips_through_store() {
        let store = SettingsStore::open_in_memory();
        // default is true
        assert!(store.load_or_default().unwrap().update_auto_check);
        set_update_auto_check(&store, false).unwrap();
        assert!(!store.load_or_default().unwrap().update_auto_check);
        set_update_auto_check(&store, true).unwrap();
        assert!(store.load_or_default().unwrap().update_auto_check);
    }
}
