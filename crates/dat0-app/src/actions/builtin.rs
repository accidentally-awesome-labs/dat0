//! Built-in action descriptors. Registered exactly once at boot via
//! `register_all(&registry)`.
//!
//! Each id in [`ids`] is a stable string referenced by Banner `action_id`
//! values (T2) and by the command palette (T6). Downstream tasks wire
//! the real dispatch bodies:
//!
//! - `file.open` — T7 empty-state hero / T11 menu wiring
//! - `window.new` — wired here; calls [`crate::window::spawn_window`]
//!   with the singletons installed in `run_app`
//! - `theme.toggle` — T12 (theme global + observe_global)
//! - `settings.open` — T13 (settings panel as window)
//! - `recents.show` — T7 (recents drawer)
//! - `recovery.review` — T5 (recovery panel)
//! - `import.cancel` — T10 (import wizard cancel button)
//! - `sample_data.retry_taxi` — T8 (empty-state hero re-fires fetch_remote)

use std::sync::Arc;

use super::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry, RegisterError};

/// Stable action ids. Banner `action_id` strings (see
/// `crate::error_ux::banner::BannerAction`) reference these constants.
pub mod ids {
    pub const FILE_OPEN: &str = "file.open";
    pub const WINDOW_NEW: &str = "window.new";
    pub const THEME_TOGGLE: &str = "theme.toggle";
    pub const SETTINGS_OPEN: &str = "settings.open";
    pub const RECENTS_SHOW: &str = "recents.show";
    pub const RECOVERY_REVIEW: &str = "recovery.review";
    pub const IMPORT_CANCEL: &str = "import.cancel";
    pub const SAMPLE_DATA_RETRY_TAXI: &str = "sample_data.retry_taxi";
    pub const VIEW_UNDO: &str = "view.undo";
    pub const VIEW_REDO: &str = "view.redo";
    // T9: edit/clipboard/bulk action ids.
    pub const VIEW_COPY: &str = "view.copy";
    pub const VIEW_CUT: &str = "view.cut";
    pub const VIEW_PASTE: &str = "view.paste";
    pub const VIEW_FILL_DOWN: &str = "view.fill_down";
    pub const VIEW_DELETE_ROWS: &str = "view.delete_rows";
    pub const VIEW_SET_NULL: &str = "view.set_null";
    pub const VIEW_SET_VALUE: &str = "view.set_value";
    pub const VIEW_DELETE_COLUMN: &str = "view.delete_column";
    // P4c T11: File → Export… dialog.
    pub const VIEW_EXPORT: &str = "view.export";
    // P5a T11: SQL Console entry points (toggle / run / cancel / tab lifecycle).
    pub const CONSOLE_TOGGLE: &str = "console.toggle";
    pub const SQL_RUN: &str = "sql.run";
    pub const SQL_CANCEL: &str = "sql.cancel";
    pub const SQL_NEW_TAB: &str = "sql.new_tab";
    pub const SQL_CLOSE_TAB: &str = "sql.close_tab";
    // P5b: SQL Console reuse/promotion actions.
    pub const SQL_SAVE_QUERY: &str = "sql.save_query";
    pub const SQL_LOAD_QUERY: &str = "sql.load_query";
    pub const SQL_HISTORY: &str = "sql.history";
    pub const SQL_SAVE_AS_TABLE: &str = "sql.save_as_table";
    pub const VIEW_SAVE_AS_TABLE: &str = "view.save_as_table";
    // P7a: Workspace open/save flows.
    pub const WORKSPACE_OPEN: &str = "workspace.open";
    pub const WORKSPACE_SAVE: &str = "workspace.save";
    // P7c T5: one-click re-import of an externally-changed source file.
    pub const LIVE_REFRESH: &str = "live.refresh";
}

