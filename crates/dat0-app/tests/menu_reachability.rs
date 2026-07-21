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
//! - **`VIEW_SCOPED`**: handled via `.on_action` on the `WorkspaceShell` root
//!   in `render`, enabled only while the shell has focus. Skipped here
//!   (headless availability is legitimately false without a focused window);
//!   behaviourally covered by the nav suites;
//! - **`KNOWN_DEAD`**: pre-existing debt — items that have shipped grayed-out
//!   since their menus were added (system/window ops that want `os_action`
//!   conversion or window plumbing). Asserted UNAVAILABLE both ways, so
//!   wiring one up forces removing it from the list (ratchet, same policy as
//!   the style-lint allowlist).
//!
//! On Linux `build_menus` returns an empty Vec (no native menu bar), so the
//! walk is vacuous there — the direct asserts on the hotfix trio keep the
//! Linux leg meaningful.

use gpui::TestAppContext;

/// Actions handled view-scoped on the `WorkspaceShell` root (window.rs
/// `render`): reach `self` / need `&mut Window`, enable only with shell
/// focus. Covered by `keyboard_nav` / `sql_console_nav` / dock-toggle tests.
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
];

/// Known-dead menu items (render permanently grayed on macOS) — pre-existing
/// debt inventoried 2026-07-21, deliberately NOT fixed by the hotfix because
/// each needs an `os_action` conversion or window-handle plumbing decision:
///
/// - `Cut`/`Copy`/`Paste`: should likely become `MenuItem::os_action` so the
///   OS routes them to the focused text input's responder chain.
/// - `Quit`/`CloseWindow`/`Minimize`/`Zoom`: window/system ops; users reach
///   them via Cmd-Q keybind-free platform paths and window chrome today.
/// - `OpenFile`: the hero/dock flows exist (`open_file_picker`) but the menu
///   action was never wired.
///
/// Fixing any of these MUST remove the entry here or the test fails — the
/// list can only shrink.
const KNOWN_DEAD: &[&str] = &[
    "dat0_menu::OpenFile",
    "dat0_menu::CloseWindow",
    "dat0_menu::Quit",
    "dat0_menu::Cut",
    "dat0_menu::Copy",
    "dat0_menu::Paste",
    "dat0_menu::Minimize",
    "dat0_menu::Zoom",
];

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

        // The hotfix trio, asserted directly so the Linux leg (empty menu
        // tree) still guards the regression.
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
        ] {
            assert!(
                available,
                "{name} lost its global handler — View/Help menu item goes dead again"
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
            let available = cx.is_action_available(action.as_ref());
            if KNOWN_DEAD.contains(&name) {
                assert!(
                    !available,
                    "{name} is listed KNOWN_DEAD but now has a global handler — \
                     remove it from KNOWN_DEAD (the list can only shrink)"
                );
            } else if VIEW_SCOPED.contains(&name) {
                // Legitimately unavailable headless; covered by nav suites.
            } else {
                assert!(
                    available,
                    "menu action {name} has NO registered handler — its menu item \
                     renders permanently grayed-out on macOS. Register it in \
                     window::register_menu_action_handlers (global) or add it to \
                     VIEW_SCOPED/KNOWN_DEAD with a reason."
                );
            }
        }
    });
}
