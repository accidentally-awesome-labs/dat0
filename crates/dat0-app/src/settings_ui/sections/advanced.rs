//! Settings → Advanced section (P10b). Real controls land in T10.
use super::SettingsSection;
use gpui::{IntoElement, ParentElement, div};

pub struct AdvancedSection;

impl SettingsSection for AdvancedSection {
    fn name_key(&self) -> &'static str {
        "settings.advanced"
    }

    fn id(&self) -> &'static str {
        "advanced"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div()
            .child(dat0_i18n::t("settings.advanced.placeholder"))
            .into_any_element()
    }
}