/// Register every built-in action onto `reg`. Returns an error if any id
/// collides — boot calls `.expect("...")` because two built-ins sharing
/// an id is a programmer error, not a runtime condition.
pub fn register_all(reg: &ActionRegistry) -> Result<(), RegisterError> {
    reg.register(ActionDescriptor {
        id: ActionId::from(ids::FILE_OPEN),
        title: "Open File…".into(),
        group: ActionGroup::File,
        keybinding: None, // wired in menu_macos.rs / linux equivalent at T11
        dispatch: Arc::new(|app| {
            // T7 / T11 — Open File dialog. Stub: tracing call only;
            // real wiring lands at T7 (empty-state hero "Open File…"
            // button) and T11 (file dialog menu binding).
            tracing::info!("action: file.open dispatched (stub — T7/T11 wires real dialog)");
            let _ = app;
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::WINDOW_NEW),
        title: "New Window".into(),
        group: ActionGroup::Navigation,
        keybinding: None,
        dispatch: Arc::new(|app| {
            // P3b T1 reshaped `spawn_window` to take `(cx, &state_root,
            // registry)`. Both `state_root` and the shared
            // `Arc<Mutex<WindowRegistry>>` are installed as singletons in
            // `run_app` before `Application::run`. If either is missing
            // we log + bail rather than synthesise a fresh registry
            // (which would break the window-count invariant used by
            // T17 / single_instance.rs).
            let Some(state_root) = crate::window_registry::state_root() else {
                tracing::warn!("action: window.new — state_root singleton not installed; skipping");
                return;
            };
            let Some(registry) = crate::window_registry::window_registry() else {
                tracing::warn!(
                    "action: window.new — window_registry singleton not installed; skipping"
                );
                return;
            };
            crate::window::spawn_window(app, state_root, registry);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::THEME_TOGGLE),
        title: "Toggle Theme".into(),
        group: ActionGroup::Theme,
        keybinding: None,
        dispatch: Arc::new(|app| {
            // T12 wires the real toggle via cx.update_global once Theme
            // is promoted to a GPUI app-scoped global.
            tracing::info!("action: theme.toggle dispatched (stub — T12 wires real toggle)");
            let _ = app;
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::SETTINGS_OPEN),
        title: "Open Settings".into(),
        group: ActionGroup::Settings,
        keybinding: None,
        dispatch: Arc::new(|app| {
            crate::settings_ui::open_settings_window(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::RECENTS_SHOW),
        title: "Show Recents".into(),
        group: ActionGroup::Navigation,
        keybinding: None,
        dispatch: Arc::new(|app| {
            tracing::info!("action: recents.show dispatched (stub — T7 wires recents drawer)");
            let _ = app;
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::RECOVERY_REVIEW),
        title: "Review previous sessions".into(),
        group: ActionGroup::Recovery,
        keybinding: None,
        dispatch: Arc::new(|app| {
            crate::recovery_panel::open(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::IMPORT_CANCEL),
        title: "Cancel Import".into(),
        group: ActionGroup::Import,
        keybinding: None,
        dispatch: Arc::new(|app| {
            crate::import_progress::cancel_active(app);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::SAMPLE_DATA_RETRY_TAXI),
        title: "Retry NYC Taxi download".into(),
        group: ActionGroup::File,
        keybinding: None,
        dispatch: Arc::new(|app| {
            // T8 leaves dispatch as a tracing breadcrumb; the empty-state
            // hero "Try this sample" button (T7) owns the actual
            // re-fire of `sample_data::fetch_remote`. Banner-button
            // routing into the hero handler lands as a T7 follow-up
            // once the hero exposes a stable retry entrypoint.
            let _ = app;
            tracing::info!(
                "action: sample_data.retry_taxi dispatched (stub — T7 follow-up wires re-fetch)"
            );
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::WORKSPACE_OPEN),
        title: "Open Workspace…".into(),
        group: ActionGroup::File,
        keybinding: None,
        dispatch: Arc::new(crate::window::open_workspace_flow),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::WORKSPACE_SAVE),
        title: "Save Workspace…".into(),
        group: ActionGroup::File,
        keybinding: None,
        dispatch: Arc::new(crate::window::save_workspace_flow),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::LIVE_REFRESH),
        title: "Refresh from source".into(),
        group: ActionGroup::File,
        keybinding: None,
        // P7c: the live-data "Refresh" banner button fires this. T5 resolves
        // the focused workspace + calls the `run_refresh` stub; T6 fills in the
        // real re-import + replay flow.
        dispatch: Arc::new(crate::window::dispatch_live_refresh),
    })?;

    super::view_actions::register(reg)?;
    super::edit_actions::register(reg)?;
    super::sql_actions::register(reg)?;

    Ok(())
}
