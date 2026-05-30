use dat0_app::grid::selection::{CellCoord, SelectionModel};

#[test]
fn click_sets_single_cell() {
    let mut s = SelectionModel::new(10, 4); // rows, cols
    s.click(CellCoord { row: 2, col: 1 });
    assert_eq!(s.active(), CellCoord { row: 2, col: 1 });
    assert!(s.contains(2, 1));
    assert!(!s.contains(0, 0));
}

#[test]
fn shift_arrow_extends_range() {
    let mut s = SelectionModel::new(10, 4);
    s.click(CellCoord { row: 1, col: 1 });
    s.extend_to(CellCoord { row: 3, col: 2 }); // shift-extend from anchor
    for r in 1..=3 {
        for c in 1..=2 {
            assert!(s.contains(r, c), "{r},{c}");
        }
    }
    assert!(!s.contains(0, 0));
}

#[test]
fn cmd_click_adds_discontiguous_range() {
    let mut s = SelectionModel::new(10, 4);
    s.click(CellCoord { row: 0, col: 0 });
    s.add_click(CellCoord { row: 5, col: 3 });
    assert!(s.contains(0, 0));
    assert!(s.contains(5, 3));
    assert!(!s.contains(2, 2));
}

#[test]
fn select_row_and_column_and_all() {
    let mut s = SelectionModel::new(3, 3);
    s.select_row(1);
    for c in 0..3 {
        assert!(s.contains(1, c));
    }
    s.clear();
    s.select_column(2);
    for r in 0..3 {
        assert!(s.contains(r, 2));
    }
    s.clear();
    s.select_all();
    for r in 0..3 {
        for c in 0..3 {
            assert!(s.contains(r, c));
        }
    }
}

#[test]
fn resolved_cells_dedupes_overlap() {
    let mut s = SelectionModel::new(10, 4);
    s.click(CellCoord { row: 0, col: 0 });
    s.extend_to(CellCoord { row: 1, col: 1 });
    s.add_click(CellCoord { row: 1, col: 1 }); // overlaps
    let cells: Vec<_> = s.resolved_cells().collect();
    assert_eq!(cells.len(), 4); // (0,0)(0,1)(1,0)(1,1) — no dupes
}

// Extra: extend_to after add_click only reshapes the LAST range, not the first.
#[test]
fn extend_to_after_add_click_reshapes_only_last_range() {
    let mut s = SelectionModel::new(10, 4);
    s.click(CellCoord { row: 0, col: 0 }); // range[0]: (0,0)-(0,0)
    s.add_click(CellCoord { row: 5, col: 0 }); // range[1]: (5,0)-(5,0), anchor=(5,0)
    s.extend_to(CellCoord { row: 7, col: 2 }); // range[1] → (5,0)-(7,2); range[0] unchanged
    // First range still just the single click cell
    assert!(s.contains(0, 0));
    assert!(!s.contains(0, 2)); // first range didn't grow
    // Second range extended
    assert!(s.contains(5, 0));
    assert!(s.contains(6, 1));
    assert!(s.contains(7, 2));
}

// Extra: move_active clamps at grid edge (no panic).
#[test]
fn move_active_clamps_at_edge() {
    let mut s = SelectionModel::new(5, 3);
    s.click(CellCoord { row: 0, col: 0 });
    s.move_active(-5, -5); // should clamp to (0, 0), not panic
    assert_eq!(s.active(), CellCoord { row: 0, col: 0 });
    s.click(CellCoord { row: 4, col: 2 });
    s.move_active(99, 99); // should clamp to (4, 2), not panic
    assert_eq!(s.active(), CellCoord { row: 4, col: 2 });
}

// Extra: extend_active clamps at grid edge.
#[test]
fn extend_active_clamps_at_edge() {
    let mut s = SelectionModel::new(5, 3);
    s.click(CellCoord { row: 2, col: 1 });
    s.extend_active(99, 99); // clamp to (4, 2)
    assert_eq!(s.active(), CellCoord { row: 4, col: 2 });
    assert!(s.contains(2, 1));
    assert!(s.contains(4, 2));
}
