//! Settings → Telemetry section (P10b). Real controls land in T10.
use super::SettingsSection;
use crate::settings::store::SettingsStore;
use gpui::{IntoElement, ParentElement, div};

pub struct TelemetrySection;

/// Persist the `crash_submission_enabled` toggle via the atomic settings write
/// path. Store-only half (no GPUI) — unit-testable like
/// `set_treat_all_as_networked`.
pub fn set_crash_submission_enabled(store: &SettingsStore, value: bool) -> anyhow::Result<()> {
    let mut s = store.load_or_default()?;
    s.telemetry.crash_submission_enabled = value;
    store.save(&s)
}

impl SettingsSection for TelemetrySection {
    fn name_key(&self) -> &'static str {
        "settings.telemetry"
    }

    fn id(&self) -> &'static str {
        "telemetry"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div()
            .child(dat0_i18n::t("settings.telemetry.placeholder"))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_round_trips_through_store() {
        let store = SettingsStore::open_in_memory();
        // default is false
        assert!(
            !store
                .load_or_default()
                .unwrap()
                .telemetry
                .crash_submission_enabled
        );
        set_crash_submission_enabled(&store, true).unwrap();
        assert!(
            store
                .load_or_default()
                .unwrap()
                .telemetry
                .crash_submission_enabled
        );
        set_crash_submission_enabled(&store, false).unwrap();
        assert!(
            !store
                .load_or_default()
                .unwrap()
                .telemetry
                .crash_submission_enabled
        );
    }
}
