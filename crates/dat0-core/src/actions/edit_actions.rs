//! Edit / clipboard / bulk-operation action descriptors.
//!
//! | id                   | shell handler                          |
//! |----------------------|----------------------------------------|
//! | `view.copy`          | `copy_selection`                       |
//! | `view.cut`           | `cut_selection`                        |
//! | `view.paste`         | `paste_clipboard`                      |
//! | `view.fill_down`     | `fill_down`                            |
//! | `view.set_null`      | `set_null_selection`                   |
//! | `view.set_value`     | `set_value_selection`                  |
//! | `view.delete_rows`   | `delete_selected_rows`                 |
//! | `view.delete_column` | `delete_column`                        |
//!
//! `view.set_value` and `view.delete_column` need an argument the palette
//! cannot supply (a scalar, a column index). The context menu calls the shell
//! directly for those; the descriptors exist for discoverability, and the
//! shell's router treats an argument-less invocation as a no-op.

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
