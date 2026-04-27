//! Profile section — author identity used when creating `.dat0` packages.
//!
//! P1 scaffolding only: renders a placeholder string. The real form
//! (name + email inputs wired to `dat0-app::settings::store`) lands in a
//! later milestone.

use super::SettingsSection;
use gpui::{IntoElement, ParentElement, div};

pub struct ProfileSection;

impl SettingsSection for ProfileSection {
    fn name_key(&self) -> &'static str {
        "settings.profile"
    }

    fn id(&self) -> &'static str {
        "profile"
    }

    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div()
            .child(dat0_i18n::t("settings.profile.placeholder"))
            .into_any_element()
    }
}
