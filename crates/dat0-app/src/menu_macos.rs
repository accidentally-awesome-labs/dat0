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
//! - `MenuItem::submenu(Menu { name, items })`
//! - `actions!(namespace, [Name1, Name2, ...])` derives
//!   `Clone + PartialEq + Default + Debug + gpui::Action` unit structs.
//!
//! On non-macOS targets the module compiles to a no-op `build_menus`
//! returning an empty `Vec<gpui::Menu>`, plus the same `menu_i18n_keys()`
//! list so the i18n invariant test runs cross-platform.

/// Maximum number of recent workspace entries shown in File → Open Recent.
///
/// Design note: the recents store holds up to 25 entries; we cap the menu at
/// 10 to keep the submenu compact and because the fan-out approach requires a
/// fixed set of action types (OpenRecent0..OpenRecent9).  If the user has more
/// than 10 workspace recents, the oldest ones are silently omitted from the
/// menu (they remain in the full recents store / drawer).
///
/// Gated to macOS: its only user, `open_recent_items`, is `#[cfg(macos)]`, so on
/// other platforms an ungated const is dead code (CI's linux clippy job catches
/// what a darwin-local `cargo clippy` cannot).
#[cfg(target_os = "macos")]
const OPEN_RECENT_MENU_CAP: usize = 10;

