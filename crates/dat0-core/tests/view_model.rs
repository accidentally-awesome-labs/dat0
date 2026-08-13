//! Pure-logic tests for ViewModel — no engine round-trip.

use dat0_core::view::{HISTORY_CAP, ViewModel};
use dat0_engine::{FilterOp, FilterValue, Scalar, SortDirection, SortKey, Transformation};

fn vm() -> ViewModel {
    ViewModel::new("tab_test".into(), "\"main\".\"orders\"".into())
}

fn filter_eq(col: &str, v: i64) -> Transformation {
    Transformation::Filter {
        column: col.into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(v),
        },
    }
}

#[test]
fn new_view_has_empty_stack_and_no_active_view() {
    let v = vm();
    assert_eq!(v.stack().len(), 0);
    assert_eq!(v.cursor(), 0);
    assert!(v.active_view().is_none());
    assert!(!v.can_undo());
    assert!(!v.can_redo());
}

#[test]
fn apply_increments_cursor_and_returns_view_name() {
    let mut v = vm();
    let change = v.apply(filter_eq("a", 1));
    assert_eq!(v.stack().len(), 1);
    assert_eq!(v.cursor(), 1);
    assert!(v.active_view().is_some());
    assert!(change.new_active_view.is_some());
    assert!(change.previous_active_view.is_none());
    assert!(change.sql.is_some(), "non-empty stack must emit SQL");
}

#[test]
fn apply_then_undo_returns_to_base() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    let change = v.undo().expect("can undo from cursor 1");
    assert_eq!(v.cursor(), 0);
    assert!(v.active_view().is_none());
    assert!(
        change.new_active_view.is_none(),
        "undo to empty rebinds to base"
    );
    assert!(change.sql.is_none());
    assert!(change.previous_active_view.is_some());
}

#[test]
fn redo_after_undo_restores_view() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.undo();
    let change = v.redo().expect("can redo from cursor 0 with stack len 1");
    assert_eq!(v.cursor(), 1);
    assert!(v.active_view().is_some());
    assert!(change.new_active_view.is_some());
}

#[test]
fn apply_after_undo_truncates_redo_history() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.apply(filter_eq("b", 2));
    v.undo(); // cursor = 1, stack.len = 2
    v.apply(filter_eq("c", 3)); // branches off, drops the b=2 entry
    assert_eq!(v.stack().len(), 2);
    assert_eq!(v.cursor(), 2);
    assert!(!v.can_redo(), "redo history was truncated");
}

#[test]
fn undo_at_bottom_returns_none() {
    let mut v = vm();
    assert!(v.undo().is_none());
}

#[test]
fn redo_at_top_returns_none() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    assert!(v.redo().is_none());
}

#[test]
fn clear_drops_to_empty_and_one_undo_restores() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.apply(filter_eq("b", 2));
    v.clear();
    assert_eq!(v.cursor(), 0);
    assert!(v.active_view().is_none());
    // P4c zipper: clear() is a normal undoable structural edit — it checkpoints
    // the pre-clear present onto `past` (and clears the redo future). The whole
    // clear is therefore undone in one step via undo() (Design §5: "one undo
    // restores"). Redo is NOT used to restore a clear anymore.
    assert!(v.can_undo());
    let change = v.undo().unwrap();
    assert_eq!(
        v.cursor(),
        2,
        "one undo restores both ops cleared in one step"
    );
    assert!(change.new_active_view.is_some());
}

#[test]
fn replace_at_cursor_does_not_create_new_history_entry() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    let stack_len_before = v.stack().len();
    let cursor_before = v.cursor();
    v.replace_at_cursor(filter_eq("a", 99));
    assert_eq!(v.stack().len(), stack_len_before);
    assert_eq!(v.cursor(), cursor_before);
    match &v.stack()[v.cursor() - 1] {
        Transformation::Filter { value, .. } => {
            assert!(matches!(
                value,
                FilterValue::Scalar {
                    value: Scalar::Int(99)
                }
            ));
        }
        _ => panic!("expected Filter at cursor"),
    }
}

#[test]
fn replace_at_cursor_is_noop_when_empty() {
    let mut v = vm();
    let change = v.replace_at_cursor(filter_eq("a", 1));
    assert_eq!(v.cursor(), 0);
    assert!(change.new_active_view.is_none(), "no ops → bind to base");
}

#[test]
fn set_sort_appends_when_absent() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.set_sort(vec![SortKey {
        column: "ts".into(),
        direction: SortDirection::Desc,
    }]);
    assert_eq!(v.stack().len(), 2);
    assert_eq!(v.cursor(), 2);
    assert!(matches!(v.stack()[1], Transformation::Sort { .. }));
}

#[test]
fn set_sort_upserts_when_present() {
    let mut v = vm();
    v.set_sort(vec![SortKey {
        column: "a".into(),
        direction: SortDirection::Asc,
    }]);
    assert_eq!(v.stack().len(), 1);
    v.set_sort(vec![SortKey {
        column: "b".into(),
        direction: SortDirection::Desc,
    }]);
    // No new history entry — same stack length, sort op replaced in place.
    assert_eq!(v.stack().len(), 1);
    match &v.stack()[0] {
        Transformation::Sort { keys } => {
            assert_eq!(keys[0].column, "b");
            assert_eq!(keys[0].direction, SortDirection::Desc);
        }
        _ => panic!("expected Sort"),
    }
}

