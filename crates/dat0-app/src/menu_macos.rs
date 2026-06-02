//! macOS native menu bar (P1.T14).
//!
//! Builds the structure-only menu tree wired to GPUI actions. Items are
//! stubs at this stage — handlers are registered in later tasks (T15
//! command palette, T16 settings, T17 dialogs/about, T18+ file ops).
//!
//! API verified against `docs/internal/gpui-api-notes.md` §0.3:
//! - `Menu { name: SharedString, items: Vec<MenuItem> }`
//! - `MenuItem::action(name: impl Into<SharedString>, action: impl Action)`
//! - `MenuItem::separator()`
//! - `actions!(namespace, [Name1, Name2, ...])` derives
//!   `Clone + PartialEq + Default + Debug + gpui::Action` unit structs.
//!
//! On non-macOS targets the module compiles to a no-op `build_menus`
//! returning an empty `Vec<gpui::Menu>`, plus the same `menu_i18n_keys()`
//! list so the i18n invariant test runs cross-platform.

#[cfg(target_os = "macos")]
pub fn build_menus(_cx: &mut gpui::App) -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};
    vec![
        Menu {
            name: dat0_i18n::t("menu.file").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.file.new_window"), NewWindow),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.file.open_file"), OpenFile),
                MenuItem::action(dat0_i18n::t("menu.file.open_workspace"), OpenWorkspace),
                MenuItem::action(dat0_i18n::t("menu.file.open_package"), OpenPackage),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.file.export"), Export),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.file.close"), CloseWindow),
                MenuItem::action(dat0_i18n::t("menu.file.quit"), Quit),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.edit").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.edit.undo"), Undo),
                MenuItem::action(dat0_i18n::t("menu.edit.redo"), Redo),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.edit.cut"), Cut),
                MenuItem::action(dat0_i18n::t("menu.edit.copy"), Copy),
                MenuItem::action(dat0_i18n::t("menu.edit.paste"), Paste),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.view").into(),
            items: vec![
                MenuItem::action(
                    dat0_i18n::t("menu.view.command_palette"),
                    OpenCommandPalette,
                ),
                MenuItem::action(dat0_i18n::t("menu.view.settings"), OpenSettings),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.window").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.window.minimize"), Minimize),
                MenuItem::action(dat0_i18n::t("menu.window.zoom"), Zoom),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.help").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.help.about"), ShowAbout),
                MenuItem::action(dat0_i18n::t("menu.help.docs"), OpenDocs),
                MenuItem::action(dat0_i18n::t("menu.help.discord"), OpenDiscord),
            ],
        },
    ]
}

/// Non-macOS no-op. Linux / Windows native menu bars are out of scope for
/// P1; the public surface is preserved so callers can compile without
/// `#[cfg(target_os = "macos")]` guards at every call site.
#[cfg(not(target_os = "macos"))]
pub fn build_menus(_cx: &mut gpui::App) -> Vec<gpui::Menu> {
    vec![]
}

gpui::actions!(
    dat0_menu,
    [
        NewWindow,
        OpenFile,
        OpenWorkspace,
        OpenPackage,
        Export,
        CloseWindow,
        Quit,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        OpenCommandPalette,
        OpenSettings,
        Minimize,
        Zoom,
        ShowAbout,
        OpenDocs,
        OpenDiscord,
    ]
);

/// Top-level menu i18n keys, used by `tests/menu.rs` to assert every key
/// resolves to an actual translation (not the key itself).
pub fn menu_i18n_keys() -> &'static [&'static str] {
    &[
        "menu.file",
        "menu.edit",
        "menu.view",
        "menu.window",
        "menu.help",
    ]
}
