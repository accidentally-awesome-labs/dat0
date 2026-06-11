//! Settings panel section registry (P1.T16).
//!
//! Each section implements `SettingsSection` to declare its i18n key, a
//! stable string id used for sidebar selection, and a `render` method that
//! produces the right-pane content. The `all_sections()` registry is the
//! single source of truth consumed by `SettingsView` (in the parent
//! module) and by the tests in `tests/settings_ui.rs`.

pub mod profile;
pub mod theme;
pub mod workspace;

/// Trait implemented by every settings panel section.
///
/// Sections are registered via `all_sections()` below. The `name_key`
/// returns an i18n key resolved through `dat0_i18n::t` so the sidebar
/// label respects the active locale; the `id` is a stable English-only
/// string used as the selection key.
pub trait SettingsSection {
    /// i18n key for the section label (e.g. `"settings.profile"`).
    fn name_key(&self) -> &'static str;

    /// Stable identifier used for selection / addressing (e.g. `"profile"`).
    fn id(&self) -> &'static str;

    /// Render the section's content pane.
    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement;
}

/// Return every section the settings panel knows about, in display order.
pub fn all_sections() -> Vec<Box<dyn SettingsSection>> {
    vec![
        Box::new(profile::ProfileSection),
        Box::new(theme::ThemeSection),
    ]
}
