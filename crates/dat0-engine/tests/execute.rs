use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn execute_returns_materialized_batches() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let qr = engine
        .execute("SELECT * FROM (VALUES (1, 'a'), (2, 'b'), (3, 'c')) v(id, name)")
        .await
        .unwrap();
    assert_eq!(qr.columns.len(), 2);
    assert!(qr.batches.iter().map(|b| b.num_rows()).sum::<usize>() == 3);
    engine.close().await.unwrap();
}

#[tokio::test]
async fn execute_propagates_sql_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine
        .execute("SELECT FROM not_a_thing")
        .await
        .expect_err("syntax error");
    assert!(matches!(err, dat0_engine::EngineError::DuckDb(_)));
    engine.close().await.unwrap();
}