// ---------------------------------------------------------------------------
// set_filter tests (T0 — filter-popover edit-apply, column-aware upsert)
// ---------------------------------------------------------------------------

#[test]
fn set_filter_appends_when_column_absent() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.set_filter(filter_eq("b", 2));
    // New column → append.
    assert_eq!(v.stack().len(), 2);
    assert_eq!(v.cursor(), 2);
    match &v.stack()[1] {
        Transformation::Filter { column, .. } => assert_eq!(column, "b"),
        _ => panic!("expected Filter on b"),
    }
}

#[test]
fn set_filter_replaces_in_place_for_same_column() {
    let mut v = vm();
    v.set_filter(filter_eq("a", 1));
    assert_eq!(v.stack().len(), 1);
    // Re-edit the same column: replace in place, no new history entry.
    v.set_filter(filter_eq("a", 99));
    assert_eq!(v.stack().len(), 1, "same column must replace, not stack");
    assert_eq!(v.cursor(), 1, "replace is a single undo step");
    match &v.stack()[0] {
        Transformation::Filter { column, value, .. } => {
            assert_eq!(column, "a");
            assert!(matches!(
                value,
                FilterValue::Scalar {
                    value: Scalar::Int(99)
                }
            ));
        }
        _ => panic!("expected Filter on a"),
    }
}

#[test]
fn set_filter_replaces_filter_buried_under_a_sort() {
    let mut v = vm();
    // Filter on `a`, then a sort on top — the filter is no longer the TOP op,
    // so `replace_at_cursor` would be wrong; `set_filter` must find it by column.
    v.apply(filter_eq("a", 1));
    v.set_sort(vec![SortKey {
        column: "ts".into(),
        direction: SortDirection::Desc,
    }]);
    assert_eq!(v.stack().len(), 2);

    v.set_filter(filter_eq("a", 42));
    assert_eq!(
        v.stack().len(),
        2,
        "must replace the buried filter, not append a third op"
    );
    match &v.stack()[0] {
        Transformation::Filter { value, .. } => assert!(matches!(
            value,
            FilterValue::Scalar {
                value: Scalar::Int(42)
            }
        )),
        _ => panic!("expected the Filter on a to be replaced in place at index 0"),
    }
    assert!(
        matches!(v.stack()[1], Transformation::Sort { .. }),
        "sort op must be untouched"
    );
}

#[test]
fn history_cap_drops_oldest_when_exceeded() {
    // P4c zipper: HISTORY_CAP bounds the `past` (undo snapshots), not the active
    // `present` stack. Drive HISTORY_CAP + 1 applies, then verify the oldest
    // snapshot (the empty base state) was evicted: we can undo exactly
    // HISTORY_CAP times, and the earliest *retained* snapshot is non-empty so
    // undo can never reach the empty base again.
    let mut v = vm();
    // HISTORY_CAP + 1 applies → a=0 .. a=HISTORY_CAP on the present stack.
    for i in 0..=(HISTORY_CAP as i64) {
        v.apply(filter_eq("a", i));
    }
    assert_eq!(
        v.stack().len(),
        HISTORY_CAP + 1,
        "present holds every applied op (the cap is on `past`, not `present`)"
    );
    assert!(v.can_undo(), "snapshots remain after the cap eviction");

    // Undo HISTORY_CAP times — pops every retained snapshot.
    for _ in 0..HISTORY_CAP {
        assert!(
            v.can_undo(),
            "must be able to undo back through every snapshot"
        );
        v.undo();
    }

    // The oldest *retained* snapshot is [a=0] (the empty base snapshot was the
    // one evicted), so we land on a single-op present, not the empty base.
    assert_eq!(
        v.stack().len(),
        1,
        "earliest retained snapshot is the single-op [a=0], not the evicted empty base"
    );
    assert!(
        !v.can_undo(),
        "the empty base snapshot was evicted by the cap — cannot undo further"
    );
    match &v.stack()[0] {
        Transformation::Filter {
            value: FilterValue::Scalar {
                value: Scalar::Int(n),
            },
            ..
        } => {
            assert_eq!(
                *n, 0,
                "earliest retained op is a=0; the empty base was evicted"
            );
        }
        _ => panic!("unexpected first entry after cap"),
    }
}

#[test]
fn nonce_seq_increments_per_apply_so_view_names_unique() {
    let mut v = vm();
    let c1 = v.apply(filter_eq("a", 1));
    let name1 = c1.new_active_view.clone().unwrap();
    let c2 = v.apply(filter_eq("a", 2));
    let name2 = c2.new_active_view.clone().unwrap();
    assert_ne!(
        name1, name2,
        "successive applies must yield distinct view names"
    );
    // Previous-on-c2 must reference name1 so the caller can drop it after rebind.
    assert_eq!(c2.previous_active_view.as_deref(), Some(name1.as_str()));
}

