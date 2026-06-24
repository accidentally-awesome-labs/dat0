//! Settings → Memory Budget section (P10b). Real InputState control lands in T7.
use super::SettingsSection;
use gpui::{IntoElement, ParentElement, div};

pub struct MemoryBudgetSection;

impl SettingsSection for MemoryBudgetSection {
    fn name_key(&self) -> &'static str {
        "settings.memory_budget"
    }

    fn id(&self) -> &'static str {
        "memory_budget"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div()
            .child(dat0_i18n::t("settings.memory_budget.placeholder"))
            .into_any_element()
    }
}
