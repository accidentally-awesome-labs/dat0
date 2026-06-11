//! Settings → Workspace section (design §"Settings → Workspace section").
//! v1 surface: a "treat all workspaces as networked" toggle + a read-only view
//! of the force-on path list. Per-path add/remove UI is v1.x.
//!
//! This task (T4) lands only the store-only persistence handler + its unit
//! test. The `impl SettingsSection` (render) and registration into
//! `all_sections()` arrive in T5.

use crate::settings::store::SettingsStore;

pub struct WorkspaceSection;

/// Persist the global "treat all as networked" toggle via the atomic settings
/// write path. Store-only half (no GPUI), unit-testable like
/// `ThemeSection::on_theme_change`.
pub fn set_treat_all_as_networked(store: &SettingsStore, value: bool) -> anyhow::Result<()> {
    let mut settings = store.load_or_default()?;
    settings.workspace.treat_all_as_networked = value;
    store.save(&settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_round_trips_through_store() {
        let store = SettingsStore::open_in_memory();
        assert!(
            !store
                .load_or_default()
                .unwrap()
                .workspace
                .treat_all_as_networked
        );
        set_treat_all_as_networked(&store, true).unwrap();
        assert!(
            store
                .load_or_default()
                .unwrap()
                .workspace
                .treat_all_as_networked
        );
        set_treat_all_as_networked(&store, false).unwrap();
        assert!(
            !store
                .load_or_default()
                .unwrap()
                .workspace
                .treat_all_as_networked
        );
    }
}
