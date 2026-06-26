//! Profile section — author identity used when creating `.dat0` packages.
//!
//! P3b T11 closes D-001 by wiring the `SettingsStore` KV facade
//! (`author.name`, `author.email`). The visible widget stays a
//! placeholder until T13 mounts the real `Root::new` window over the
//! `SettingsView` — at that point the closures sketched in
//! [`on_name_change`] / [`on_email_change`] become the live
//! `gpui_component` `InputState::on_change` handlers (see
//! `docs/internal/gpui-component-api-notes.md` §3 for the `Input`
//! constructor that lands then).
//!
//! The load-bearing contract for D-001 is the SettingsStore round-trip
//! itself, exercised by `tests/settings_ui.rs::profile_*_persists_*`.

use super::SettingsSection;
use crate::settings::store::SettingsStore;

pub struct ProfileSection;

impl ProfileSection {
    /// Closure used by the (T13-mounted) Name input's `on_change` hook.
    /// Extracted as a free function so the wiring is type-checked + test
    /// covered today: T11 asserts the KV round-trip in
    /// `tests/settings_ui.rs`. The signature matches the
    /// `Fn(&SettingsStore, &str) -> anyhow::Result<()>` shape the
    /// `gpui-component` input widget will adopt at T13.
    pub fn on_name_change(store: &SettingsStore, value: &str) -> anyhow::Result<()> {
        store.set("author.name", value)
    }

    /// Closure used by the (T13-mounted) Email input's `on_change` hook.
    /// See [`on_name_change`] for rationale.
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
