//! B7: the left dock's Catalog panel — a thin wrapper over the shell's catalog
//! body, following B5's [`GridPanel`](super::grid_panel::GridPanel) template.
//!
//! The panel owns NO catalog state. [`crate::window::WorkspaceShell`] keeps
//! `catalog_tree`, `catalog_collapsed`, `catalog_active` and `catalog_nav_key`;
//! this panel's `render` delegates straight back into
//! [`WorkspaceShell::render_catalog_body`].
//!
//! The master plan's B7 row proposed moving that state in here. It was written
//! before B5 established this template, and the move would touch session
//! persistence, `catalog_nav_key` and three a11y shims for no user-visible gain
//! — in the same slice that migrates nine focus handles. See design §3.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::window::WorkspaceShell;

pub struct CatalogPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl CatalogPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B7 onward** — upstream's `Panel` docs
    /// say a panel name must not change once defined.
    pub const PANEL_NAME: &str = "CatalogPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for CatalogPanel {}

impl Focusable for CatalogPanel {
    /// Returns the SHELL's root focus handle, not one of our own — a private
    /// handle is tracked by no element, so focusing it silently swallows focus
    /// rather than moving it (B5).
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for CatalogPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// PLAIN text, deliberately WITHOUT an `a11y_label` — and this is the one
    /// place the three left panels differ.
    ///
    /// `catalog/panel.rs` already names its root
    /// `.a11y("catalog-tree", Button, t("catalog.title"))`. A second node
    /// carrying that same name would make `A11ySnapshot::query_by_role` panic on
    /// a duplicate match (`tests/support/mod.rs:139`), taking whole suites down
    /// rather than failing one assertion. The word is still rendered, so the
    /// title bar looks identical; it simply is not a second capture node.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(SharedString::from(dat0_i18n::t("catalog.title")))
    }

    /// v1 dock scope is resize + collapse only.
    ///
    /// Note this does NOT remove the ⋯ button — `tab_panel.rs:483` renders it
    /// unconditionally — it only makes that menu's "Zoom In" row disabled.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// The shell's bool is the single source of truth (design §5); the dock
    /// derives from it, never the other way round.
    ///
    /// A dead weak handle means this panel came from the B9 placeholder builder
    /// in [`super::register_panels`] — stay hidden rather than render a
    /// catalog-shaped hole.
    fn visible(&self, cx: &App) -> bool {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).catalog_visible())
            .unwrap_or(false)
    }
}

impl Render for CatalogPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder.
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_catalog_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key — a rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        let panel = CatalogPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "CatalogPanel");
        assert_eq!(CatalogPanel::PANEL_NAME, "CatalogPanel");
    }

    /// A shell-less panel must degrade, not panic — the B9 placeholder builder
    /// hands one out. `visible()` needs an `App`, so assert on the branch it
    /// keys off instead.
    #[test]
    fn shell_less_panel_has_no_shell_to_read() {
        let panel = CatalogPanel::new(gpui::WeakEntity::new_invalid());
        assert!(panel.shell.upgrade().is_none());
    }
}
