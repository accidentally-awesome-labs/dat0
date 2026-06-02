use dat0_app::view::pipeline_bar::describe_transform;
use dat0_engine::transform::{RowKey, Transformation};

#[test]
fn describes_each_variant() {
    assert_eq!(
        describe_transform(&Transformation::Rename {
            column: "a".into(),
            to: "A".into()
        }),
        "Rename a→A"
    );
    assert_eq!(
        describe_transform(&Transformation::DeleteColumn {
            columns: vec!["c".into()]
        }),
        "Delete col c"
    );
    assert_eq!(
        describe_transform(&Transformation::RowDelete {
            rows: vec![RowKey::Surrogate { id: 1 }]
        }),
        "Delete 1 row(s)"
    );
}
