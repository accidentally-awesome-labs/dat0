//! Edit / clipboard / bulk-operation action descriptors (T9).
//!
//! Registers the following stable action ids (all in the `Edit` group):
//!
//! | id                | handler                                  |
//! |-------------------|------------------------------------------|
//! | `view.copy`       | `WorkspaceShell::copy_selection`         |
//! | `view.cut`        | `WorkspaceShell::cut_selection`          |
//! | `view.paste`      | `WorkspaceShell::paste_clipboard`        |
//! | `view.fill_down`  | `WorkspaceShell::fill_down`              |
//! | `view.set_null`   | `WorkspaceShell::set_null_selection`     |
//! | `view.set_value`  | `WorkspaceShell::set_value_selection`    |
//! | `view.delete_rows`| `WorkspaceShell::delete_selected_rows`   |
//!
//! Dispatch pattern mirrors `view_actions.rs` (T13): resolve the focused
//! `WorkspaceShell` via [`crate::window_registry::focused_workspace_weak`],
//! downcast the `AnyWeakEntity`, upgrade it, then `update` the shell with
//! the GPUI `App` context so the handler runs on the main thread.
//!
//! Keybindings (Cmd+C / Cmd+X / Cmd+V / Ctrl+D / Delete) are deferred to T11.
//! The context-menu wiring is in [`crate::grid::context_menu`] (T9 Step 4).

use std::sync::Arc;

use super::builtin::ids;
use super::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry, RegisterError};

/// Register all edit / clipboard / bulk actions onto `reg`.
///
/// Called from [`super::builtin::register_all`] at app boot. Returns
/// [`RegisterError::DuplicateId`] if any id is already present — this is a
/// programmer error, not a runtime condition, so boot panics on it.
pub fn register(reg: &ActionRegistry) -> Result<(), RegisterError> {
    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_COPY),
        title: "Copy".into(),
        group: ActionGroup::Edit,
        keybinding: None, // T11 wires Cmd+C / Ctrl+C
        dispatch: Arc::new(|app| {
            dispatch_copy(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_CUT),
        title: "Cut".into(),
        group: ActionGroup::Edit,
        keybinding: None, // T11 wires Cmd+X / Ctrl+X
        dispatch: Arc::new(|app| {
            dispatch_cut(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_PASTE),
        title: "Paste".into(),
        group: ActionGroup::Edit,
        keybinding: None, // T11 wires Cmd+V / Ctrl+V
        dispatch: Arc::new(|app| {
            dispatch_paste(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_FILL_DOWN),
        title: "Fill Down".into(),
        group: ActionGroup::Edit,
        keybinding: None, // T11 wires Ctrl+D
        dispatch: Arc::new(|app| {
            dispatch_fill_down(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_SET_NULL),
        title: "Set NULL".into(),
        group: ActionGroup::Edit,
        keybinding: None, // T11 wires Delete key
        dispatch: Arc::new(|app| {
            dispatch_set_null(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_SET_VALUE),
        title: "Set Value…".into(),
        group: ActionGroup::Edit,
        keybinding: None, // no standard keybind; invoked from context menu
        dispatch: Arc::new(|app| {
            // T9: no-arg dispatch cannot carry a `Scalar` value. The context
            // menu bypasses the registry for this action and calls
            // `WorkspaceShell::set_value_selection` directly via a closure.
            // This descriptor serves command-palette discoverability only;
            // a future task (T14/polish) can open a value-input dialog here.
            tracing::debug!(
                "view.set_value dispatched via registry \
                 (no value arg — context menu uses direct closure)"
            );
            let _ = app;
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::VIEW_DELETE_ROWS),
        title: "Delete Row(s)".into(),
        group: ActionGroup::Edit,
        keybinding: None, // T11 wires Delete / Backspace in row-selection mode
        dispatch: Arc::new(|app| {
            dispatch_delete_rows(app);
        }),
    })?;

    Ok(())
}

/// Retrieve the focused `WorkspaceShell` entity, if available.
///
/// Mirrors the identical helper in `view_actions.rs`: uses the type-erased
/// `AnyWeakEntity` stored in `FOCUSED_WORKSPACE` (window_registry.rs) to avoid
/// a circular import. Returns `None` if no workspace has been registered or the
/// workspace entity has been dropped.
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

fn dispatch_copy(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.copy: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.copy_selection(cx));
}

fn dispatch_cut(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.cut: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.cut_selection(cx));
}

fn dispatch_paste(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.paste: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.paste_clipboard(cx));
}

fn dispatch_fill_down(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.fill_down: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.fill_down(cx));
}

fn dispatch_set_null(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.set_null: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.set_null_selection(cx));
}

fn dispatch_delete_rows(app: &mut gpui::App) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!("view.delete_rows: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.delete_selected_rows(cx));
}
