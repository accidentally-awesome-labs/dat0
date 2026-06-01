//! Round-trip + wire-format tests for `Edit`, `RowDelete`, `RowKey`, `CellEdit`.
//!
//! Forward-compatibility guard: `RowKey` is internally tagged on `"kind"` so
//! that P7 can add a `Pk { col, val }` variant without breaking existing
//! `Surrogate` rows in session.json / transforms.jsonl.
//!
//! PLAN-SNIPPET ADAPTATION: The P4b task description showed `RowKey::Surrogate(7)`
//! (tuple form). Serde's internally-tagged enum does NOT support newtype variants
//! wrapping a primitive (e.g. `Surrogate(i64)`) — it panics at compile time with
//! "cannot serialize tagged newtype variant ... containing an integer". The type
//! therefore uses a struct variant: `Surrogate { id: i64 }`. All test constructors
//! here use `RowKey::Surrogate { id: 7 }` accordingly.

use dat0_engine::{CellEdit, RowKey, Scalar, Transformation};

fn roundtrip(t: &Transformation) -> Transformation {
    let s = serde_json::to_string(t).unwrap();
    serde_json::from_str(&s).unwrap()
}

#[test]
fn edit_roundtrips_multi_cell() {
    let t = Transformation::Edit {
        cells: vec![
            CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "name".into(),
                value: Scalar::Str("Acme".into()),
            },
            CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "amt".into(),
                value: Scalar::Int(42),
            },
        ],
    };
    assert_eq!(roundtrip(&t), t);
    // RowKey is tagged so P7 can add Pk without breaking this row:
    let j = serde_json::to_value(&t).unwrap();
    assert_eq!(j["cells"][0]["row"]["kind"], "surrogate");
}

#[test]
fn row_delete_roundtrips() {
    let t = Transformation::RowDelete {
        rows: vec![RowKey::Surrogate { id: 1 }, RowKey::Surrogate { id: 9 }],
    };
    assert_eq!(roundtrip(&t), t);
    // Pin the wire token that P7/P8 deserializers branch on:
    let j = serde_json::to_value(&t).unwrap();
    assert_eq!(j["kind"], "row_delete");
}

#[test]
fn edit_kind_tag_is_self_describing() {
    let t = Transformation::Edit { cells: vec![] };
    let j = serde_json::to_value(&t).unwrap();
    assert_eq!(j["kind"], "edit");
}

#[test]
fn projection_variants_roundtrip_and_tag() {
    use dat0_engine::transform::Transformation;
    let reorder = Transformation::Reorder {
        columns: vec!["b".into(), "a".into()],
    };
    let rename = Transformation::Rename {
        column: "a".into(),
        to: "A".into(),
    };
    let delete = Transformation::DeleteColumn {
        columns: vec!["c".into()],
    };
    assert_eq!(roundtrip(&reorder), reorder);
    assert_eq!(roundtrip(&rename), rename);
    assert_eq!(roundtrip(&delete), delete);
    // Externally tagged on "kind", snake_case.
    assert_eq!(serde_json::to_value(&reorder).unwrap()["kind"], "reorder");
    assert_eq!(serde_json::to_value(&rename).unwrap()["kind"], "rename");
    assert_eq!(
        serde_json::to_value(&delete).unwrap()["kind"],
        "delete_column"
    );
}
