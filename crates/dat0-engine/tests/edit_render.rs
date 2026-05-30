//! Golden + parity tests for the P4b edit/delete overlay in compile_view_sql.
//!
//! The headline regression guard is `no_edits_emits_flat_p4a_sql`: when the op
//! stack contains no Edit/RowDelete, the emitted SQL MUST be byte-identical to
//! P4a's flat `SELECT * FROM <base> [WHERE ...] [ORDER BY ...]` form.
//!
//! NOTE: `RowKey` is a struct variant (`RowKey::Surrogate { id: N }`), not the
//! tuple form — see crates/dat0-engine/src/transform.rs.

use dat0_engine::{
    CellEdit, FilterOp, FilterValue, RowKey, Scalar, SortDirection, SortKey, Transformation,
    compile_view_sql,
};

const BASE: &str = "\"main\".\"orders\"";

#[test]
fn no_edits_emits_flat_p4a_sql() {
    // Parity guard: a filter+sort-only stack must produce the exact P4a flat form.
    let ops = vec![
        Transformation::Filter {
            column: "amt".into(),
            op: FilterOp::Gt,
            value: FilterValue::Scalar {
                value: Scalar::Int(10),
            },
        },
        Transformation::Sort {
            keys: vec![SortKey {
                column: "name".into(),
                direction: SortDirection::Asc,
            }],
        },
    ];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM \"main\".\"orders\" WHERE (\"amt\" > 10) ORDER BY \"name\" ASC"
    );
}

#[test]
fn edit_renders_replace_overlay() {
    let ops = vec![Transformation::Edit {
        cells: vec![CellEdit {
            row: RowKey::Surrogate { id: 7 },
            column: "name".into(),
            value: Scalar::Str("Acme".into()),
        }],
    }];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert!(sql.contains("SELECT * REPLACE ("), "got: {sql}");
    assert!(
        sql.contains("CASE WHEN __dat0_rowid = 7 THEN 'Acme' ELSE \"name\" END AS \"name\""),
        "got: {sql}"
    );
}

#[test]
fn row_delete_renders_not_in() {
    let ops = vec![Transformation::RowDelete {
        rows: vec![RowKey::Surrogate { id: 1 }, RowKey::Surrogate { id: 9 }],
    }];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert!(sql.contains("WHERE __dat0_rowid NOT IN (1, 9)"), "got: {sql}");
}

#[test]
fn edit_then_filter_sees_edited_value() {
    // Overlay is inner; filter is outer → filter applies to the edited projection.
    let ops = vec![
        Transformation::Edit {
            cells: vec![CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "amt".into(),
                value: Scalar::Int(99),
            }],
        },
        Transformation::Filter {
            column: "amt".into(),
            op: FilterOp::Gt,
            value: FilterValue::Scalar {
                value: Scalar::Int(50),
            },
        },
    ];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert!(sql.starts_with("SELECT * FROM (SELECT * REPLACE"), "got: {sql}");
    assert!(sql.contains(") WHERE (\"amt\" > 50)"), "got: {sql}");
}

#[test]
fn last_write_wins_per_cell() {
    let ops = vec![
        Transformation::Edit {
            cells: vec![CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "name".into(),
                value: Scalar::Str("first".into()),
            }],
        },
        Transformation::Edit {
            cells: vec![CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "name".into(),
                value: Scalar::Str("second".into()),
            }],
        },
    ];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert!(sql.contains("THEN 'second'"), "got: {sql}");
    assert!(!sql.contains("'first'"), "got: {sql}");
}

#[test]
fn empty_edit_is_render_error() {
    let ops = vec![Transformation::Edit { cells: vec![] }];
    assert!(compile_view_sql(BASE, &ops).is_err());
}

#[test]
fn edited_string_literal_is_single_quote_escaped() {
    // Guard: edited string values route through the same escaping as filters
    // (single-quote doubling), so an apostrophe cannot break out of the literal.
    let ops = vec![Transformation::Edit {
        cells: vec![CellEdit {
            row: RowKey::Surrogate { id: 7 },
            column: "name".into(),
            value: Scalar::Str("O'Brien".into()),
        }],
    }];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert!(sql.contains("THEN 'O''Brien'"), "got: {sql}");
    assert!(!sql.contains("'O'Brien'"), "unescaped quote leaked: {sql}");
}

#[test]
fn edit_and_delete_coexist_in_one_overlay() {
    // Edit + RowDelete in the same stack → one inner subquery carrying both the
    // REPLACE CASE projection and the NOT IN row filter.
    let ops = vec![
        Transformation::Edit {
            cells: vec![CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "name".into(),
                value: Scalar::Str("Acme".into()),
            }],
        },
        Transformation::RowDelete {
            rows: vec![RowKey::Surrogate { id: 5 }],
        },
    ];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert!(sql.contains("SELECT * REPLACE ("), "got: {sql}");
    assert!(
        sql.contains("CASE WHEN __dat0_rowid = 7 THEN 'Acme' ELSE \"name\" END AS \"name\""),
        "got: {sql}"
    );
    assert!(sql.contains("WHERE __dat0_rowid NOT IN (5)"), "got: {sql}");
}

#[test]
fn multi_column_edit_preserves_first_seen_order() {
    // Determinism contract: REPLACE columns render in first-seen order (the
    // golden SQL pins exact text, so any reordering would be a flake).
    let ops = vec![Transformation::Edit {
        cells: vec![
            CellEdit {
                row: RowKey::Surrogate { id: 1 },
                column: "b".into(),
                value: Scalar::Int(2),
            },
            CellEdit {
                row: RowKey::Surrogate { id: 1 },
                column: "a".into(),
                value: Scalar::Int(3),
            },
        ],
    }];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    let b_pos = sql.find("AS \"b\"").expect("col b present");
    let a_pos = sql.find("AS \"a\"").expect("col a present");
    assert!(b_pos < a_pos, "expected b before a (first-seen order), got: {sql}");
}
