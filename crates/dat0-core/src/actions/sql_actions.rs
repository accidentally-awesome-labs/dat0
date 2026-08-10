//! SQL Console reuse / promotion action descriptors (save, load, history,
//! save-as-table).
//!
//! These are palette- and menu-reachable; the shell routes them to the focused
//! console.

use super::builtin::{descriptor, ids};
use super::registry::{ActionGroup, ActionRegistry, RegisterError};

/// Register the five reuse/promotion descriptors onto `reg`.
pub fn register(reg: &ActionRegistry) -> Result<(), RegisterError> {
    for id in [
        ids::SQL_SAVE_QUERY,
        ids::SQL_LOAD_QUERY,
        ids::SQL_HISTORY,
        ids::SQL_SAVE_AS_TABLE,
        ids::VIEW_SAVE_AS_TABLE,
    ] {
        reg.register(descriptor(id, dat0_i18n::t(id), ActionGroup::Edit))?;
    }
    Ok(())
}
