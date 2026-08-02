//! B6: the right dock's Inspector panel — a thin wrapper over the shell's
//! inspector body, following B5's [`GridPanel`](super::grid_panel::GridPanel)
//! template exactly.
//!
//! The panel owns NO inspector state. [`crate::window::WorkspaceShell`] keeps
//! `inspector`, the projection context and the visibility bool; this panel's
//! `render` delegates straight back into
//! [`WorkspaceShell::render_inspector_body`].
//!
//! ## Why `title()` carries the a11y label
//!
//! `inspector::panel::render_inspector` used to draw its own "Inspector" title
//! row. A `TabPanel` paints a 30px title bar above the body, so keeping both
//! would show the word twice. The row moved here rather than being deleted, so
//! the accessible name is relocated instead of lost — which also keeps the
//! total capture-node count neutral, and a net-zero count is a much sharper
//! signal than one that moved for two reasons at once.
//!
//! It also lands on the safer side of the T0 risk: only `active-panel` →
//! `tab-content` is wrapped in `.cached(..)` (`tab_panel.rs:851-861`), and the
//! title bar sits outside it.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::window::WorkspaceShell;

pub struct InspectorPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl InspectorPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B6 onward** — upstream's `Panel`
    /// docs say a panel name must not change once defined.
    pub const PANEL_NAME: &str = "InspectorPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for InspectorPanel {}

impl Focusable for InspectorPanel {
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

impl Panel for InspectorPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// Called every frame by the title bar, so this stays a static i18n lookup
    /// plus one pushed label — never a `format!` and never a shell read.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = dat0_i18n::t("inspector.title");
        div()
            .a11y_label(AccessRole::Label, title.clone())
            .child(SharedString::from(title))
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
    /// in [`super::register_panels`] — stay hidden rather than render an
    /// inspector-shaped hole.
    fn visible(&self, cx: &App) -> bool {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).inspector_visible())
            .unwrap_or(false)
    }
}

impl Render for InspectorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder.
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_inspector_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key: `DockArea::load` resolves it through the global
    /// `PanelRegistry`, and upstream's trait docs say it must never change once
    /// defined. This is a rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        let panel = InspectorPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "InspectorPanel");
        assert_eq!(InspectorPanel::PANEL_NAME, "InspectorPanel");
    }

    /// A shell-less panel must degrade, not panic — the B9 placeholder builder
    /// hands one out. `visible()` needs an `App`, so assert on the branch it
    /// keys off instead.
    #[test]
    fn shell_less_panel_has_no_shell_to_read() {
        let panel = InspectorPanel::new(gpui::WeakEntity::new_invalid());
        assert!(panel.shell.upgrade().is_none());
    }
}
