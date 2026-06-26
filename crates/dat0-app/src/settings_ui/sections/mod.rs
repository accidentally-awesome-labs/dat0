//! Settings panel section registry.
//!
//! Each section implements `SettingsSection` to declare its i18n key and a
//! stable string id used for sidebar selection. The `all_sections()` registry
//! is the single source of truth consumed by `SettingsPanel` (panel.rs) for
//! sidebar rendering, and by the tests in `tests/settings_ui.rs`.

pub mod advanced;
pub mod ai;
pub mod memory_budget;
pub mod motherduck;
pub mod profile;
pub mod telemetry;
pub mod theme;
pub mod updates;
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
}

/// Return every section the settings panel knows about, in display order.
/// Consumed by `SettingsPanel` (panel.rs) for sidebar rendering and by the
/// `tests/settings_ui.rs` integration tests.
pub fn all_sections() -> Vec<Box<dyn SettingsSection>> {
    vec![
        Box::new(profile::ProfileSection),
        Box::new(theme::ThemeSection),
        Box::new(memory_budget::MemoryBudgetSection),
        Box::new(motherduck::MotherDuckSection),
        Box::new(ai::AiSection),
        Box::new(telemetry::TelemetrySection),
        Box::new(workspace::WorkspaceSection),
        Box::new(updates::UpdatesSection),
        Box::new(advanced::AdvancedSection),
    ]
}
