//! Theme section — choose the active theme; mention user-theme drop folder.
//!
//! P3b T11 closes D-001 by wiring the `SettingsStore` KV facade
//! (`theme.id`). The visible widget stays a placeholder until T13 mounts
//! the real `Root::new` window over `SettingsView` — at that point
//! [`on_theme_change`] becomes the `gpui_component::Select` widget's
//! `on_change` handler (see `docs/internal/gpui-component-api-notes.md`
//! §3.6 for the `Select` constructor that lands then).
//!
//! The load-bearing contract for D-001 is the SettingsStore round-trip
//! itself, exercised by `tests/settings_ui.rs::theme_dropdown_persists_*`.

use super::SettingsSection;
use crate::settings::store::SettingsStore;
use gpui::{IntoElement, ParentElement, div};

pub struct ThemeSection;

/// Options surfaced by the (T13-mounted) Theme dropdown. Kept here
/// (rather than buried in the render closure) so T12 can import the
/// same list for the live-switch fan-out test.
pub const THEME_IDS: &[&str] = &["dark", "light", "high-contrast"];

impl ThemeSection {
    /// Closure invoked when the user picks a new theme from the
    /// (T13-mounted) dropdown. Persists the new id via the
    /// `SettingsStore` KV facade and — at T12 — will also call
    /// `crate::theme::Theme::switch(cx, new_id)` so every subscriber
    /// of the `Theme` global re-renders without an app restart.
    ///
    /// The split keeps the test surface (this closure) and the GPUI
    /// global-mutation surface (T12) decoupled.
    pub fn on_theme_change(store: &SettingsStore, new_id: &str) -> anyhow::Result<()> {
        store.set("theme.id", new_id)?;
        // T12: also call Theme::switch(cx, new_id) to fan out to every
        // `cx.observe_global::<Theme>` subscriber (D-002 follow-up).
        // The settings persistence above is independent of live-switch
        // and is sufficient on its own to round-trip the new id on app
        // restart, which is what D-001 commits to.
        Ok(())
    }
}

impl SettingsSection for ThemeSection {
    fn name_key(&self) -> &'static str {
        "settings.theme"
    }

    fn id(&self) -> &'static str {
        "theme"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        // T13 swaps this placeholder for a `gpui_component::Select`
        // bound to `on_theme_change`. The SettingsStore plumbing is
        // already live + tested via `tests/settings_ui.rs`; the view
        // stub is the only piece pending mount of the real window
        // (see D-001 closure in `docs/deferrals.md` and §3.6 of the
        // gpui-component spike doc).
        div()
            .child(dat0_i18n::t("settings.theme.placeholder"))
            .into_any_element()
    }
}
