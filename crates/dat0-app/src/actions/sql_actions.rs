//! P5b SQL Console action descriptors (save/load/history/save-as-table). The
//! Window-needing dispatches are breadcrumbs (handled view-scoped via buttons);
//! they exist so the command palette can surface the actions (D4).
//!
//! These mirror the P5a SQL Console descriptors in [`super::view_actions`]: the
//! real entry points are the view-scoped buttons (T5/T8/T10/T11) because that
//! work needs a `&mut Window` the registry `Fn(&mut App)` dispatch can't supply.
//! The descriptors here make the actions discoverable in the command palette
//! (T6 overlay) and log a breadcrumb when fired from the App path.

use std::sync::Arc;

use super::builtin::ids;
use super::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry, RegisterError};

/// Register the five P5b SQL Console reuse/promotion descriptors onto `reg`.
///
/// Called from [`super::builtin::register_all`] at app boot. Returns
/// [`RegisterError::DuplicateId`] if any id is already present — a programmer
/// error, not a runtime condition, so boot panics on it.
pub fn register(reg: &ActionRegistry) -> Result<(), RegisterError> {
    for (id, key) in [
        (ids::SQL_SAVE_QUERY, "sql.save_query"),
        (ids::SQL_LOAD_QUERY, "sql.load_query"),
        (ids::SQL_HISTORY, "sql.history"),
        (ids::SQL_SAVE_AS_TABLE, "sql.save_as_table"),
        (ids::VIEW_SAVE_AS_TABLE, "view.save_as_table"),
    ] {
        reg.register(ActionDescriptor {
            id: ActionId::from(id),
            title: dat0_i18n::t(key),
            group: ActionGroup::Edit,
            keybinding: None,
            dispatch: Arc::new(move |_app| {
                tracing::debug!(
                    "action: {id} dispatched via registry — handled view-scoped (needs Window)"
                );
            }),
        })?;
    }
    Ok(())
}
