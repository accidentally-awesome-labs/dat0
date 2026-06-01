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

    Ok(())
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
        let change = workspace.update(app, |ws, _cx| {
            let change = ws.view_model.as_mut().and_then(|vm| vm.undo());
            // Refresh the ColumnView off the new active stack (P4c T5). A
            // display-only undo (e.g. undoing a Rename/Reorder, T6+) never
            // round-trips through `apply_view_change`, so this is the only hook
            // that keeps the header labels/order + addressing fresh for those.
            // For a real data-view undo it is harmless (the source columns are
            // unchanged) and `apply_view_change` refreshes again on rebind.
            ws.refresh_column_view();
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
        let change = workspace.update(app, |ws, _cx| {
            let change = ws.view_model.as_mut().and_then(|vm| vm.redo());
            // Refresh the ColumnView off the new active stack (P4c T5);
            // symmetric to `dispatch_undo` — see the rationale there.
            ws.refresh_column_view();
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
