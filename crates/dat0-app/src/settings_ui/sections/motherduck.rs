//! Settings → MotherDuck section (P10b). Real controls land in T8.
use super::SettingsSection;
use gpui::{IntoElement, ParentElement, div};

pub struct MotherDuckSection;

impl SettingsSection for MotherDuckSection {
    fn name_key(&self) -> &'static str {
        "settings.motherduck"
    }

    fn id(&self) -> &'static str {
        "motherduck"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div()
            .child(dat0_i18n::t("settings.motherduck.placeholder"))
            .into_any_element()
    }
}
