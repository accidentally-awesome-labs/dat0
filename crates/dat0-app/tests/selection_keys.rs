//! T11: keyboard-map unit tests over `SelectionModel` (pure-logic, no GPUI).
//!
//! These are the a11y exit-gate tests — the keyboard-only selection sweep
//! lives here before the full T14 manual UAT.

use dat0_app::grid::keymap::{Key, apply_key};
use dat0_app::grid::selection::{CellCoord, SelectionModel};

// ── Plan's required test ──────────────────────────────────────────────────────

#[test]
fn arrows_move_shift_extends_select_all() {
    let mut s = SelectionModel::new(5, 5);
    s.click(CellCoord { row: 0, col: 0 });

    // Down arrow → active moves to (1,0), (0,0) de-selected.
    apply_key(&mut s, Key::Down);
    assert!(s.contains(1, 0), "after Down: should contain (1,0)");
    assert!(!s.contains(0, 0), "after Down: (0,0) should be cleared");

    // Shift+Right → extends from (1,0) to (1,1).
    apply_key(&mut s, Key::ShiftRight);
    assert!(
        s.contains(1, 0),
        "after ShiftRight: anchor (1,0) still selected"
    );
    assert!(
        s.contains(1, 1),
        "after ShiftRight: (1,1) should be selected"
    );

    // Ctrl/Cmd+A → select all (5×5 grid).
    apply_key(&mut s, Key::SelectAll);
    assert!(s.contains(4, 4), "after SelectAll: (4,4) should be covered");
    assert!(s.contains(0, 0), "after SelectAll: (0,0) should be covered");

    // Escape → clear.
    apply_key(&mut s, Key::Escape);
    assert!(
        !s.contains(0, 0),
        "after Escape: selection should be cleared"
    );
    assert!(
        !s.contains(4, 4),
        "after Escape: selection should be cleared"
    );
}

// ── Additional coverage beyond the plan ──────────────────────────────────────

#[test]
fn arrow_keys_move_active_and_clear_selection() {
    let mut s = SelectionModel::new(4, 4);
    s.click(CellCoord { row: 2, col: 2 });

    apply_key(&mut s, Key::Up);
    assert!(s.contains(1, 2));
    assert!(!s.contains(2, 2));

    apply_key(&mut s, Key::Left);
    assert!(s.contains(1, 1));
    assert!(!s.contains(1, 2));

    apply_key(&mut s, Key::Down);
    assert!(s.contains(2, 1));

    apply_key(&mut s, Key::Right);
    assert!(s.contains(2, 2));
}

#[test]
fn shift_extend_all_four_directions() {
    let mut s = SelectionModel::new(5, 5);
    s.click(CellCoord { row: 2, col: 2 });

    // Shift+Up extends upward: range from (2,2) anchor to (1,2).
    apply_key(&mut s, Key::ShiftUp);
    assert!(s.contains(2, 2));
    assert!(s.contains(1, 2));
    assert!(!s.contains(0, 2));

    // Re-anchor at (2,2) and extend down.
    s.click(CellCoord { row: 2, col: 2 });
    apply_key(&mut s, Key::ShiftDown);
    assert!(s.contains(2, 2));
    assert!(s.contains(3, 2));

    // Re-anchor at (2,2) and extend left.
    s.click(CellCoord { row: 2, col: 2 });
    apply_key(&mut s, Key::ShiftLeft);
    assert!(s.contains(2, 2));
    assert!(s.contains(2, 1));

    // Re-anchor at (2,2) and extend right.
    s.click(CellCoord { row: 2, col: 2 });
    apply_key(&mut s, Key::ShiftRight);
    assert!(s.contains(2, 2));
    assert!(s.contains(2, 3));
}

