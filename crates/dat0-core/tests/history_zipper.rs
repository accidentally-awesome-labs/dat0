use dat0_core::view::ViewModel;
use dat0_engine::transform::{FilterOp, FilterValue, Scalar, Transformation};

fn vm() -> ViewModel {
    ViewModel::new("t".into(), "\"main\".\"orders\"".into())
}

fn filter_eq(col: &str, n: i64) -> Transformation {
    Transformation::Filter {
        column: col.into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(n),
        },
    }
}

#[test]
fn apply_then_remove_at_drops_middle_and_is_undoable() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.apply(filter_eq("a", 2));
    v.apply(filter_eq("a", 3));
    assert_eq!(v.active().len(), 3);

    // Remove the middle op (index 1 → a=2).
    v.remove_at(1);
    assert_eq!(v.active().len(), 2);
    assert_eq!(v.active()[0], filter_eq("a", 1));
    assert_eq!(v.active()[1], filter_eq("a", 3));

    // Undo restores the removed op.
    v.undo();
    assert_eq!(v.active().len(), 3);
    assert_eq!(v.active()[1], filter_eq("a", 2));
}

#[test]
fn jump_to_truncates_and_is_undoable() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.apply(filter_eq("a", 2));
    v.apply(filter_eq("a", 3));
    v.jump_to(1); // keep first 1 op
    assert_eq!(v.active().len(), 1);
    assert_eq!(v.active()[0], filter_eq("a", 1));
    v.undo();
    assert_eq!(v.active().len(), 3);
}

#[test]
fn projection_apply_is_display_only_no_sql() {
    let mut v = vm();
    v.apply(filter_eq("a", 1)); // establishes an active view
    let change = v.apply(Transformation::Rename {
        column: "a".into(),
        to: "A".into(),
    });
    assert!(
        change.is_display_only(),
        "rename must recompile to identical data SQL → display-only, got {change:?}"
    );
    assert!(change.sql.is_none());
    assert_eq!(v.active().len(), 2);
}

#[test]
fn undo_of_projection_op_is_display_only_with_no_drop() {
    // Filter establishes an active view + its SQL. Applying a Rename recompiles
    // to byte-identical data SQL (projection ops are display-only in Option B),
    // so the view is unchanged. Undoing the Rename ALSO recompiles to the same
    // SQL → the undo emits a display-only ViewChange. Critically it must carry
    // `previous_active_view: None`: the view stays bound, nothing may be dropped.
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.apply(Transformation::Rename {
        column: "a".into(),
        to: "A".into(),
    });
    let change = v.undo().expect("undo must yield a ViewChange");
    assert!(
        change.is_display_only(),
        "undo of a projection op recompiles to identical SQL → display-only, got {change:?}"
    );
    assert!(change.sql.is_none(), "display-only change carries no SQL");
    assert_eq!(
        change.previous_active_view, None,
        "display-only undo must not drop any view: previous_active_view must be None"
    );
    assert!(
        change.new_active_view.is_some(),
        "display-only change keeps the same active view bound"
    );
    // The redo is symmetric: re-applying the Rename is also display-only.
    let redo = v.redo().expect("redo must yield a ViewChange");
    assert!(
        redo.is_display_only(),
        "redo of a projection op is display-only too"
    );
    assert_eq!(redo.previous_active_view, None);
}

#[test]
fn undo_of_redundant_op_is_display_only() {
    // Two identical sort applies: the second checkpoints but produces the same
    // compiled SQL. Undoing the second op restores a `present` that recompiles
    // to byte-identical SQL → display-only change, no drop.
    use dat0_engine::transform::{SortDirection, SortKey};
    let sort = || Transformation::Sort {
        keys: vec![SortKey {
            column: "a".into(),
            direction: SortDirection::Asc,
        }],
    };
    let mut v = vm();
    v.apply(sort());
    v.apply(sort()); // redundant — same compiled SQL as one sort
    let change = v.undo().expect("undo must yield a ViewChange");
    assert!(
        change.is_display_only(),
        "undo of a redundant op recompiles to identical SQL → display-only, got {change:?}"
    );
    assert_eq!(change.previous_active_view, None);
}

#[test]
fn apply_clears_redo_future() {
    let mut v = vm();
    v.apply(filter_eq("a", 1));
    v.undo();
    assert!(v.can_redo());
    v.apply(filter_eq("a", 2)); // diverge
    assert!(
        !v.can_redo(),
        "applying after undo must clear the redo future"
    );
}

#[test]
fn remove_rename_keeps_later_filter_valid() {
    // Removing a Rename must not orphan a later Filter (source-identity binding).
    let mut v = vm();
    v.apply(Transformation::Rename {
        column: "a".into(),
        to: "A".into(),
    });
    v.apply(filter_eq("a", 1)); // filter binds source "a", not display "A"
    v.remove_at(0); // drop the rename
    assert_eq!(v.active().len(), 1);
    assert_eq!(v.active()[0], filter_eq("a", 1)); // filter intact + still on "a"
}
