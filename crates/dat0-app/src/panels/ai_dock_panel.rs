//! B7: the left dock's AI panel — a thin wrapper over the shell's AI body,
//! following B5's [`GridPanel`](super::grid_panel::GridPanel) template.
//!
//! The panel owns NO AI state. [`crate::window::WorkspaceShell`] keeps the
//! `AiPanel` draft, the eight `ai-*` focus handles (minted from its `hero_focus`
//! map) and every handler; this panel's `render` delegates straight back into
//! [`WorkspaceShell::render_ai_body`].
//!
//! Named `AiDockPanel` rather than `AiPanel` because
//! [`crate::ai::panel::AiPanel`] already exists and is the DRAFT STATE this
//! renders — two different things that would otherwise share a name.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::window::WorkspaceShell;

pub struct AiDockPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl AiDockPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B7 onward.**
    pub const PANEL_NAME: &str = "AiDockPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for AiDockPanel {}

impl Focusable for AiDockPanel {
    /// The SHELL's root handle — see [`super::catalog_panel::CatalogPanel`].
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for AiDockPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// Carries an `a11y_label` for the same reason as the Connections panel:
    /// nothing else names this surface. `ai.title` is "AI Providers", which is
    /// what the View menu calls it.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = dat0_i18n::t("ai.title");
        div()
            .a11y_label(AccessRole::Label, title.clone())
            .child(SharedString::from(title))
    }

    /// v1 dock scope is resize + collapse only.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// The shell's bool is the single source of truth (design §5).
    fn visible(&self, cx: &App) -> bool {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).ai_visible())
            .unwrap_or(false)
    }
}

impl Render for AiDockPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder.
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_ai_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key — a rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        let panel = AiDockPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "AiDockPanel");
        assert_eq!(AiDockPanel::PANEL_NAME, "AiDockPanel");
    }

    #[test]
    fn shell_less_panel_has_no_shell_to_read() {
        let panel = AiDockPanel::new(gpui::WeakEntity::new_invalid());
        assert!(panel.shell.upgrade().is_none());
    }
}
