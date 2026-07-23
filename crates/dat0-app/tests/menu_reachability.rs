//! Menu-reachability inventory — regression gate for the 2026-07-21
//! dead-menu-item hotfix (UI-redesign master plan §4b).
//!
//! macOS grays out any `MenuItem::action` whose gpui action has no registered
//! handler; `App::is_action_available` is the exact oracle the platform menu
//! validation consults (active-window focus path + global listeners). That is
//! how View ▸ Settings… shipped permanently grayed: `OpenSettings` was
//! declared and attached to a menu item, but no `cx.on_action` handler
//! existed anywhere, and its only other wiring (the ActionRegistry
//! `settings.open` descriptor) is consumed solely by the stub command
//! palette.
//!
//! This test walks the REAL menu tree (`menu_macos::build_menus`) against the
//! REAL production handler set (`window::register_menu_action_handlers` — the
//! same fn `run_app` calls) and buckets every action found:
//!
//! - **global** (the default): must be `is_action_available` right after
//!   registration — a new menu item whose action has no handler fails here
//!   immediately;
//! - **`VIEW_SCOPED`**: focus-gated — handled on the `WorkspaceShell` root or
//!   inside gpui-component's `"Input"` key context, enabled only while the
//!   right element has focus. Skipped here (headless availability is
//!   legitimately false without a focused window); behaviourally covered by
//!   the nav suites.
//!
//! The original inventory also carried a `KNOWN_DEAD` bucket (11 items shipped
//! permanently grayed); it emptied on 2026-07-22 and was removed — every menu
//! action now must land in one of the two buckets above.
//!
//! On Linux `build_menus` returns an empty Vec (no native menu bar), so the
//! walk is vacuous there — the direct asserts on the hotfix trio keep the
//! Linux leg meaningful.

use gpui::TestAppContext;

/// Focus-gated actions — legitimately unavailable headless, enabled only
/// while the right element has focus:
///
/// - `dat0_menu::*` entries: handled view-scoped on the `WorkspaceShell` root
///   (window.rs `render`); reach `self` / need `&mut Window`. Covered by
///   `keyboard_nav` / `sql_console_nav` / dock-toggle tests.
/// - `input::*` entries: the Edit menu dispatches gpui-component's Input
///   actions (dead-item fix, 2026-07-22), handled inside the focused Input's
///   `"Input"` key context — items enable exactly while a text input has
///   focus.
const VIEW_SCOPED: &[&str] = &[
    "dat0_menu::SqlRun",
    "dat0_menu::SqlCancel",
    "dat0_menu::SqlConsoleToggle",
    "dat0_menu::SqlNewTab",
    "dat0_menu::SqlCloseTab",
    "dat0_menu::ConnectionsToggle",
    "dat0_menu::CatalogToggle",
    "dat0_menu::InspectorToggle",
    "dat0_menu::ChartVisualize",
    "dat0_menu::AiPanelToggle",
    "input::Cut",
    "input::Copy",
    "input::Paste",
];

// The 2026-07-21 inventory found 11 dead menu items (no handler anywhere →
// permanently grayed). PR #59 fixed OpenSettings/OpenDocs/OpenDiscord; the
// 2026-07-22 follow-up fixed the rest (Quit, CloseWindow, Minimize, Zoom
// globally; OpenFile view-scoped; Cut/Copy/Paste re-pointed at
// `gpui_component::input` actions). The dead-list is now EMPTY — every menu
// action must be global-available or listed in `VIEW_SCOPED` with a reason.

/// Recursively collect every `MenuItem::Action` in a menu tree.
fn collect_actions(items: &[gpui::MenuItem], out: &mut Vec<Box<dyn gpui::Action>>) {
    for item in items {
        match item {
            gpui::MenuItem::Action { action, .. } => out.push(action.boxed_clone()),
            gpui::MenuItem::Submenu(menu) => collect_actions(&menu.items, out),
            _ => {}
        }
    }
}

#[gpui::test]
fn every_menu_action_has_a_registered_handler(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // The exact production registration path (run_app calls this fn).
        dat0_app::window::register_menu_action_handlers(cx);

        // The previously-dead items now wired globally, asserted directly so
        // the Linux leg (empty menu tree) still guards the regressions.
        for (name, available) in [
            (
                "OpenSettings",
                cx.is_action_available(&dat0_app::menu_macos::OpenSettings),
            ),
            (
                "OpenDocs",
                cx.is_action_available(&dat0_app::menu_macos::OpenDocs),
            ),
            (
                "OpenDiscord",
                cx.is_action_available(&dat0_app::menu_macos::OpenDiscord),
            ),
            ("Quit", cx.is_action_available(&dat0_app::menu_macos::Quit)),
            (
                "CloseWindow",
                cx.is_action_available(&dat0_app::menu_macos::CloseWindow),
            ),
            (
                "Minimize",
                cx.is_action_available(&dat0_app::menu_macos::Minimize),
            ),
            ("Zoom", cx.is_action_available(&dat0_app::menu_macos::Zoom)),
            (
                "OpenFile",
                cx.is_action_available(&dat0_app::menu_macos::OpenFile),
            ),
        ] {
            assert!(
                available,
                "{name} lost its global handler — its menu item goes dead again"
            );
        }

        // Full inventory over the real menu tree (macOS; vacuous on Linux).
        let mut actions = Vec::new();
        for menu in dat0_app::menu_macos::build_menus(cx) {
            collect_actions(&menu.items, &mut actions);
        }
        // Guard against a vacuous walk where the inventory silently checks
        // nothing: the macOS menu bar always carries actions.
        #[cfg(target_os = "macos")]
        assert!(
            !actions.is_empty(),
            "build_menus returned no actions on macOS — inventory is vacuous"
        );
        for action in &actions {
            let name = action.name();
            if VIEW_SCOPED.contains(&name) {
                // Legitimately unavailable headless; focus-gated (see const).
                continue;
            }
            assert!(
                cx.is_action_available(action.as_ref()),
                "menu action {name} has NO registered handler — its menu item \
                 renders permanently grayed-out on macOS. Register it in \
                 window::register_menu_action_handlers (global) or add it to \
                 VIEW_SCOPED with a reason."
            );
        }
    });
}
