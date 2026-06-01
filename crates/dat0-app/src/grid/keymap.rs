//! Pure keyboard-to-selection mapping (T11, P4b a11y exit gate).
//!
//! This module is intentionally GPUI-free: `Key` and `apply_key` operate
//! entirely on [`SelectionModel`] so they are unit-testable without spinning
//! up a GPUI app context (see `tests/selection_keys.rs`).
//!
//! GPUI key-event → `Key` translation lives in `window.rs` and `grid/mod.rs`
//! (the `on_key_down` handlers).

use super::selection::SelectionModel;

/// Logical keyboard action affecting grid selection.
///
/// Variants are one-to-one with the keystrokes the GPUI handler recognises.
/// Plain arrows → `move_active`; Shift+arrows → `extend_active`;
/// Cmd/Ctrl+arrows → jump to grid edge; special actions for SelectAll,
/// SelectRow, SelectColumn, Escape.
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

/// Translate a GPUI [`gpui::KeyDownEvent`] into a logical [`Key`], if the
/// keystroke maps to one of the grid navigation keys.
///
/// Returns `None` for keystrokes that are not handled here (e.g. Enter, F2,
/// Cmd+C — those are handled separately by the caller via direct dispatch
/// to the appropriate `WorkspaceShell` method).
///
/// `secondary` on macOS is the platform (Cmd) key; on Linux/Windows it is
/// the control key.  [`gpui::Modifiers::secondary`] encodes this
/// platform-conditional correctly.
pub fn key_from_event(event: &gpui::KeyDownEvent) -> Option<Key> {
    let ks = &event.keystroke;
    let mods = &ks.modifiers;
    let key = ks.key.as_str();

    // Secondary = Cmd on macOS, Ctrl on Linux/Windows.
    let secondary = mods.secondary();
    let shift_only = mods.shift && !mods.platform && !mods.control && !mods.alt;
    let secondary_only = secondary && !mods.shift && !mods.alt;
    let no_mods = !mods.shift && !mods.platform && !mods.control && !mods.alt;

    match key {
        "up" if no_mods => Some(Key::Up),
        "down" if no_mods => Some(Key::Down),
        "left" if no_mods => Some(Key::Left),
        "right" if no_mods => Some(Key::Right),

        "up" if shift_only => Some(Key::ShiftUp),
        "down" if shift_only => Some(Key::ShiftDown),
        "left" if shift_only => Some(Key::ShiftLeft),
        "right" if shift_only => Some(Key::ShiftRight),

        "up" if secondary_only => Some(Key::JumpTop),
        "down" if secondary_only => Some(Key::JumpBottom),
        "left" if secondary_only => Some(Key::JumpLeft),
        "right" if secondary_only => Some(Key::JumpRight),

        "escape" if no_mods => Some(Key::Escape),

        // Ctrl/Cmd+A → SelectAll.
        "a" if secondary_only => Some(Key::SelectAll),

        _ => None,
    }
}
