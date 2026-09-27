//! Edit / clipboard / bulk-operation action descriptors.
//!
//! Each acts on the grid's selection, through `dat0-ui`'s
//! `components::grid::edits`:
//!
//! | id                   | effect                                           |
//! |----------------------|--------------------------------------------------|
//! | `view.copy`          | the selection, as TSV, to the clipboard          |
//! | `view.cut`           | copy, then set the selection to NULL             |
//! | `view.paste`         | the clipboard's block, at the active cell        |
//! | `view.fill_down`     | each column's top selected value, downward       |
//! | `view.set_null`      | the selection to NULL                            |
//! | `view.set_value`     | asks for a value, then sets the selection to it  |
//! | `view.delete_rows`   | every row holding a selected cell                |
//! | `view.delete_column` | hides every column holding a selected cell       |

use super::builtin::{descriptor, ids};
use super::registry::{ActionGroup, ActionRegistry, RegisterError};

/// Register all edit / clipboard / bulk actions onto `reg`.
pub fn register(reg: &ActionRegistry) -> Result<(), RegisterError> {
    for (id, title) in [
        (ids::VIEW_COPY, "Copy"),
        (ids::VIEW_CUT, "Cut"),
        (ids::VIEW_PASTE, "Paste"),
        (ids::VIEW_FILL_DOWN, "Fill Down"),
        (ids::VIEW_SET_NULL, "Set NULL"),
        (ids::VIEW_SET_VALUE, "Set Value\u{2026}"),
        (ids::VIEW_DELETE_ROWS, "Delete Row(s)"),
        (ids::VIEW_DELETE_COLUMN, "Delete Column"),
    ] {
        reg.register(descriptor(id, title, ActionGroup::Edit))?;
    }

    Ok(())
}
