//! `view.undo` + `view.redo` action descriptors.
//!
//! Dispatch closures are wired here but resolve to a `tracing::debug`
//! no-op until T13 plumbs focused-tab `ViewModel` lookup + engine
//! round-trip via `MainThreadDispatcher`. Tests assert registration only;
//! full dispatch coverage lands in T13's `view_lifecycle` e2e test.
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

/// Dispatch body for `view.undo`.
///
/// T13 replaces this stub with:
///   1. `crate::view::focused_view_model(cx)` — resolve the active tab's
///      `ViewModel` from the workspace shell.
///   2. `vm.undo()` — returns `Option<ViewChange>`.
///   3. If `Some(change)` → post `engine.create_or_replace_view(...)` via
///      `crate::window_registry::dispatcher()` + `MainThreadDispatcher`.
fn dispatch_undo(_app: &mut gpui::App) {
    // T13 wires focused-tab lookup + engine round-trip.
    // Dispatch body is intentionally a no-op for T7; the action is
    // discoverable in the palette and triggerable via keybind/menu.
    tracing::debug!("view.undo dispatched — T13 wires engine round-trip");
}

/// Dispatch body for `view.redo`.
///
/// T13 replaces this stub with the symmetric redo path (see `dispatch_undo`).
fn dispatch_redo(_app: &mut gpui::App) {
    tracing::debug!("view.redo dispatched — T13 wires engine round-trip");
}
