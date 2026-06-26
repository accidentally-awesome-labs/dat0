//! Theme section — choose the active theme; mention user-theme drop folder.
//!
//! P3b T11 closed D-001 by wiring the `SettingsStore` KV facade
//! (`theme.id`). P3b T12 closes D-002 by promoting
//! `crate::theme::Theme` to a `gpui::Global` and exposing a
//! [`theme_change_handler`] that does both the persisted-store write
//! and the live `Theme::switch` fan-out. The visible widget stays a
//! placeholder until T13 mounts the real `Root::new` window over
//! `SettingsView` — at that point [`theme_change_handler`] becomes
//! the `gpui_component::Select` widget's `on_change` handler (see
//! `docs/internal/gpui-component-api-notes.md` §3.6 for the `Select`
//! constructor that lands then).
//!
//! The load-bearing contract for D-001 is still the SettingsStore
//! round-trip, exercised by
//! `tests/settings_ui.rs::theme_dropdown_persists_*`. D-002's
//! contract — live fan-out via `cx.observe_global::<Theme>` — is
//! exercised by `tests/theme_live_switch.rs`.

use super::SettingsSection;
use crate::settings::store::SettingsStore;

pub struct ThemeSection;

/// Options surfaced by the (T13-mounted) Theme dropdown. Kept here
/// (rather than buried in the render closure) so T12 can import the
/// same list for the live-switch fan-out test.
pub const THEME_IDS: &[&str] = &["dark", "light", "high-contrast"];

impl ThemeSection {
    /// Closure invoked when the user picks a new theme from the
    /// (T13-mounted) dropdown. Persists the new id via the
    /// `SettingsStore` KV facade.
    ///
    /// This is the SettingsStore-only half of the dropdown's
    /// `on_change` work — kept separate from
    /// [`theme_change_handler`] so unit tests can exercise the
    /// persistence round-trip without standing up a `gpui::App`.
    /// Production callers should prefer [`theme_change_handler`],
    /// which performs both the store write AND the live
    /// `Theme::switch` fan-out.
    pub fn on_theme_change(store: &SettingsStore, new_id: &str) -> anyhow::Result<()> {
        store.set("theme.id", new_id)?;
        Ok(())
    }
}

/// Production handler for the Theme dropdown's `on_change` callback.
/// Persists the new id via [`ThemeSection::on_theme_change`] AND
/// calls [`crate::theme::Theme::switch`] so every view subscribed
/// through `cx.observe_global::<Theme>` re-renders on the next tick.
///
/// Split from [`ThemeSection::on_theme_change`] because the
/// `&mut gpui::App` argument can't be threaded through the existing
/// SettingsStore-only unit tests (`tests/settings_ui.rs`); the live
/// fan-out is exercised separately via `tests/theme_live_switch.rs`.
/// When T13 mounts the real `Select` widget this is the function
/// bound to its `on_change` handler — see
/// `docs/internal/gpui-component-api-notes.md` §3.6.
pub fn theme_change_handler(
    cx: &mut gpui::App,
    store: &SettingsStore,
    new_id: &str,
) -> anyhow::Result<()> {
    ThemeSection::on_theme_change(store, new_id)?;
    crate::theme::Theme::switch(cx, new_id);
    Ok(())
}

impl SettingsSection for ThemeSection {
    fn name_key(&self) -> &'static str {
        "settings.theme"
    }

    fn id(&self) -> &'static str {
        "theme"
    }
}
