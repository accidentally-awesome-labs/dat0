use dat0_app::view::fold_columns;
use dat0_engine::transform::{ProjectionColumn, Transformation};

fn base() -> Vec<String> {
    vec!["a".into(), "b".into(), "c".into()]
}

fn disp(folded: &[ProjectionColumn]) -> Vec<(String, String)> {
    folded
        .iter()
        .map(|c| (c.source.clone(), c.display.clone()))
        .collect()
}

#[test]
fn no_projection_ops_is_identity() {
    let folded = fold_columns(&base(), &[]);
    assert_eq!(
        disp(&folded),
        vec![
            ("a".into(), "a".into()),
            ("b".into(), "b".into()),
            ("c".into(), "c".into())
        ]
    );
}

#[test]
fn rename_sets_display_keeps_source_and_position() {
    let ops = vec![Transformation::Rename {
        column: "b".into(),
        to: "B!".into(),
    }];
    let folded = fold_columns(&base(), &ops);
    assert_eq!(disp(&folded)[1], ("b".into(), "B!".into()));
}

#[test]
fn delete_excludes_column() {
    let ops = vec![Transformation::DeleteColumn {
        columns: vec!["b".into()],
    }];
    let folded = fold_columns(&base(), &ops);
    assert_eq!(
        disp(&folded),
        vec![("a".into(), "a".into()), ("c".into(), "c".into())]
    );
}

#[test]
fn reorder_applies_full_visible_order_preserving_renames() {
    let ops = vec![
        Transformation::Rename {
            column: "a".into(),
            to: "A".into(),
        },
        Transformation::Reorder {
            columns: vec!["c".into(), "a".into(), "b".into()],
        },
    ];
    let folded = fold_columns(&base(), &ops);
    assert_eq!(
        disp(&folded),
        vec![
            ("c".into(), "c".into()),
            ("a".into(), "A".into()), // rename survives reorder
            ("b".into(), "b".into())
        ]
    );
}

#[test]
fn screen_col_maps_to_source_after_reorder() {
    use dat0_app::view::column_view::source_for_screen_col;
    let ops = vec![Transformation::Reorder {
        columns: vec!["c".into(), "a".into(), "b".into()],
    }];
    let folded = fold_columns(&base(), &ops);
    assert_eq!(source_for_screen_col(&folded, 0), Some("c"));
    assert_eq!(source_for_screen_col(&folded, 1), Some("a"));
    assert_eq!(source_for_screen_col(&folded, 2), Some("b"));
    assert_eq!(source_for_screen_col(&folded, 3), None);
}

#[test]
fn delete_then_reorder_omits_deleted() {
    let ops = vec![
        Transformation::DeleteColumn {
            columns: vec!["b".into()],
        },
        Transformation::Reorder {
            columns: vec!["c".into(), "a".into()],
        },
    ];
    let folded = fold_columns(&base(), &ops);
    assert_eq!(
        disp(&folded),
        vec![("c".into(), "c".into()), ("a".into(), "a".into())]
    );
}
