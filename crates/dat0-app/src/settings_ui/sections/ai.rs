//! Settings → AI section (P10b). Real controls land in T9.
use super::SettingsSection;
use gpui::{IntoElement, ParentElement, div};

pub struct AiSection;

impl SettingsSection for AiSection {
    fn name_key(&self) -> &'static str {
        "settings.ai"
    }

    fn id(&self) -> &'static str {
        "ai"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div()
            .child(dat0_i18n::t("settings.ai.placeholder"))
            .into_any_element()
    }
}
