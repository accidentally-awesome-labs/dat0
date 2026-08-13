use dat0_core::view::export_dialog::{ExportScope, build_export};
use dat0_engine::render::render_export_select;
use dat0_engine::transform::ProjectionColumn;

#[test]
fn current_view_uses_active_view_and_projection() {
    let cv = vec![
        ProjectionColumn {
            source: "a".into(),
            display: "A".into(),
        },
        ProjectionColumn {
            source: "b".into(),
            display: "b".into(),
        },
    ];
    let (inner, cols) = build_export(
        ExportScope::CurrentView,
        "\"main\".\"orders\"",
        Some("\"v_tab1_3\""),
        &cv,
        &["a".into(), "b".into(), "c".into()],
    );
    let sql = render_export_select(&inner, &cols);
    assert_eq!(
        sql,
        "SELECT \"a\" AS \"A\", \"b\" FROM (SELECT * FROM \"v_tab1_3\")"
    );
}

#[test]
fn full_table_is_raw_base_columns() {
    let (inner, cols) = build_export(
        ExportScope::FullTable,
        "\"main\".\"orders\"",
        Some("\"v_tab1_3\""),
        &[],
        &["a".into(), "b".into()],
    );
    let sql = render_export_select(&inner, &cols);
    assert_eq!(
        sql,
        "SELECT \"a\", \"b\" FROM (SELECT * FROM \"main\".\"orders\")"
    );
}
