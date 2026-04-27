//! Settings panel UI (P1.T16) — sidebar list of section labels on the
//! left, active section's content pane on the right.
//!
//! `SettingsView` is the GPUI `Render` entity. The plan (P1.T21 / boot
//! orchestration) wires it to the View → Settings… menu action; until
//! then this module is scaffolded but not yet opened from the running
//! app. The two integration tests in `tests/settings_ui.rs` exercise the
//! `sections` registry directly so the panel stays testable independent
//! of the GPUI window lifecycle.
//!
//! Layout follows the lower-level Zed `settings_ui` pattern documented in
//! `docs/internal/gpui-api-notes.md` §0.5 (Reference B): outer row split
//! into a fixed-width sidebar (`w_64`) and a flex-1 content pane. The
//! gpui-component `setting` module (Reference A) is the higher-level
//! alternative we may migrate to in a later milestone.

pub mod sections;

use gpui::{IntoElement, Render, div, prelude::*};

/// Settings panel view. Holds the currently selected section id; the
/// section list itself is sourced from `sections::all_sections()` on
/// every render so adding a new section requires only registering it
/// there.
pub struct SettingsView {
    selected_section: String,
}

impl SettingsView {
    /// Construct a new settings view with `profile` selected by default.
    pub fn new() -> Self {
        Self {
            selected_section: "profile".into(),
        }
    }
}

impl Default for SettingsView {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for SettingsView {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        // `Context<Self>` derefs to `App`, so we can pass `cx` straight to
        // section renderers that expect `&mut gpui::App`.
        let app: &mut gpui::App = cx;
        let sections = sections::all_sections();
        let active_index = sections
            .iter()
            .position(|s| s.id() == self.selected_section);

        let sidebar = div().w_64().flex().flex_col().children(
            sections
                .iter()
                .map(|s| div().child(dat0_i18n::t(s.name_key()))),
        );

        let content = div().flex_1().when_some(active_index, |d, idx| {
            d.child(sections[idx].render(window, app))
        });

        div().flex().flex_row().child(sidebar).child(content)
    }
}
