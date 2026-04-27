//! Theme section — choose the active theme; mention user-theme drop folder.
//!
//! P1 scaffolding only: renders a placeholder string. Real theme picker
//! (list of bundled + user themes from
//! `~/Library/Application Support/dat0/themes/`) lands in a later
//! milestone alongside the gpui-component theme integration.

use super::SettingsSection;
use gpui::{IntoElement, ParentElement, div};

pub struct ThemeSection;

impl SettingsSection for ThemeSection {
    fn name_key(&self) -> &'static str {
        "settings.theme"
    }

    fn id(&self) -> &'static str {
        "theme"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div()
            .child(dat0_i18n::t("settings.theme.placeholder"))
            .into_any_element()
    }
}
