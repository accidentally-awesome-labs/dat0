//! Pure-logic tests for ViewModel — no engine round-trip.

use dat0_app::view::{HISTORY_CAP, ViewModel};
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
    // Stack is preserved so one redo restores. (Design §5: "one undo restores".)
    // redo() from cursor==0 jumps to stack.len() so the whole clear is undone in one step.
    assert!(v.can_redo());
    let change = v.redo().unwrap();
    assert_eq!(v.cursor(), 2, "redo from clear restores both ops");
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

#[test]
fn history_cap_drops_oldest_when_exceeded() {
    let mut v = vm();
    for i in 0..HISTORY_CAP as i64 {
        v.apply(filter_eq("a", i));
    }
    assert_eq!(v.stack().len(), HISTORY_CAP);
    assert_eq!(v.cursor(), HISTORY_CAP);

    // One more — oldest entry drops, cursor stays at cap.
    v.apply(filter_eq("a", HISTORY_CAP as i64));
    assert_eq!(v.stack().len(), HISTORY_CAP);
    assert_eq!(v.cursor(), HISTORY_CAP);
    // The first entry (a=0) is gone; first surviving entry is a=1.
    match &v.stack()[0] {
        Transformation::Filter {
            value: FilterValue::Scalar {
                value: Scalar::Int(n),
            },
            ..
        } => {
            assert_eq!(*n, 1);
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
