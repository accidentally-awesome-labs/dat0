//! Settings → Workspace section (design §"Settings → Workspace section").
//! v1 surface: a "treat all workspaces as networked" toggle + a read-only view
//! of the force-on path list. Per-path add/remove UI is v1.x.
//!
//! T4 landed the store-only persistence handler + its unit test.
//! The `impl SettingsSection` (render) and `all_sections()` registration
//! land here in T5.

use super::SettingsSection;
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

impl SettingsSection for WorkspaceSection {
    fn name_key(&self) -> &'static str {
        "settings.workspace"
    }

    fn id(&self) -> &'static str {
        "workspace"
    }
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
