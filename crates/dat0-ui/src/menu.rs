//! The native menu bar.
//!
//! Every item that means "do a thing dat0 knows about" is created with the
//! **stable `ActionRegistry` id as its `muda` id**, so a menu click is
//! `registry.dispatch(event.id(), &events)` and nothing else. There is no
//! second table mapping menu items to behaviour, which is what let the GPUI
//! build ship dead menu items (PRs #59/#60) — an item whose action existed
//! nowhere still rendered, still highlighted, and did nothing.
//! `tests/menu_reachability.rs` asserts every id resolves.
//!
//! Text editing (undo/redo/cut/copy/paste/select-all) uses
//! [`PredefinedMenuItem`]: those reach the focused webview input natively, so
//! dat0 neither implements them nor routes them. That deletes the
//! `gpui_component::input::{Cut, Copy, Paste}` dependency the GPUI menu needed
//! and, with it, the comment explaining why those three could not be
//! `os_action`s.
//!
//! `dioxus-desktop`'s `default_menu_bar()` is not used: it has no App submenu
//! (so no Preferences and no About) and its Window menu is ordered for
//! Windows. It is a starting point for a demo, not a shipped menu bar.

use dioxus::desktop::muda::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

use dat0_core::actions::builtin::ids;
use dat0_core::keymap::chord_for;
use dat0_core::recents::RecentEntry;

/// Ids for menu items that are window management or external links rather than
/// registry actions. Prefixed so they can never collide with an action id.
pub mod menu_ids {
    pub const ABOUT: &str = "menu.about";
    pub const CHECK_UPDATES: &str = "menu.check_updates";
    pub const DOCS: &str = "menu.docs";
    pub const DISCORD: &str = "menu.discord";
    pub const OPEN_PACKAGE: &str = "menu.open_package";
    pub const EXPORT_PACKAGE: &str = "menu.export_package";
    pub const UNPACK_PACKAGE: &str = "menu.unpack_package";
    pub const REPLAY_PACKAGE: &str = "menu.replay_package";
    pub const TOGGLE_SIDEBAR: &str = "menu.toggle_sidebar";
    pub const TOGGLE_INSPECTOR: &str = "menu.toggle_inspector";
    /// `recents.open.0` … `recents.open.9`.
    pub const RECENT_PREFIX: &str = "recents.open.";
}

/// File → Open Recent is capped at ten.
///
/// The store holds 25; a submenu of 25 paths is a wall, and the rest stay
/// reachable through the sidebar's PACKAGES section and the palette.
const OPEN_RECENT_CAP: usize = 10;

/// Translate a keymap chord (`"cmd-shift-p"`) into a `muda` accelerator string
/// (`"CmdOrCtrl+Shift+P"`).
///
/// The keymap stays the single source of truth for chords: the palette's hint
/// and the menu's accelerator are the same row, so they cannot disagree.
fn accelerator(action_id: &str) -> Option<String> {
    let chord = chord_for(action_id)?;
    let mut parts: Vec<String> = Vec::new();
    for p in chord.split('-') {
        parts.push(match p {
            "cmd" | "super" | "win" => "CmdOrCtrl".to_string(),
            "ctrl" => "Control".to_string(),
            "alt" | "option" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "enter" => "Enter".to_string(),
            "escape" => "Escape".to_string(),
            "tab" => "Tab".to_string(),
            other if other.len() == 1 => other.to_ascii_uppercase(),
            other => {
                let mut c = other.chars();
                match c.next() {
                    Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                    None => return None,
                }
            }
        });
    }
    Some(parts.join("+"))
}

/// An item whose id is an action id and whose accelerator comes from the keymap.
fn item(action_id: &'static str, label_key: &str) -> MenuItem {
    let accel = accelerator(action_id).and_then(|a| a.parse().ok());
    MenuItem::with_id(action_id, dat0_i18n::t(label_key), true, accel)
}

/// An item with no chord (window management, external links).
fn plain(id: &'static str, label_key: &str) -> MenuItem {
    MenuItem::with_id(id, dat0_i18n::t(label_key), true, None)
}

