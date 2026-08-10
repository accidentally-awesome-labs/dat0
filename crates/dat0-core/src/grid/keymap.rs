//! Pure keyboard-to-selection mapping (T11, P4b a11y exit gate).
//!
//! This module is intentionally GPUI-free: `Key` and `apply_key` operate
//! entirely on [`SelectionModel`] so they are unit-testable without spinning
//! up a GPUI app context (see `tests/selection_keys.rs`).
//!
//! GPUI key-event → `Key` translation lives in `window.rs` and `grid/mod.rs`
//! (the `on_key_down` handlers).

use crate::grid::selection::SelectionModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    // ── Plain arrow keys (move active cell, new single-cell selection) ─────
    Up,
    Down,
    Left,
    Right,

    // ── Shift+arrow (extend last range without clearing) ──────────────────
    ShiftUp,
    ShiftDown,
    ShiftLeft,
    ShiftRight,

    // ── Cmd/Ctrl+arrow (jump to grid edge) ────────────────────────────────
    /// Cmd/Ctrl+Up → jump to row 0 (same column).
    JumpTop,
    /// Cmd/Ctrl+Down → jump to last row (same column).
    JumpBottom,
    /// Cmd/Ctrl+Left → jump to col 0 (same row).
    JumpLeft,
    /// Cmd/Ctrl+Right → jump to last col (same row).
    JumpRight,

    // ── Whole-grid / structural selections ────────────────────────────────
    /// Cmd/Ctrl+A → select entire grid.
    SelectAll,
    /// Select the entire row of the active cell.
    SelectRow,
    /// Select the entire column of the active cell.
    SelectColumn,

    // ── Clear ─────────────────────────────────────────────────────────────
    /// Escape → clear the selection.
    Escape,
}

/// Apply a logical `Key` action to `sel`, mutating it in-place.
///
/// This is the pure heart of the T11 keyboard map.  GPUI wires
/// `KeyDownEvent` → `Key` → `apply_key`; the function itself has no
/// knowledge of GPUI events.
pub fn apply_key(sel: &mut SelectionModel, key: Key) {
    match key {
        // ── Plain arrow moves ─────────────────────────────────────────────
        Key::Up => sel.move_active(-1, 0),
        Key::Down => sel.move_active(1, 0),
        Key::Left => sel.move_active(0, -1),
        Key::Right => sel.move_active(0, 1),

        // ── Shift+arrow extends ───────────────────────────────────────────
        Key::ShiftUp => sel.extend_active(-1, 0),
        Key::ShiftDown => sel.extend_active(1, 0),
        Key::ShiftLeft => sel.extend_active(0, -1),
        Key::ShiftRight => sel.extend_active(0, 1),

        // ── Cmd/Ctrl+arrow jumps (absolute move to edge) ──────────────────
        Key::JumpTop => {
            let col = sel.active().col;
            sel.move_active_to(0, col);
        }
        Key::JumpBottom => {
            let col = sel.active().col;
            let last_row = sel.rows().saturating_sub(1);
            sel.move_active_to(last_row, col);
        }
        Key::JumpLeft => {
            let row = sel.active().row;
            sel.move_active_to(row, 0);
        }
        Key::JumpRight => {
            let row = sel.active().row;
            let last_col = sel.cols().saturating_sub(1);
            sel.move_active_to(row, last_col);
        }

        // ── Structural selections ─────────────────────────────────────────
        Key::SelectAll => sel.select_all(),
        Key::SelectRow => {
            let row = sel.active().row;
            sel.select_row(row);
        }
        Key::SelectColumn => {
            let col = sel.active().col;
            sel.select_column(col);
        }

        // ── Clear ─────────────────────────────────────────────────────────
        Key::Escape => sel.clear(),
    }
}
