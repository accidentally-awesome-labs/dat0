//! Theme section — choose the active theme.
//!
//! Wires the `SettingsStore` KV facade (`theme.id`) and exposes
//! [`theme_change_handler`] which does both the persisted-store write
//! and the live `Theme::switch` fan-out. The cycle button in
//! `SettingsPanel` (panel.rs) calls `Theme::switch` directly.
//!
//! Round-trip: `tests/settings_ui.rs::theme_dropdown_persists_*`.
//! Live fan-out: `tests/theme_live_switch.rs`.

use super::SettingsSection;
use crate::settings::store::SettingsStore;

pub struct ThemeSection;

/// Options surfaced by the (T13-mounted) Theme dropdown. Kept here
/// (rather than buried in the render closure) so T12 can import the
/// same list for the live-switch fan-out test.
pub const THEME_IDS: &[&str] = &["dark", "light", "high-contrast"];

impl ThemeSection {
    /// Persist a new theme id via the `SettingsStore` KV facade.
    ///
    /// Store-only; does NOT call `Theme::switch`. Tests use this to
    /// exercise the persistence round-trip without a `gpui::App`.
    /// Production callers should prefer [`theme_change_handler`].
    pub fn on_theme_change(store: &SettingsStore, new_id: &str) -> anyhow::Result<()> {
        store.set("theme.id", new_id)?;
        Ok(())
    }
}

/// Production handler for theme changes: persists the new id via
/// [`ThemeSection::on_theme_change`] AND calls [`crate::theme::Theme::switch`]
/// so every view subscribed via `cx.observe_global::<Theme>` re-renders.
///
/// Kept separate from [`ThemeSection::on_theme_change`] because the
/// `&mut gpui::App` arg can't be threaded through the SettingsStore-only
/// unit tests; live fan-out is exercised by `tests/theme_live_switch.rs`.
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
