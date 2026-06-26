//! Profile section — author identity used when creating `.dat0` packages.
//!
//! Wires the `SettingsStore` KV facade (`author.name`, `author.email`).
//! The Name/Email inputs are mounted in `SettingsPanel` (panel.rs) and
//! call [`on_name_change`] / [`on_email_change`] on every render tick via
//! `persist_inputs`. The load-bearing contract is the SettingsStore
//! round-trip, exercised by `tests/settings_ui.rs::profile_*_persists_*`.

use super::SettingsSection;
use crate::settings::store::SettingsStore;

pub struct ProfileSection;

impl ProfileSection {
    /// Persist the Name input value via the `SettingsStore` KV facade.
    /// Called by `SettingsPanel::persist_inputs` on each render tick.
    pub fn on_name_change(store: &SettingsStore, value: &str) -> anyhow::Result<()> {
        store.set("author.name", value)
    }

    /// Persist the Email input value via the `SettingsStore` KV facade.
    /// Called by `SettingsPanel::persist_inputs` on each render tick.
    pub fn on_email_change(store: &SettingsStore, value: &str) -> anyhow::Result<()> {
        store.set("author.email", value)
    }
}

impl SettingsSection for ProfileSection {
    fn name_key(&self) -> &'static str {
        "settings.profile"
    }

    fn id(&self) -> &'static str {
        "profile"
    }
}
