//! Settings → Telemetry section (P10b). Real controls land in T10.
use super::SettingsSection;
use gpui::{IntoElement, ParentElement, div};

pub struct TelemetrySection;

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