/// The recent-workspace items, newest first.
///
/// `Package` recents are excluded: File → Open Recent is about workspaces, and
/// packages have their own sidebar section.
fn open_recent_items() -> Vec<MenuItem> {
    let Some(store) = dat0_core::globals::recents() else {
        return Vec::new();
    };
    let Ok(guard) = store.lock() else {
        return Vec::new();
    };
    guard
        .list()
        .iter()
        .filter_map(|e| match e {
            RecentEntry::Workspace { path } => Some(path.clone()),
            RecentEntry::Package { .. } => None,
        })
        .take(OPEN_RECENT_CAP)
        .enumerate()
        .map(|(i, path)| {
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            // Ten fixed ids, matching the fan-out the GPUI menu used, so the
            // shell resolves an index rather than a path it would have to
            // re-derive.
            MenuItem::with_id(format!("{}{i}", menu_ids::RECENT_PREFIX), label, true, None)
        })
        .collect()
}

/// Build the menu bar.
pub fn build() -> Menu {
    let menu = Menu::new();

    // ── App ──────────────────────────────────────────────────────────────────
    // `default_menu_bar()` omits this entirely, which is why it is not used.
    let app = Submenu::new("dat0", true);
    let _ = app.append_items(&[
        &PredefinedMenuItem::about(
            Some(&dat0_i18n::t("menu.help.about")),
            Some(AboutMetadata {
                name: Some("dat0".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                ..Default::default()
            }),
        ),
        &PredefinedMenuItem::separator(),
        &item(ids::SETTINGS_OPEN, "menu.view.settings"),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);

    // ── File ─────────────────────────────────────────────────────────────────
    let file = Submenu::new(dat0_i18n::t("menu.file"), true);
    let _ = file.append_items(&[
        &item(ids::WINDOW_NEW, "menu.file.new_window"),
        &PredefinedMenuItem::separator(),
        &item(ids::FILE_OPEN, "menu.file.open_file"),
        &item(ids::WORKSPACE_OPEN, "menu.file.open_workspace"),
        &plain(menu_ids::OPEN_PACKAGE, "menu.file.open_package"),
    ]);
    let recents = open_recent_items();
    if !recents.is_empty() {
        let sub = Submenu::new(dat0_i18n::t("menu.file.open_recent"), true);
        for r in &recents {
            let _ = sub.append(r);
        }
        let _ = file.append(&sub);
    }
    let _ = file.append_items(&[
        &PredefinedMenuItem::separator(),
        &item(ids::WORKSPACE_SAVE, "menu.file.save_workspace"),
        &PredefinedMenuItem::separator(),
        &plain(menu_ids::EXPORT_PACKAGE, "menu.file.export_package"),
        &plain(menu_ids::UNPACK_PACKAGE, "menu.file.unpack_package"),
        &plain(menu_ids::REPLAY_PACKAGE, "menu.file.replay_package"),
        &PredefinedMenuItem::separator(),
        &item(ids::VIEW_EXPORT, "menu.file.export"),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::close_window(Some(&dat0_i18n::t("menu.file.close"))),
    ]);

    // ── Edit ─────────────────────────────────────────────────────────────────
    // Undo/redo are dat0's (they walk the transform stack, not a text buffer);
    // the clipboard six are the platform's and reach the focused input natively.
    let edit = Submenu::new(dat0_i18n::t("menu.edit"), true);
    let _ = edit.append_items(&[
        &item(ids::VIEW_UNDO, "menu.edit.undo"),
        &item(ids::VIEW_REDO, "menu.edit.redo"),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(Some(&dat0_i18n::t("menu.edit.cut"))),
        &PredefinedMenuItem::copy(Some(&dat0_i18n::t("menu.edit.copy"))),
        &PredefinedMenuItem::paste(Some(&dat0_i18n::t("menu.edit.paste"))),
        &PredefinedMenuItem::select_all(Some(&dat0_i18n::t("menu.edit.select_all"))),
    ]);

    // ── View ─────────────────────────────────────────────────────────────────
    let view = Submenu::new(dat0_i18n::t("menu.view"), true);
    let _ = view.append_items(&[
        &plain(menu_ids::TOGGLE_SIDEBAR, "catalog.toggle"),
        &plain(menu_ids::TOGGLE_INSPECTOR, "inspector.toggle"),
        &item(ids::CHART_VISUALIZE, "chart.visualize"),
        &PredefinedMenuItem::separator(),
        &item(ids::CONSOLE_TOGGLE, "sql.console_toggle"),
        &item(ids::SQL_RUN, "sql.run"),
        &item(ids::SQL_CANCEL, "sql.cancel"),
        &PredefinedMenuItem::separator(),
        &item(ids::AI_PANEL_OPEN, "menu.ai_panel"),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::fullscreen(None),
    ]);

    // ── Window ───────────────────────────────────────────────────────────────
    let window = Submenu::new(dat0_i18n::t("menu.window"), true);
    let _ = window.append_items(&[
        &PredefinedMenuItem::minimize(Some(&dat0_i18n::t("menu.window.minimize"))),
        &PredefinedMenuItem::maximize(Some(&dat0_i18n::t("menu.window.zoom"))),
    ]);

    // ── Help ─────────────────────────────────────────────────────────────────
    let help = Submenu::new(dat0_i18n::t("menu.help"), true);
    let _ = help.append_items(&[
        &item(ids::ONBOARDING_TAKE_TOUR, "menu.help.take_tour"),
        &plain(menu_ids::CHECK_UPDATES, "menu.help.check_updates"),
        &PredefinedMenuItem::separator(),
        &plain(menu_ids::DOCS, "menu.help.docs"),
        &plain(menu_ids::DISCORD, "menu.help.discord"),
    ]);

    let _ = menu.append_items(&[&app, &file, &edit, &view, &window, &help]);

    // Keep the children alive for the life of the process.
    //
    // `dioxus-desktop` parks this `Menu` in the per-window `WebviewInstance`,
    // but macOS's `init_for_nsapp` installs it as the *process* menu bar. Close
    // that window and the last `Rc` drops, freeing the `MenuChild` ivars that
    // `NSApp.mainMenu` still points at - so the next click on any menu item
    // reads freed memory. It surfaces as a `ZeroWidth` panic inside muda's icon
    // encoder: the garbage reads back as an `Option<Icon>` of 0 x 0, which is
    // exactly what muda's only validation (`pixel_count == width * height`)
    // waves through, and what its PNG encoder then refuses. Reproduced with
    // File -> New Window, close it, click any menu item.
    //
    // `Menu` is `Rc`-backed and `Clone`, so one leaked clone per menu pins the
    // children no matter which window owns the menu bar. Costs a few words per
    // window; the alternative is a use-after-free on every menu click.
    std::mem::forget(menu.clone());

    menu
}

/// Every id this menu bar can emit that is **not** a `PredefinedMenuItem`.
///
/// Exposed so `tests/menu_reachability.rs` can assert each one either resolves
/// in the `ActionRegistry` or is a declared `menu_ids` constant — the check
/// that keeps a dead menu item from shipping.
pub fn emitted_ids() -> Vec<String> {
    let mut ids: Vec<String> = vec![
        ids::SETTINGS_OPEN,
        ids::WINDOW_NEW,
        ids::FILE_OPEN,
        ids::WORKSPACE_OPEN,
        ids::WORKSPACE_SAVE,
        ids::VIEW_EXPORT,
        ids::VIEW_UNDO,
        ids::VIEW_REDO,
        ids::CHART_VISUALIZE,
        ids::CONSOLE_TOGGLE,
        ids::SQL_RUN,
        ids::SQL_CANCEL,
        ids::AI_PANEL_OPEN,
        ids::ONBOARDING_TAKE_TOUR,
        menu_ids::OPEN_PACKAGE,
        menu_ids::EXPORT_PACKAGE,
        menu_ids::UNPACK_PACKAGE,
        menu_ids::REPLAY_PACKAGE,
        menu_ids::TOGGLE_SIDEBAR,
        menu_ids::TOGGLE_INSPECTOR,
        menu_ids::CHECK_UPDATES,
        menu_ids::DOCS,
        menu_ids::DISCORD,
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    ids.extend((0..OPEN_RECENT_CAP).map(|i| format!("{}{i}", menu_ids::RECENT_PREFIX)));
    ids
}

/// The `menu_ids` constants, for the reachability test's second arm.
pub fn local_ids() -> Vec<String> {
    let mut v: Vec<String> = [
        menu_ids::ABOUT,
        menu_ids::CHECK_UPDATES,
        menu_ids::DOCS,
        menu_ids::DISCORD,
        menu_ids::OPEN_PACKAGE,
        menu_ids::EXPORT_PACKAGE,
        menu_ids::UNPACK_PACKAGE,
        menu_ids::REPLAY_PACKAGE,
        menu_ids::TOGGLE_SIDEBAR,
        menu_ids::TOGGLE_INSPECTOR,
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    v.extend((0..OPEN_RECENT_CAP).map(|i| format!("{}{i}", menu_ids::RECENT_PREFIX)));
    v
}

/// Every i18n key the menu bar renders a label from.
///
/// Kept beside [`build`] rather than derived from it because a `muda::Menu`
/// cannot be constructed off the platform main thread, so a test cannot walk
/// the real bar. `tests/menu.rs` asserts each key resolves — `dat0_i18n::t`
/// echoes a missing key back verbatim, which would ship a menu item labelled
/// `menu.file.open_workspace`.
pub fn label_keys() -> Vec<&'static str> {
    vec![
        // App
        "menu.help.about",
        "menu.view.settings",
        // File
        "menu.file",
        "menu.file.new_window",
        "menu.file.open_file",
        "menu.file.open_workspace",
        "menu.file.open_package",
        "menu.file.open_recent",
        "menu.file.save_workspace",
        "menu.file.export_package",
        "menu.file.unpack_package",
        "menu.file.replay_package",
        "menu.file.export",
        "menu.file.close",
        // Edit
        "menu.edit",
        "menu.edit.undo",
        "menu.edit.redo",
        "menu.edit.cut",
        "menu.edit.copy",
        "menu.edit.paste",
        "menu.edit.select_all",
        // View
        "menu.view",
        "catalog.toggle",
        "inspector.toggle",
        "chart.visualize",
        "sql.console_toggle",
        "sql.run",
        "sql.cancel",
        "menu.ai_panel",
        // Window
        "menu.window",
        "menu.window.minimize",
        "menu.window.zoom",
        // Help
        "menu.help",
        "menu.help.take_tour",
        "menu.help.check_updates",
        "menu.help.docs",
        "menu.help.discord",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_becomes_a_muda_accelerator() {
        // `chord_for` is platform-dependent, so assert the shape rather than a
        // fixed string: what matters is that the modifier survives translation.
        let undo = accelerator(ids::VIEW_UNDO).expect("undo is bound");
        assert!(undo.ends_with("+Z"), "{undo}");
        assert!(
            undo.starts_with("CmdOrCtrl") || undo.starts_with("Control"),
            "{undo}"
        );
    }

    #[test]
    fn every_generated_accelerator_parses_as_one() {
        // A malformed accelerator is dropped by `parse().ok()`, which would
        // ship a menu item with no visible shortcut and no error.
        for id in emitted_ids() {
            if let Some(a) = accelerator(&id) {
                assert!(
                    a.parse::<dioxus::desktop::muda::accelerator::Accelerator>()
                        .is_ok(),
                    "{id} produced an unparseable accelerator {a:?}"
                );
            }
        }
    }

    #[test]
    fn an_unbound_action_has_no_accelerator() {
        assert_eq!(accelerator(ids::SQL_NEW_TAB), None);
        assert_eq!(accelerator("nope.not.an.action"), None);
    }

    #[test]
    fn local_ids_cannot_collide_with_action_ids() {
        // The `menu.` / `recents.open.` prefixes are what make that true; this
        // asserts it rather than trusting the convention.
        let reg = dat0_core::actions::test_registry();
        for id in local_ids() {
            assert!(
                !reg.contains(&id),
                "{id} is both a menu-local id and a registered action"
            );
        }
    }
}
