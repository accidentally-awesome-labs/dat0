//! `view.undo` + `view.redo` action descriptors.
//!
//! T13: dispatch closures are now wired to the real focused-tab `ViewModel`
//! lookup + engine round-trip via `spawn_view_change`. The pattern:
//!
//! 1. Retrieve the focused `WorkspaceShell` via `focused_workspace_weak()`,
//!    upgrade the `AnyWeakEntity`, and downcast to `Entity<WorkspaceShell>`.
//! 2. Call `vm.undo()` / `vm.redo()` to get `Option<ViewChange>`.
//! 3. If `Some(change)` → `spawn_view_change(engine, base_table, change, rebind_closure)`.
//!
//! Keybinds are registered in [`crate::window::spawn_window`] (same
//! pattern as `OpenCommandPalette` / Cmd-Shift-P from P3b T6):
//! - macOS:  Cmd-Z / Cmd-Shift-Z
//! - Linux:  Ctrl-Z / Ctrl-Shift-Z

use std::sync::Arc;

use super::builtin::ids;
use super::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry, RegisterError};

/// Register `view.undo` and `view.redo` onto `reg`.
///
/// Called from [`super::builtin::register_all`] at app boot. Returns
/// [`RegisterError::DuplicateId`] if either id is already present — this
/// is a programmer error, not a runtime condition, so boot panics on it.
pub fn register(reg: &ActionRegistry) -> Result<(), RegisterError> {
    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_UNDO),
        title: "Undo".into(),
        group: ActionGroup::Edit,
        keybinding: None, // keybind wired in window.rs (bind_keys + on_action)
        dispatch: Arc::new(|app| {
            dispatch_undo(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_REDO),
        title: "Redo".into(),
        group: ActionGroup::Edit,
        keybinding: None, // keybind wired in window.rs (bind_keys + on_action)
        dispatch: Arc::new(|app| {
            dispatch_redo(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_EXPORT),
        title: "Export…".into(),
        group: ActionGroup::File,
        keybinding: None, // keybind wired in window.rs (bind_keys + on_action)
        dispatch: Arc::new(|app| {
            dispatch_export(app);
        }),
    })?;

    // P5a T11: SQL Console descriptors. The KEYBIND + MENU paths are handled
    // VIEW-scoped on the WorkspaceShell root (`window.rs::render`), because the
    // console toggle/new-tab handlers need a `&mut Window` that the registry
    // `Fn(&mut App)` dispatch path can't supply. These descriptors exist so the
    // P5b command palette can SURFACE the actions; their dispatch bodies cover
    // the App-reachable subset (run / cancel) and leave the Window-needing ones
    // (toggle / tab lifecycle) as breadcrumbs that the palette overlay (P5b)
    // re-routes through the focused window once the WindowRegistry hop lands.
    reg.register(ActionDescriptor {
        id: ActionId::from(ids::CONSOLE_TOGGLE),
        title: dat0_i18n::t("sql.console_toggle"),
        group: ActionGroup::Navigation,
        keybinding: None, // keybind: Cmd+Shift+C (view-scoped, window.rs)
        dispatch: Arc::new(|_app| {
            // Needs `&mut Window` (lazily builds + shows the console). Reached via
            // the Cmd+Shift+C keybind / View-menu item (view-scoped handler).
            tracing::debug!(
                "action: console.toggle dispatched via registry — handled view-scoped (needs Window); no-op from App path"
            );
        }),
    })?;

    // P9a T7: Charts → Visualize. Like CONSOLE_TOGGLE, the toggle needs the
    // focused workspace's `cx` (it spawns a describe_table + plot-query round
    // trip), but unlike the console it works from `cx` alone — so the App-path
    // dispatch CAN do the work via `focused_workspace` + `toggle_chart_panel`.
    reg.register(ActionDescriptor {
        id: ActionId::from(ids::CHART_VISUALIZE),
        title: dat0_i18n::t("chart.visualize"),
        group: ActionGroup::Navigation,
        keybinding: None, // also reachable view-scoped via the Charts menu item
        dispatch: Arc::new(|app| {
            dispatch_visualize(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::SQL_RUN),
        title: dat0_i18n::t("sql.run"),
        group: ActionGroup::Edit,
        keybinding: None, // keybind: Cmd+Enter (view-scoped, window.rs)
        dispatch: Arc::new(|app| {
            dispatch_sql_run(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::SQL_CANCEL),
        title: dat0_i18n::t("sql.cancel"),
        group: ActionGroup::Edit,
        keybinding: None, // keybind: Cmd+. (view-scoped, window.rs)
        dispatch: Arc::new(|app| {
            dispatch_sql_cancel(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::SQL_NEW_TAB),
        title: dat0_i18n::t("sql.new_tab"),
        group: ActionGroup::Edit,
        keybinding: None, // menu + console "+" button (view-scoped, window.rs)
        dispatch: Arc::new(|_app| {
            tracing::debug!(
                "action: sql.new_tab dispatched via registry — handled view-scoped (needs Window); no-op from App path"
            );
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::SQL_CLOSE_TAB),
        title: dat0_i18n::t("sql.close_tab"),
        group: ActionGroup::Edit,
        keybinding: None, // menu + console "✕" button (view-scoped, window.rs)
        dispatch: Arc::new(|app| {
            dispatch_sql_close_tab(app);
        }),
    })?;

    Ok(())
}

/// Dispatch body for `sql.run` (P5a T11). Runs the focused workspace's active
/// statement into the main grid. Cursor-only resolution (no selection
/// override — T0 proved there is no public selection getter at this
/// gpui-component rev). No-op when no console is mounted.
fn dispatch_sql_run(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("sql.run: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| {
        if let Some(console) = ws.sql_console.clone() {
            ws.spawn_sql_run(console, crate::query::ResultTarget::MainGrid, cx);
        }
    });
}

/// Dispatch body for `sql.cancel` (P5a T11). Interrupts the focused
/// workspace's in-flight run; safe when there is none.
fn dispatch_sql_cancel(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("sql.cancel: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.cancel_sql_run(cx));
}

/// Dispatch body for `sql.close_tab` (P5a T11). Closes the active tab of the
/// focused workspace's console (a no-op on the last tab). No-op when no
/// console is mounted.
fn dispatch_sql_close_tab(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("sql.close_tab: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| {
        if let Some(console) = ws.sql_console.clone() {
            let active = console.read(cx).active;
            console.update(cx, |c, cx| c.close_tab(active, cx));
        }
    });
}

/// Dispatch body for `chart.visualize` (P9a T7). Toggles the focused
/// workspace's right-dock chart panel; on open it binds to the active grid's
/// base table and kicks off the first plot query. No-op when no workspace is
/// focused (or no file is registered — `toggle_chart_panel` guards on that).
pub fn dispatch_visualize(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("chart.visualize: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.toggle_chart_panel(cx));
}

/// Retrieve the focused `WorkspaceShell` entity, if available.
///
/// Uses the type-erased `AnyWeakEntity` stored in `FOCUSED_WORKSPACE`
/// (window_registry.rs) to avoid a circular import. Returns `None` if no
/// workspace has been registered or if the workspace entity has been dropped.
fn focused_workspace(app: &mut gpui::App) -> Option<gpui::Entity<crate::window::WorkspaceShell>> {
    let weak = crate::window_registry::focused_workspace_weak()?;
    let any_entity = weak.upgrade()?;
    let entity = any_entity
        .downcast::<crate::window::WorkspaceShell>()
        .ok()?;
    // Validate the entity is still alive by attempting a read.
    let _ = entity.read(app);
    Some(entity)
}

/// Dispatch body for `view.undo` (T13).
///
/// 1. Resolve the focused workspace via `focused_workspace_weak()`.
/// 2. Call `vm.undo()` on the workspace's ViewModel.
/// 3. If a `ViewChange` is produced, spawn the engine round-trip via
///    `spawn_view_change`; the `on_rebind` closure calls
///    `WorkspaceShell::apply_view_change` on the main thread.
fn dispatch_undo(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.undo: no focused workspace");
        return;
    };

    let (engine, base_table, change, ws_weak) = {
        let ws = workspace.read(app);
        let Some(vm) = ws.view_model.as_ref() else {
            tracing::debug!("view.undo: no ViewModel (no file registered yet)");
            return;
        };
        if !vm.can_undo() {
            tracing::debug!("view.undo: nothing to undo");
            return;
        }
        // We need &mut to call undo(); extract values first, then mutate.
        let base_table = vm.base_table().to_string();
        let engine = ws.engine();
        let ws_weak = workspace.downgrade();
        // Release the immutable borrow of `ws` before the `update` call below
        // requires mutable access to the same entity.
        let _ = ws;
        let change = workspace.update(app, |ws, cx| {
            let change = ws.view_model.as_mut().and_then(|vm| vm.undo());
            // Refresh the ColumnView off the new active stack (P4c T5). A
            // display-only undo (undoing a Rename/Reorder/DeleteColumn) never
            // round-trips through `apply_view_change`, so this is the only hook
            // that keeps the grid header AND the projection-aware Inspector fresh.
            //
            // The Inspector is re-PROJECTED here (it reads the live `column_view`
            // on render), but never re-PROFILED — projection ops are no-ops in
            // `compile_view_sql`, so the SUMMARIZE source is unchanged (guarded by
            // `view::consumer_tests::display_only_undo_keeps_inspector_profile_source_stable`).
            // For a real data-view undo the notify is harmless (apply_view_change
            // repaints + re-profiles again on rebind).
            ws.refresh_column_view();
            cx.notify();
            change
        });
        (engine, base_table, change, ws_weak)
    };

    let Some(change) = change else {
        return;
    };

    crate::view::spawn_view_change(
        engine,
        base_table,
        change,
        Arc::new(move |new_ds, app_cx| {
            if let Some(handle) = ws_weak.upgrade() {
                handle.update(app_cx, |ws, cx| ws.apply_view_change(new_ds, cx));
            }
        }),
    );
}

/// Dispatch body for `view.export` (P4c T11).
///
/// Resolves the focused workspace and asks it to mount the File → Export…
/// dialog. The dialog is a no-op (graceful) when no `ViewModel` is mounted
/// (`WorkspaceShell::open_export_dialog` guards on that), so Export… off an
/// empty workspace presents nothing rather than a dialog that can't build a
/// SELECT.
fn dispatch_export(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.export: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.open_export_dialog(cx));
}

/// Dispatch body for `view.redo` (T13). Symmetric to `dispatch_undo`.
fn dispatch_redo(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.redo: no focused workspace");
        return;
    };

    let (engine, base_table, change, ws_weak) = {
        let ws = workspace.read(app);
        let Some(vm) = ws.view_model.as_ref() else {
            tracing::debug!("view.redo: no ViewModel (no file registered yet)");
            return;
        };
        if !vm.can_redo() {
            tracing::debug!("view.redo: nothing to redo");
            return;
        }
        let base_table = vm.base_table().to_string();
        let engine = ws.engine();
        let ws_weak = workspace.downgrade();
        // Release the immutable borrow of `ws` before the `update` call below
        // requires mutable access to the same entity.
        let _ = ws;
        let change = workspace.update(app, |ws, cx| {
            let change = ws.view_model.as_mut().and_then(|vm| vm.redo());
            // Symmetric to dispatch_undo — re-project the Inspector + refresh the
            // grid header on a display-only redo (re-projected, not re-profiled).
            ws.refresh_column_view();
            cx.notify();
            change
        });
        (engine, base_table, change, ws_weak)
    };

    let Some(change) = change else {
        return;
    };

    crate::view::spawn_view_change(
        engine,
        base_table,
        change,
        Arc::new(move |new_ds, app_cx| {
            if let Some(handle) = ws_weak.upgrade() {
                handle.update(app_cx, |ws, cx| ws.apply_view_change(new_ds, cx));
            }
        }),
    );
}
