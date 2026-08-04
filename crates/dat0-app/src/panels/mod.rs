//! Dock panels — `gpui_component::dock::Panel` implementors.
//!
//! B5 introduces the first one ([`grid_panel::GridPanel`]); B6 added the
//! inspector and charts right docks; B7 added the catalog, connections and AI
//! left docks. B8 adds the SQL console. They live here rather than in
//! `src/view/` because a `Panel` is a different kind of thing from a free render
//! fn: it is an entity with a stable `panel_name` that `DockArea::load` resolves
//! through a global registry (B9).

pub mod ai_dock_panel;
pub mod catalog_panel;
pub mod charts_panel;
pub mod connections_panel;
pub mod grid_panel;
pub mod inspector_panel;
pub mod sql_console_panel;

use gpui::{App, AppContext as _};

/// Register every dat0 panel with gpui-component's global `PanelRegistry`.
///
/// Called from `run_app` AND from each test binary's `init_components`: a
/// registration performed only in production is silently absent under test
/// (the `register_modal_keys` lesson from B1/B2).
///
/// Nothing calls `DockArea::load` until B9, so the builder below is currently
/// unreachable. It hands back a shell-less panel rather than panicking — the
/// `WeakEntity::new_invalid()` upgrade fails and `GridPanel::render` paints an
/// empty div, which degrades gracefully instead of arming a landmine. B9
/// replaces it with a builder that resolves the live shell.
pub fn register_panels(cx: &mut App) {
    gpui_component::dock::register_panel(
        cx,
        grid_panel::GridPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| grid_panel::GridPanel::new(gpui::WeakEntity::new_invalid())))
        },
    );

    gpui_component::dock::register_panel(
        cx,
        inspector_panel::InspectorPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(
                cx.new(|_| inspector_panel::InspectorPanel::new(gpui::WeakEntity::new_invalid())),
            )
        },
    );

    gpui_component::dock::register_panel(
        cx,
        charts_panel::ChartsPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| charts_panel::ChartsPanel::new(gpui::WeakEntity::new_invalid())))
        },
    );

    // B7: the three left-dock panels.
    gpui_component::dock::register_panel(
        cx,
        catalog_panel::CatalogPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| catalog_panel::CatalogPanel::new(gpui::WeakEntity::new_invalid())))
        },
    );

    gpui_component::dock::register_panel(
        cx,
        connections_panel::ConnectionsPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(
                cx.new(|_| {
                    connections_panel::ConnectionsPanel::new(gpui::WeakEntity::new_invalid())
                }),
            )
        },
    );

    gpui_component::dock::register_panel(
        cx,
        ai_dock_panel::AiDockPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| ai_dock_panel::AiDockPanel::new(gpui::WeakEntity::new_invalid())))
        },
    );

    // B8: the bottom dock. Same degraded contract as the six above, but the
    // console is a real stateful entity rather than a shell delegate, so the
    // placeholder hands back a console over ZERO persisted tabs and its own
    // fresh autocomplete snapshot — `SqlConsole::new` falls back to a single
    // empty tab. B9 replaces all seven with builders that resolve the live
    // shell and the real session.
    gpui_component::dock::register_panel(
        cx,
        crate::view::sql_console::SqlConsole::PANEL_NAME,
        |_dock_area, _state, _info, window, cx| {
            Box::new(cx.new(|cx| {
                crate::view::sql_console::SqlConsole::new(
                    &[],
                    None,
                    crate::query::completion::new_shared_snapshot(),
                    window,
                    cx,
                )
            }))
        },
    );
}
