use dat0_core::grid::selection::{CellCoord, SelectionModel};

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

// ---------------------------------------------------------------------------
// B3: selected_cell_count — exact union area, computed without touching cells.
// ---------------------------------------------------------------------------

#[test]
fn selected_cell_count_is_zero_with_no_selection() {
    let s = SelectionModel::new(10, 10);
    assert_eq!(s.selected_cell_count(), 0);
}

#[test]
fn selected_cell_count_counts_a_single_cell() {
    let mut s = SelectionModel::new(10, 10);
    s.click(CellCoord { row: 2, col: 3 });
    assert_eq!(s.selected_cell_count(), 1);
}

#[test]
fn selected_cell_count_counts_a_rectangle() {
    let mut s = SelectionModel::new(10, 10);
    s.click(CellCoord { row: 1, col: 1 });
    s.extend_to(CellCoord { row: 3, col: 4 }); // 3 rows x 4 cols
    assert_eq!(s.selected_cell_count(), 12);
}

/// `extend_to` can leave `r0 > r1` / `c0 > c1`; the count must normalise.
#[test]
fn selected_cell_count_normalises_a_backwards_rectangle() {
    let mut s = SelectionModel::new(10, 10);
    s.click(CellCoord { row: 3, col: 4 });
    s.extend_to(CellCoord { row: 1, col: 1 });
    assert_eq!(s.selected_cell_count(), 12);
}

#[test]
fn selected_cell_count_sums_disjoint_ranges() {
    let mut s = SelectionModel::new(10, 10);
    s.click(CellCoord { row: 0, col: 0 });
    s.extend_to(CellCoord { row: 1, col: 1 }); // 4
    s.add_click(CellCoord { row: 5, col: 5 });
    s.extend_to(CellCoord { row: 5, col: 7 }); // 3
    assert_eq!(s.selected_cell_count(), 7);
}

/// The case a sum-of-areas implementation gets wrong: it would return 18.
#[test]
fn selected_cell_count_counts_overlapping_ranges_once() {
    let mut s = SelectionModel::new(10, 10);
    s.click(CellCoord { row: 0, col: 0 });
    s.extend_to(CellCoord { row: 2, col: 2 }); // 9
    s.add_click(CellCoord { row: 1, col: 1 });
    s.extend_to(CellCoord { row: 3, col: 3 }); // 9, sharing 4
    assert_eq!(s.selected_cell_count(), 14);
}

#[test]
fn selected_cell_count_ignores_a_click_inside_an_existing_range() {
    let mut s = SelectionModel::new(10, 10);
    s.click(CellCoord { row: 0, col: 0 });
    s.extend_to(CellCoord { row: 4, col: 4 }); // 25
    s.add_click(CellCoord { row: 2, col: 2 }); // already inside
    assert_eq!(s.selected_cell_count(), 25);
}

/// The whole reason the method exists. `select_all` spans the entire grid (the
/// model is built with `rows = GridDataSource::row_count`) and the status bar
/// reads this EVERY FRAME. A `resolved_cells().count()` implementation would
/// not fail this test — it would hang it.
#[test]
fn selected_cell_count_is_arithmetic_not_per_cell() {
    let mut s = SelectionModel::new(1_000_000, 20);
    s.select_all();
    assert_eq!(s.selected_cell_count(), 20_000_000);
}

#[test]
fn bounds_cover_every_range_whichever_way_it_was_dragged() {
    let mut s = SelectionModel::new(20, 10);
    assert_eq!(s.bounds(), None, "nothing selected");
    // Dragged up and to the left: the range's corners arrive reversed.
    s.click(CellCoord { row: 6, col: 4 });
    s.extend_to(CellCoord { row: 3, col: 2 });
    s.add_click(CellCoord { row: 9, col: 1 });
    let b = s.bounds().expect("a selection");
    assert_eq!((b.r0, b.c0, b.r1, b.c1), (3, 1, 9, 4));
}

#[test]
fn selected_rows_merge_overlapping_ranges_and_count_without_walking() {
    let mut s = SelectionModel::new(20, 10);
    s.click(CellCoord { row: 2, col: 0 });
    s.extend_to(CellCoord { row: 4, col: 3 }); // rows 2..=4
    s.add_click(CellCoord { row: 4, col: 7 }); // row 4 again
    s.add_click(CellCoord { row: 5, col: 9 }); // touches: 2..=5
    s.add_click(CellCoord { row: 11, col: 0 });
    assert_eq!(s.selected_rows(), vec![2, 3, 4, 5, 11]);
    assert_eq!(s.selected_row_count(), 5);
    assert_eq!(s.selected_cols(), vec![0, 1, 2, 3, 7, 9]);

    // A select-all's rows are counted, not listed.
    let mut all = SelectionModel::new(1_000_000, 20);
    all.select_all();
    assert_eq!(all.selected_row_count(), 1_000_000);
}