#[test]
fn view_name_strips_unsafe_chars_from_tab_id() {
    let mut v = ViewModel::new("tab-7/with.special".into(), "\"main\".\"t\"".into());
    let change = v.apply(filter_eq("a", 1));
    let name = change.new_active_view.unwrap();
    assert!(name.starts_with("v_tab7withspecial_"));
}

// ---------------------------------------------------------------------------
// edit_cells / delete_rows / is_dirty tests (T6 — inline cell editor)
// ---------------------------------------------------------------------------

#[test]
fn edit_cells_pushes_one_undo_step_and_marks_dirty() {
    use dat0_engine::{CellEdit, RowKey, Scalar, Transformation};
    let mut vm = dat0_core::view::ViewModel::new("t".into(), "\"main\".\"t\"".into());
    assert!(!vm.is_dirty());
    let _ = vm.edit_cells(vec![CellEdit {
        row: RowKey::Surrogate { id: 1 },
        column: "a".into(),
        value: Scalar::Int(5),
    }]);
    assert!(vm.is_dirty());
    assert_eq!(vm.active().len(), 1);
    assert!(matches!(vm.active()[0], Transformation::Edit { .. }));
    vm.undo();
    assert!(!vm.is_dirty());
}

#[test]
fn delete_rows_pushes_edit_op_and_marks_dirty() {
    use dat0_engine::{RowKey, Transformation};
    let mut vm = dat0_core::view::ViewModel::new("t".into(), "\"main\".\"t\"".into());
    assert!(!vm.is_dirty());
    let _ = vm.delete_rows(vec![
        RowKey::Surrogate { id: 2 },
        RowKey::Surrogate { id: 3 },
    ]);
    assert!(vm.is_dirty());
    assert_eq!(vm.active().len(), 1);
    assert!(matches!(vm.active()[0], Transformation::RowDelete { .. }));
    vm.undo();
    assert!(!vm.is_dirty());
}

#[test]
fn is_dirty_is_false_for_filter_and_sort_only() {
    use dat0_engine::{SortDirection, SortKey};
    let mut vm = vm();
    vm.apply(filter_eq("a", 1));
    vm.set_sort(vec![SortKey {
        column: "a".into(),
        direction: SortDirection::Asc,
    }]);
    assert!(
        !vm.is_dirty(),
        "filter/sort-only stacks are not dirty (no Edit/RowDelete)"
    );
}

// ---------------------------------------------------------------------------
// find_filter_for tests (T10 — filter popover edit flow)
// ---------------------------------------------------------------------------

#[test]
fn find_filter_for_returns_most_recent() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.apply(filter_eq("b", 2));
    v.apply(filter_eq("a", 99)); // most recent filter on "a"
    let found = v.find_filter_for("a").unwrap();
    match found {
        Transformation::Filter { value, .. } => {
            assert!(
                matches!(
                    value,
                    FilterValue::Scalar {
                        value: Scalar::Int(99)
                    }
                ),
                "expected Scalar Int(99), got {value:?}"
            );
        }
        _ => panic!("expected Filter"),
    }
}

#[test]
fn find_filter_for_returns_none_when_absent() {
    let v = vm();
    assert!(v.find_filter_for("a").is_none(), "empty stack → None");
}

#[test]
fn find_filter_for_ignores_ops_beyond_cursor() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.apply(filter_eq("a", 2));
    // Undo once: cursor now at 1. Only filter_eq("a", 1) is active.
    v.undo();
    let found = v.find_filter_for("a").unwrap();
    assert!(
        matches!(
            found,
            Transformation::Filter {
                value: FilterValue::Scalar {
                    value: Scalar::Int(1)
                },
                ..
            }
        ),
        "must not see the undone op"
    );
}

#[test]
fn find_filter_for_returns_none_for_unknown_column() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    assert!(v.find_filter_for("z").is_none());
}

// ---------------------------------------------------------------------------
// current_sort_as_active tests (T12 — sort-header click handler)
// ---------------------------------------------------------------------------

#[test]
fn current_sort_as_active_returns_empty_when_no_sort() {
    let v = vm();
    let active = v.current_sort_as_active();
    assert!(active.keys.is_empty(), "no ops → empty ActiveSort");
}

#[test]
fn current_sort_as_active_returns_keys_from_sort_op() {
    let mut v = vm();
    v.set_sort(vec![SortKey {
        column: "price".into(),
        direction: SortDirection::Asc,
    }]);
    let active = v.current_sort_as_active();
    assert_eq!(active.find("price"), Some((1, SortDirection::Asc)));
}

#[test]
fn current_sort_as_active_ignores_ops_beyond_cursor() {
    let mut v = vm();
    v.set_sort(vec![SortKey {
        column: "a".into(),
        direction: SortDirection::Desc,
    }]);
    v.set_sort(vec![SortKey {
        column: "b".into(),
        direction: SortDirection::Asc,
    }]);
    // Undo: cursor steps back to the first sort (upsert replaced, so cursor = 1).
    v.undo();
    // After undo, cursor is 0 — no active sort.
    let active = v.current_sort_as_active();
    assert!(active.keys.is_empty(), "undone sort must not appear");
}
