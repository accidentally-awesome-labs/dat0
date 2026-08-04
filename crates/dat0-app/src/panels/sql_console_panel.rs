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
    /// The ACTIVE TAB'S EDITOR handle, so a `window.focus(panel)` from dock
    /// code lands where the user types.
    ///
    /// Never a private handle: one minted here would be tracked by no element,
    /// and focusing it silently SWALLOWS focus rather than moving it (B5).
    /// Index-guarded because `active` is a plain `usize` — the console
    /// maintains at least one tab, but a panic in a `&App` accessor called
    /// every frame is not worth the assumption.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.tabs
            .get(self.active)
            .or_else(|| self.tabs.first())
            .map(|tab| tab.input.read(cx).focus_handle(cx))
            .unwrap_or_else(|| cx.focus_handle())
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
