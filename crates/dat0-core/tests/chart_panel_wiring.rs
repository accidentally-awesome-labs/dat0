//! End-to-end (headless): bind a chart panel to a seeded table, build the plot
//! SQL from the spec, run it through the engine, and extract a PlotTable.
use dat0_core::charts::data::PlotTable;
use dat0_core::charts::panel::ChartPanel;
use dat0_core::charts::query::build_plot_sql;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

#[tokio::test]
async fn visualize_builds_and_runs_plot_query() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("w.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    engine
        .create_table(
            "sales",
            "SELECT * FROM (VALUES ('West', 10.0), ('East', 20.0), ('West', 5.0)) v(region, amt)",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();

    let mut panel = ChartPanel::new();
    panel.bind(
        "\"sales\"".into(),
        vec![
            ("region".into(), "VARCHAR".into()),
            ("amt".into(), "DOUBLE".into()),
        ],
    );
    // simulate the user choosing a bar of region vs amt
    panel.spec.chart_type = dat0_core::charts::spec::ChartType::Bar;
    panel.spec.x = Some("region".into());
    panel.spec.y = Some("amt".into());

    let sql = build_plot_sql(&panel.spec).unwrap();
    let qr = engine.execute(&sql).await.unwrap();
    let pt = PlotTable::from_query_result(&qr);
    assert_eq!(pt.rows, 2, "two regions");
    assert!(pt.num("v").is_some(), "aggregated value column present");
    engine.close().await.unwrap();
}
