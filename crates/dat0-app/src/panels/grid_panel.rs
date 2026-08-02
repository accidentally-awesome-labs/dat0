//! B5: the DockArea center panel — a thin wrapper over the shell's grid body.
//!
//! The panel owns NO grid state. [`crate::window::WorkspaceShell`] keeps
//! `data_source`, `table_state`, `selection`, `recents_active`, the hero focus
//! handles, the root focus handle and the arrow-key handler; this panel's
//! `render` delegates straight back into `WorkspaceShell::render_grid_body`.
//!
//! Hero and focus-handle migration into the panel is deliberately deferred to
//! B7 (master plan §6, the declared focus-migration slice). Doing it here would
//! leave two suspects behind any red keyboard-nav result in the one slice whose
//! entire premise is that nothing changed.
//!
//! ## Why the shell update in `render` is safe
//!
//! `GridPanel::render` calls `shell.update(..)` while the shell's own render is
//! what put this panel on screen. That is NOT the re-entrancy B4 hit (a registry
//! closure dispatched from inside a `Context<WorkspaceShell>` update, which
//! needed `App::defer`): gpui has released the parent's lease by the time a
//! child view's element is laid out. Measured by the B5 T0 gate — see the design
//! doc §9.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, WeakEntity, Window,
    div,
};
use gpui_component::dock::{Panel, PanelEvent};

use crate::window::WorkspaceShell;

pub struct GridPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl GridPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B5 onward** — upstream's `Panel`
    /// docs say a panel name must not change once defined.
    pub const PANEL_NAME: &str = "GridPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for GridPanel {}

impl Focusable for GridPanel {
    /// Returns the SHELL's root focus handle, not one of our own.
    ///
    /// That handle is the grid's tab stop and the host of the arrow-key
    /// handler, so a `window.focus(panel)` from dock code lands on the real
    /// grid. A private handle would be tracked by no element, and focusing it
    /// would silently swallow focus rather than move it.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for GridPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }
}

impl Render for GridPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder, which
            // hands out a shell-less panel (see [`crate::panels::register_panels`]).
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_grid_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel name is B9's serialization key: `DockArea::load` resolves it
    /// through the global `PanelRegistry`, and upstream's trait docs say it
    /// must never change once defined. This is a rename ratchet — the string is
    /// load-bearing for a slice that has not been written yet.
    #[test]
    fn panel_name_is_frozen() {
        let panel = GridPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "GridPanel");
        assert_eq!(GridPanel::PANEL_NAME, "GridPanel");
    }
}
