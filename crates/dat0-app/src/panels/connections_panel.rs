//! B7: the left dock's Connections panel — a thin wrapper over the shell's
//! connections body, following B5's [`GridPanel`](super::grid_panel::GridPanel)
//! template.
//!
//! The panel owns NO connection state. [`crate::window::WorkspaceShell`] keeps
//! the `ConnectionManager` and every handler; this panel's `render` delegates
//! straight back into [`WorkspaceShell::render_connections_body`].

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::window::WorkspaceShell;

pub struct ConnectionsPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl ConnectionsPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B7 onward.**
    pub const PANEL_NAME: &str = "ConnectionsPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for ConnectionsPanel {}

impl Focusable for ConnectionsPanel {
    /// The SHELL's root handle — see [`super::catalog_panel::CatalogPanel`].
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for ConnectionsPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// Carries an `a11y_label`, unlike the Catalog's title: nothing else in this
    /// panel names it, so without this the panel is anonymous to a screen reader.
    /// There is no duplicate to collide with here — the body's own title row is
    /// removed at T4, and it was a bare child that contributed no node anyway.
    ///
    /// Static i18n lookup, because `title()` runs every frame.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = dat0_i18n::t("connections.title");
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
            .map(|ws| ws.read(cx).connections_visible())
            .unwrap_or(false)
    }
}

impl Render for ConnectionsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder.
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_connections_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key — a rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        let panel = ConnectionsPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "ConnectionsPanel");
        assert_eq!(ConnectionsPanel::PANEL_NAME, "ConnectionsPanel");
    }

    #[test]
    fn shell_less_panel_has_no_shell_to_read() {
        let panel = ConnectionsPanel::new(gpui::WeakEntity::new_invalid());
        assert!(panel.shell.upgrade().is_none());
    }
}
