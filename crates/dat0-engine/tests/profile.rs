use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 128 * 1024 * 1024,
    }
}

#[tokio::test]
async fn profile_table_maps_numeric_and_string() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(tmp.path().join("p.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    engine
        .create_table(
            "orders",
            "SELECT * FROM (VALUES (10.0,'paid'),(20.0,'open'),(NULL,'paid'),(40.0,'paid')) AS v(amount, status)",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();

    let prof = engine.profile_table("orders", None).await.expect("profile");
    let amount = prof
        .columns
        .iter()
        .find(|c| c.name == "amount")
        .expect("amount col");
    assert!(amount.numeric.is_some(), "numeric stats present");
    let n = amount.numeric.as_ref().unwrap();
    assert_eq!(n.min, 10.0);
    assert_eq!(n.max, 40.0);
    assert!(
        (amount.null_pct - 25.0).abs() < 0.01,
        "1 of 4 null → 25%"
    );

    let status = prof
        .columns
        .iter()
        .find(|c| c.name == "status")
        .expect("status col");
    assert!(status.numeric.is_none(), "string col has no numeric stats");
    assert!(status.approx_distinct >= 2, "paid/open distinct");
    engine.close().await.unwrap();
}