/// Build the Open Recent submenu items from the current recents store.
///
/// Returns a `Vec<MenuItem>` containing one action per recent workspace (up to
/// [`OPEN_RECENT_MENU_CAP`]).  Returns an empty `Vec` when there are no recent
/// workspaces (the caller omits the submenu entirely in that case).
#[cfg(target_os = "macos")]
fn open_recent_items() -> Vec<gpui::MenuItem> {
    use gpui::MenuItem;

    let Some(recents_arc) = crate::window_registry::recents() else {
        return vec![];
    };
    let Ok(guard) = recents_arc.lock() else {
        return vec![];
    };

    let workspace_entries: Vec<std::path::PathBuf> = guard
        .list()
        .iter()
        .filter_map(|e| {
            if let crate::recents::RecentEntry::Workspace { path } = e {
                Some(path.clone())
            } else {
                None
            }
        })
        .take(OPEN_RECENT_MENU_CAP)
        .collect();

    workspace_entries
        .into_iter()
        .enumerate()
        .map(|(idx, path)| {
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let action: Box<dyn gpui::Action> = match idx {
                0 => Box::new(OpenRecent0),
                1 => Box::new(OpenRecent1),
                2 => Box::new(OpenRecent2),
                3 => Box::new(OpenRecent3),
                4 => Box::new(OpenRecent4),
                5 => Box::new(OpenRecent5),
                6 => Box::new(OpenRecent6),
                7 => Box::new(OpenRecent7),
                8 => Box::new(OpenRecent8),
                9 => Box::new(OpenRecent9),
                _ => unreachable!("capped at OPEN_RECENT_MENU_CAP=10"),
            };
            MenuItem::Action {
                name: label.into(),
                action,
                os_action: None,
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub fn build_menus(_cx: &mut gpui::App) -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};

    // Build the File → Open Recent submenu. If there are no recent workspaces
    // we omit the submenu entirely (GPUI doesn't support disabled items).
    let recent_items = open_recent_items();
    let open_recent_entry = if recent_items.is_empty() {
        None
    } else {
        Some(MenuItem::submenu(Menu {
            name: dat0_i18n::t("menu.file.open_recent").into(),
            items: recent_items,
        }))
    };

    let mut file_items = vec![
        MenuItem::action(dat0_i18n::t("menu.file.new_window"), NewWindow),
        MenuItem::separator(),
        MenuItem::action(dat0_i18n::t("menu.file.open_file"), OpenFile),
        MenuItem::action(dat0_i18n::t("menu.file.open_workspace"), OpenWorkspace),
        MenuItem::action(dat0_i18n::t("menu.file.open_package"), OpenPackage),
    ];
    if let Some(recent) = open_recent_entry {
        file_items.push(recent);
    }
    file_items.extend([
        MenuItem::separator(),
        MenuItem::action(dat0_i18n::t("menu.file.save_workspace"), SaveWorkspace),
        MenuItem::separator(),
        // P8 T9: .dat0 package operations.
        MenuItem::action(dat0_i18n::t("menu.file.export_package"), ExportPackage),
        MenuItem::action(dat0_i18n::t("menu.file.unpack_package"), UnpackPackage),
        MenuItem::action(dat0_i18n::t("menu.file.replay_package"), ReplayPackage),
        MenuItem::separator(),
        MenuItem::action(dat0_i18n::t("menu.file.export"), Export),
        MenuItem::separator(),
        MenuItem::action(dat0_i18n::t("menu.file.close"), CloseWindow),
        MenuItem::action(dat0_i18n::t("menu.file.quit"), Quit),
    ]);

    vec![
        Menu {
            name: dat0_i18n::t("menu.file").into(),
            items: file_items,
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
                MenuItem::separator(),
                // P5a T11: SQL Console toggle + active-statement run / cancel.
                MenuItem::action(dat0_i18n::t("sql.console_toggle"), SqlConsoleToggle),
                MenuItem::action(dat0_i18n::t("sql.run"), SqlRun),
                MenuItem::action(dat0_i18n::t("sql.cancel"), SqlCancel),
                // P5c T11: Connections panel toggle.
                MenuItem::action(dat0_i18n::t("connections.toggle"), ConnectionsToggle),
                // P6a T7: Catalog left-dock toggle.
                MenuItem::action(dat0_i18n::t("catalog.toggle"), CatalogToggle),
                // P6a T9: Inspector right-dock toggle.
                MenuItem::action(dat0_i18n::t("inspector.toggle"), InspectorToggle),
                // P9a T7: Charts right-dock toggle (Visualize).
                MenuItem::action(dat0_i18n::t("chart.visualize"), ChartVisualize),
                // P9c-1 T9: AI panel left-dock toggle.
                MenuItem::action(dat0_i18n::t("menu.ai_panel"), AiPanelToggle),
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
                // P10a-2 T6: manual update check.
                MenuItem::action(dat0_i18n::t("menu.help.check_updates"), CheckForUpdates),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.help.docs"), OpenDocs),
                MenuItem::action(dat0_i18n::t("menu.help.discord"), OpenDiscord),
                // P10c T8: Report a Bug.
                MenuItem::action(dat0_i18n::t("menu.help.report_bug"), ReportBug),
                // P11a T7: Take a Tour — re-opens the onboarding carousel.
                MenuItem::action(dat0_i18n::t("menu.help.take_tour"), TakeTour),
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
        // P8 T9: .dat0 package actions (Export / Unpack / Replay; Open is above).
        ExportPackage,
        UnpackPackage,
        ReplayPackage,
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
        // P10a-2 T6: "Check for Updates" menu action.
        CheckForUpdates,
        OpenDocs,
        OpenDiscord,
        // P10c T8: Report a Bug — opens the crash/bug-report dialog.
        ReportBug,
        // P11a T7: Take a Tour — re-opens the onboarding carousel.
        TakeTour,
        // P11a T9: Open demo.dat0 — unpack bundled demo package as editable workspace.
        OpenDemoWorkspace,
        // P5a T11: SQL Console entry points (toggle / run / cancel / tab lifecycle).
        SqlConsoleToggle,
        SqlRun,
        SqlCancel,
        SqlNewTab,
        SqlCloseTab,
        // P5c T11: Connections panel toggle.
        ConnectionsToggle,
        // P6a T7: Catalog left-dock toggle.
        CatalogToggle,
        // P6a T9: Inspector right-dock toggle.
        InspectorToggle,
        // P9a T7: Charts right-dock toggle (Visualize).
        ChartVisualize,
        // P9c-1 T9: AI panel left-dock toggle.
        AiPanelToggle,
        // P7a T7: workspace save flow.
        SaveWorkspace,
        // P7a T10: File → Open Recent fan-out (10-slot cap; see OPEN_RECENT_MENU_CAP).
        // Each action maps to an index into the filtered workspace-recents list.
        // Entries beyond index 9 are silently omitted from the menu (still in the
        // recents store / drawer).  Handlers are registered in window.rs run_app.
        OpenRecent0,
        OpenRecent1,
        OpenRecent2,
        OpenRecent3,
        OpenRecent4,
        OpenRecent5,
        OpenRecent6,
        OpenRecent7,
        OpenRecent8,
        OpenRecent9,
    ]
);

/// Top-level menu i18n keys, used by `tests/menu.rs` to assert every key
/// resolves to an actual translation (not the key itself).
pub fn menu_i18n_keys() -> &'static [&'static str] {
    &[
        "menu.file",
        "menu.file.save_workspace",
        "menu.file.open_recent",
        // P8 T9: .dat0 package menu items.
        "menu.file.open_package",
        "menu.file.export_package",
        "menu.file.unpack_package",
        "menu.file.replay_package",
        "menu.edit",
        "menu.view",
        "menu.window",
        "menu.help",
    ]
}

/// Re-apply the menu bar so File → Open Recent reflects the latest entries.
///
/// Reads the recents singleton via `build_menus` and posts `cx.set_menus` onto
/// the GPUI main thread via [`crate::window_registry::dispatcher`].  No-op if
/// the dispatcher isn't installed yet (e.g., very early during startup before
/// `Application::run` fires).
///
/// Called by the workspace save/open flows after pushing to the recents store
/// so the menu immediately shows the new entry.
pub fn rebuild_menus_with_recents() {
    let Some(dispatcher) = crate::window_registry::dispatcher() else {
        return;
    };
    // `build_menus` reads the recents singleton synchronously inside the closure
    // so it runs on the main thread with the freshest store state.
    let _ = dispatcher.dispatch(|cx: &mut gpui::App| {
        #[cfg(target_os = "macos")]
        {
            let menus = build_menus(cx);
            cx.set_menus(menus);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = cx;
        }
    });
}
