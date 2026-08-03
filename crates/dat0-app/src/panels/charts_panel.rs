//! B6: the right dock's Charts panel — a thin wrapper over the shell's chart
//! body, following B5's [`GridPanel`](super::grid_panel::GridPanel) template.
//!
//! The panel owns NO chart state. [`crate::window::WorkspaceShell`] keeps
//! `chart_panel`, `chart_image`, the visibility bool and every listener; this
//! panel's `render` delegates straight back into
//! [`WorkspaceShell::render_charts_body`].
//!
//! ## Why the export buttons live here and not in the body
//!
//! [`Panel::toolbar_buttons`] renders into the 30px title bar. Upstream stamps
//! `.xsmall().ghost().tab_stop(false)` on every button it returns
//! (`tab_panel.rs:454`) — title-bar controls are mouse-only by construction.
//! That is exactly why B6 T2 registered `chart.export.png` /
//! `chart.export.svg` in the command palette FIRST: those descriptors, not
//! these buttons, are chart export's keyboard path.
//!
//! The chart-type cycle, the per-axis cycles and Save deliberately stay in the
//! body. Save carries a real `disabled` state whose affordance reads correctly
//! at body size, and the axis buttons carry long interpolated labels
//! (`"X: order_date"`) that a 30px `text_ellipsis` bar would truncate to noise.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::button::Button;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::window::WorkspaceShell;

pub struct ChartsPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl ChartsPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B6 onward.**
    pub const PANEL_NAME: &str = "ChartsPanel";
    /// Kept byte-identical to the ids these buttons carried in the body
    /// toolbar, so the move is invisible to anything keyed on them.
    pub const EXPORT_PNG_ID: &str = "chart-export-png";
    pub const EXPORT_SVG_ID: &str = "chart-export-svg";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for ChartsPanel {}

impl Focusable for ChartsPanel {
    /// The SHELL's root handle — see [`super::inspector_panel::InspectorPanel`].
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for ChartsPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// Static i18n lookup — `title()` runs every frame.
    ///
    /// Plural "Charts", matching the View menu, rather than the existing
    /// `chart.panel.title` ("Chart") which is already interpolated into the
    /// body's chart-type button and would read oddly as a panel name.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = dat0_i18n::t("charts.title");
        div()
            .a11y_label(AccessRole::Label, title.clone())
            .child(SharedString::from(title))
    }

    /// v1 dock scope is resize + collapse only. Does NOT remove the ⋯ button
    /// (`tab_panel.rs:483` renders it unconditionally) — only disables its
    /// "Zoom In" row.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// The shell's bool is the single source of truth (design §5).
    fn visible(&self, cx: &App) -> bool {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).chart_visible())
            .unwrap_or(false)
    }

    /// Short uppercase labels rather than icons: no export-shaped icon is
    /// bundled (86 upstream icons; the nearest are `external-link` and
    /// `arrow-down`), and a bare glyph loses its meaning first at high
    /// contrast — the theme this redesign is least forgiving about.
    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![
            self.export_button(Self::EXPORT_PNG_ID, "chart.export.png", true),
            self.export_button(Self::EXPORT_SVG_ID, "chart.export.svg", false),
        ])
    }
}

impl ChartsPanel {
    /// One export button. Clicking with no rendered data is a silent no-op —
    /// `export_chart` guards on `chart_panel.data`, which is the behaviour the
    /// body toolbar had too.
    fn export_button(&self, id: &'static str, label_key: &str, png: bool) -> Button {
        let shell = self.shell.clone();
        let label = dat0_i18n::t(label_key);
        Button::new(id)
            .label(label.clone())
            // A bare gpui-component `Button` contributes NOTHING to the capture
            // tree — only an explicit `.a11y`/`.a11y_label` pushes a node. These
            // two are the entire mouse affordance for export after B6, so they
            // get a real accessible name rather than being invisible to both the
            // oracle and any future screen reader. `Button` implements
            // `InteractiveElement`, so this chains directly (A5).
            .a11y(id, AccessRole::Button, label)
            .on_click(move |_ev, _window, app| {
                if let Some(ws) = shell.upgrade() {
                    ws.update(app, |ws, cx| ws.export_chart(png, cx));
                }
            })
    }
}

impl Render for ChartsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder.
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_charts_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key — rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        let panel = ChartsPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "ChartsPanel");
        assert_eq!(ChartsPanel::PANEL_NAME, "ChartsPanel");
    }

    /// After B6 the title bar's two buttons are the only mouse affordance for
    /// export, so their ids are load-bearing for `tests/right_dock.rs`.
    #[test]
    fn export_button_ids_are_stable() {
        assert_eq!(ChartsPanel::EXPORT_PNG_ID, "chart-export-png");
        assert_eq!(ChartsPanel::EXPORT_SVG_ID, "chart-export-svg");
    }

    /// The labels are what a 30px title bar has to fit. Guard the shortness
    /// rather than the exact string, which translations will change.
    #[test]
    fn export_labels_are_short_enough_for_a_title_bar() {
        for key in ["chart.export.png", "chart.export.svg"] {
            let label = dat0_i18n::t(key);
            assert!(
                !label.is_empty() && label.chars().count() <= 6,
                "{key} = {label:?} is too long for a 30px title bar shared with \
                 the panel name and the ⋯ menu"
            );
        }
    }
}
