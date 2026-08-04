//! B8: the SQL console's `Panel` impls — the bottom dock.
//!
//! Unlike every sibling in this module there is **no wrapper entity**. B5-B7
//! wrap because the grid, inspector, charts, catalog, connections and AI
//! bodies are render fns ON the shell; the console is already a standalone
//! entity owning its own state, so a wrapper would add a hop and force
//! `focus_handle` to walk shell → console → active tab. `Panel` is a foreign
//! trait but `SqlConsole` is a local type, so these impls are legal here and
//! stay beside their siblings.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _,
    SharedString, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::view::sql_console::SqlConsole;

impl SqlConsole {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B8 onward** — upstream's `Panel`
    /// docs say a panel name must not change once defined.
    pub const PANEL_NAME: &str = "SqlConsolePanel";
}

impl EventEmitter<PanelEvent> for SqlConsole {}

impl Focusable for SqlConsole {
    /// The console's dedicated ROOT handle — **not** the active tab's editor,
    /// and not any other live stop inside the console.
    ///
    /// ⚠⚠ This is load-bearing and the first attempt got it wrong.
    /// `TabPanel::render` does `.track_focus(&self.focus_handle(cx))` on its
    /// container, and `TabPanel::focus_handle` delegates to the ACTIVE PANEL's
    /// (`tab_panel.rs:1167-1173`). So whatever this returns is *also* tracked
    /// on the TabPanel container — an ANCESTOR of this console, sitting
    /// OUTSIDE the `.tab_group()` the container opens.
    ///
    /// Returning the editor's handle therefore registered one `FocusId` twice
    /// in a single frame, at two different tab-stop paths, and every one of the
    /// console's ~18 focus stops dropped out of the Tab ring: a 200-press walk
    /// never reached a single named console stop. Enter/Space activation and
    /// the focus ring would have gone with them.
    ///
    /// B6/B7's panels never hit this because they hand back the SHELL's handle,
    /// which lives outside the panel subtree entirely.
    ///
    /// It is still tracked by a real element (the console's root, in `render`),
    /// so focusing it is not swallowed the way an untracked handle would be
    /// (B5).
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.root_focus.clone()
    }
}

impl Panel for SqlConsole {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// PLAIN text, deliberately WITHOUT an `a11y_label` — same reasoning as
    /// [`CatalogPanel::title`](super::catalog_panel::CatalogPanel). A dynamic
    /// title carrying the active tab's name was rejected at brainstorm: tab
    /// titles are user-editable, and a user naming a tab to match another node
    /// makes `A11ySnapshot::query_by_role` PANIC on a duplicate match
    /// (`tests/support/mod.rs:139`), taking whole suites down rather than
    /// failing one assertion.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(SharedString::from(dat0_i18n::t("sql.console.title")))
    }

    /// v1 dock scope is resize + collapse only.
    ///
    /// As `CatalogPanel` records, this does NOT remove the ⋯ button —
    /// `tab_panel.rs:483` renders it unconditionally — it only makes that
    /// menu's "Zoom In" row disabled.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    // `closable` is deliberately NOT overridden. `TabPanel::closable`
    // (`tab_panel.rs:100-113`) short-circuits on `!self.draggable(cx)`, and
    // `draggable = !is_locked(cx) && !is_last_panel(cx)`; dat0 calls
    // `dock.set_locked(true, ..)` at DockArea construction, so the lock has
    // been suppressing the close button for all five B6/B7 panels already.
    // That dependency is load-bearing and appears nowhere else in dat0 — the
    // pin in `tests/dock_chrome_spike.rs` holds it.

    // `visible` is deliberately NOT overridden either: the dock's own
    // open/closed state IS the console's visibility (design §4). Returning
    // `false` would blank the title bar's contents while `Dock::render` still
    // reserves its height, which is strictly worse than showing the bar.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key — a rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        assert_eq!(SqlConsole::PANEL_NAME, "SqlConsolePanel");
    }
}
