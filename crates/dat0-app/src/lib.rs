//! dat0 desktop application library (internal API surface).

pub mod actions;
pub mod app_lock;
pub mod boot;
pub mod error_ux;
pub mod file_drop;
pub mod grid;
pub mod main_bridge;
pub mod menu_macos;
pub mod platform;
pub mod recents;
pub mod recovery_panel;
pub mod session;
pub mod settings;
pub mod settings_ui;
pub mod telemetry;
pub mod theme;
pub mod updater;
pub mod window;
pub mod window_registry;

pub use window::run_app;

/// Stub for active-import cancellation — T10 (P3b) replaces with the
/// real cancel-token + `engine.interrupt(handle)` path driven by the
/// import wizard. Built-in action `import.cancel` (see
/// [`actions::builtin::ids::IMPORT_CANCEL`]) dispatches into this module
/// so the palette can resolve the id today without blocking on T10.
pub mod import_progress {
    /// Cancel the currently-active import, if any. T3 ships a no-op
    /// tracing call; T10 wires the real cancel.
    pub fn cancel_active(_app: &mut gpui::App) {
        tracing::info!("import_progress::cancel_active stub — T10 wires the real cancel");
    }
}
