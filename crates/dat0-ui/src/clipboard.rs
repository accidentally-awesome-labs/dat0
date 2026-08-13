//! System clipboard access.
//!
//! `dioxus-desktop` provides none, so this wraps `arboard`.
//!
//! # One `Clipboard`, held forever
//!
//! The handle lives in a process-global `OnceLock` rather than being created
//! per copy. On X11 and Wayland the clipboard is not owned by the system: the
//! *copying process* serves the data on request, and dropping the `Clipboard`
//! withdraws the offer. A per-call handle therefore copies successfully and
//! then loses the contents the instant the function returns — on Linux only,
//! which is exactly the sort of thing that ships.
//!
//! The serialisation itself is `dat0_core::grid::clipboard`: TSV out, TSV in,
//! and per-column coercion on paste.

use std::sync::{Mutex, OnceLock};

use arboard::Clipboard;

/// The process-wide handle. `Mutex` because `arboard::Clipboard` is `!Sync`
/// and copy can be reached from a menu, a keystroke and a context menu.
static CLIPBOARD: OnceLock<Option<Mutex<Clipboard>>> = OnceLock::new();

fn handle() -> Option<&'static Mutex<Clipboard>> {
    CLIPBOARD
        .get_or_init(|| match Clipboard::new() {
            Ok(c) => Some(Mutex::new(c)),
            Err(e) => {
                // Headless CI and locked-down desktops have no clipboard.
                // Copy becoming a no-op is correct; taking the app down is not.
                tracing::warn!("no system clipboard available: {e}");
                None
            }
        })
        .as_ref()
}

/// Put text on the clipboard. Returns whether it landed.
pub fn set_text(text: &str) -> bool {
    let Some(cb) = handle() else { return false };
    let Ok(mut cb) = cb.lock() else { return false };
    match cb.set_text(text.to_string()) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("clipboard write failed: {e}");
            false
        }
    }
}

/// Read text from the clipboard. `None` when it is empty, holds something that
/// is not text, or there is no clipboard at all.
pub fn text() -> Option<String> {
    let cb = handle()?;
    let mut cb = cb.lock().ok()?;
    match cb.get_text() {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::debug!("clipboard read failed: {e}");
            None
        }
    }
}

/// Copy a rectangular block of cells as TSV — the format every spreadsheet
/// pastes.
pub fn copy_cells(grid: &[Vec<String>]) -> bool {
    set_text(&dat0_core::grid::clipboard::tsv_serialize(grid))
}

/// Read the clipboard as a rectangular block.
pub fn paste_cells() -> Option<Vec<Vec<String>>> {
    let t = text()?;
    if t.is_empty() {
        return None;
    }
    Some(dat0_core::grid::clipboard::tsv_parse(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_clipboard_degrades_to_a_no_op() {
        // Whatever the environment, these must not panic: headless CI has no
        // clipboard and a copy that aborts the process is worse than one that
        // does nothing.
        let _ = set_text("probe");
        let _ = text();
        let _ = paste_cells();
    }

    #[test]
    fn cells_round_trip_through_tsv() {
        // The serialisation contract, independent of whether a clipboard is
        // present — this is what a spreadsheet on the other end will read.
        let grid = vec![
            vec!["1".to_string(), "alpha".to_string()],
            vec!["2".to_string(), "bravo".to_string()],
        ];
        let tsv = dat0_core::grid::clipboard::tsv_serialize(&grid);
        // CRLF between rows, not LF: that is what Excel and Sheets emit, and
        // pasting into them is the whole reason this format was chosen.
        assert_eq!(tsv, "1\talpha\r\n2\tbravo");
        assert_eq!(dat0_core::grid::clipboard::tsv_parse(&tsv), grid);
    }
}