#[test]
fn cmd_jump_to_edges() {
    let mut s = SelectionModel::new(5, 6);
    s.click(CellCoord { row: 2, col: 3 });

    // Jump to top (row 0, same col).
    apply_key(&mut s, Key::JumpTop);
    assert!(s.contains(0, 3));
    assert!(!s.contains(2, 3));

    // Jump to bottom (last row = 4, col 3).
    apply_key(&mut s, Key::JumpBottom);
    assert!(s.contains(4, 3));
    assert!(!s.contains(0, 3));

    // Jump to left (col 0, same row 4).
    apply_key(&mut s, Key::JumpLeft);
    assert!(s.contains(4, 0));
    assert!(!s.contains(4, 3));

    // Jump to right (last col = 5, row 4).
    apply_key(&mut s, Key::JumpRight);
    assert!(s.contains(4, 5));
    assert!(!s.contains(4, 0));
}

#[test]
fn select_row_and_select_column() {
    let mut s = SelectionModel::new(4, 5);
    s.click(CellCoord { row: 1, col: 2 });

    apply_key(&mut s, Key::SelectRow);
    // All columns in row 1 should be selected.
    for c in 0..5 {
        assert!(
            s.contains(1, c),
            "SelectRow: col {c} of row 1 should be selected"
        );
    }
    // Other rows should not be selected.
    assert!(!s.contains(0, 0));
    assert!(!s.contains(2, 0));

    // Now select the column the active cell is in.
    // After SelectRow, active is (1, 0) per select_row impl.
    // Re-click to set active at (2, 3).
    s.click(CellCoord { row: 2, col: 3 });
    apply_key(&mut s, Key::SelectColumn);
    // All rows in col 3 should be selected.
    for r in 0..4 {
        assert!(
            s.contains(r, 3),
            "SelectColumn: row {r} of col 3 should be selected"
        );
    }
    assert!(!s.contains(0, 0));
    assert!(!s.contains(0, 2));
}

#[test]
fn escape_clears_selection() {
    let mut s = SelectionModel::new(3, 3);
    s.click(CellCoord { row: 1, col: 1 });
    assert!(s.contains(1, 1));

    apply_key(&mut s, Key::Escape);
    assert!(!s.contains(0, 0));
    assert!(!s.contains(1, 1));
    assert!(!s.contains(2, 2));
}

#[test]
fn arrows_clamp_at_grid_edges() {
    let mut s = SelectionModel::new(3, 3);
    s.click(CellCoord { row: 0, col: 0 });

    // Moving up from row 0 stays at row 0.
    apply_key(&mut s, Key::Up);
    assert!(s.contains(0, 0));

    // Moving left from col 0 stays at col 0.
    apply_key(&mut s, Key::Left);
    assert!(s.contains(0, 0));

    // Move to bottom-right corner.
    s.click(CellCoord { row: 2, col: 2 });
    apply_key(&mut s, Key::Down);
    assert!(s.contains(2, 2));

    apply_key(&mut s, Key::Right);
    assert!(s.contains(2, 2));
}

#[test]
fn jump_edges_on_already_at_edge() {
    let mut s = SelectionModel::new(3, 4);
    s.click(CellCoord { row: 0, col: 0 });

    // JumpTop from row 0 stays at (0,0).
    apply_key(&mut s, Key::JumpTop);
    assert!(s.contains(0, 0));

    // JumpLeft from col 0 stays at (0,0).
    apply_key(&mut s, Key::JumpLeft);
    assert!(s.contains(0, 0));
}

#[test]
fn select_all_covers_entire_grid() {
    let mut s = SelectionModel::new(3, 4);
    s.click(CellCoord { row: 1, col: 2 });

    apply_key(&mut s, Key::SelectAll);
    for r in 0..3 {
        for c in 0..4 {
            assert!(s.contains(r, c), "SelectAll: ({r},{c}) should be covered");
        }
    }
}

#[test]
fn multiple_shift_extends_grow_range() {
    let mut s = SelectionModel::new(5, 5);
    s.click(CellCoord { row: 2, col: 2 });

    apply_key(&mut s, Key::ShiftRight);
    apply_key(&mut s, Key::ShiftRight);
    // Range should now span (2,2)→(2,4).
    assert!(s.contains(2, 2));
    assert!(s.contains(2, 3));
    assert!(s.contains(2, 4));
    assert!(!s.contains(2, 5)); // clamped (last col is 4)
}
