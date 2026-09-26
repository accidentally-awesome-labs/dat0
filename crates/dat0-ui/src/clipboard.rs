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
//!
//! # No system clipboard
//!
//! Headless CI and some locked-down desktops have none. A copy then lands in
//! [`SPARE`], inside this process, so copy and paste within dat0 still work;
//! other apps cannot see it.

use std::sync::{Mutex, OnceLock};

use arboard::Clipboard;

/// The process-wide handle. `Mutex` because `arboard::Clipboard` is `!Sync`
/// and copy can be reached from a menu, a keystroke and a context menu.
static CLIPBOARD: OnceLock<Option<Mutex<Clipboard>>> = OnceLock::new();

/// What was copied, when there is no system clipboard to hold it.
static SPARE: Mutex<Option<String>> = Mutex::new(None);

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
    let Some(cb) = handle() else {
        let Ok(mut spare) = SPARE.lock() else {
            return false;
        };
        *spare = Some(text.to_string());
        return true;
    };
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
    let Some(cb) = handle() else {
        return SPARE.lock().ok()?.clone();
    };
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
    fn a_copy_can_be_pasted_back_with_or_without_a_system_clipboard() {
        // Headless CI has no clipboard: the copy must neither panic nor
        // vanish, since the grid's paste reads it back.
        assert!(set_text("a\tb"));
        assert_eq!(text().as_deref(), Some("a\tb"));
        assert_eq!(
            paste_cells(),
            Some(vec![vec!["a".to_string(), "b".to_string()]])
        );
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
