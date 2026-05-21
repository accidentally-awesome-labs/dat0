use std::time::Duration;

use dat0_engine::{DuckDBEngine, EngineStatus, MemoryBudget, QueryEngine};

/// Extract the first column, first row as a String from a one-cell query.
async fn scalar(engine: &DuckDBEngine, sql: &str) -> String {
    let res = engine.execute(sql).await.expect("execute");
    let batch = res.batches.first().expect("at least one batch");
    let col = batch.column(0);
    let arr = col
        .as_any()
        .downcast_ref::<duckdb::arrow::array::StringArray>()
        .expect("StringArray");
    arr.value(0).to_string()
}

fn budget_512mb() -> MemoryBudget {
    MemoryBudget {
        bytes: 512 * 1024 * 1024,
    }
}

#[tokio::test]
async fn engine_status_starts_initializing_then_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget_512mb()).unwrap();
    assert_eq!(engine.status(), EngineStatus::Initializing);
    engine.init().await.unwrap();
    assert_eq!(engine.status(), EngineStatus::Ready);
    engine.close().await.unwrap();
    assert_eq!(engine.status(), EngineStatus::Closed);
}

#[tokio::test]
async fn engine_init_applies_memory_pragma() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let budget = MemoryBudget {
        bytes: 1024 * 1024 * 1024,
    }; // 1 GB
    let engine = DuckDBEngine::new(scratch.clone(), budget).unwrap();
    engine.init().await.unwrap();

    let limit = scalar(&engine, "SELECT current_setting('memory_limit')").await;
    // DuckDB normalizes; expect "1.0 GiB" or similar.
    assert!(limit.contains("GiB") || limit.contains("MiB"));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn engine_rejects_ops_after_close() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget_512mb()).unwrap();
    engine.init().await.unwrap();
    engine.close().await.unwrap();
    let err = engine
        .execute("SELECT 1")
        .await
        .expect_err("must reject ops after close");
    assert!(matches!(err, dat0_engine::EngineError::EngineClosed));
}

#[tokio::test]
async fn engine_rejects_double_init() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget_512mb()).unwrap();
    engine.init().await.unwrap();
    let err = engine.init().await.expect_err("second init must fail");
    assert!(matches!(err, dat0_engine::EngineError::EngineFailed(_)));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn interrupt_handle_is_clonable_cross_thread() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget_512mb()).unwrap();
    engine.init().await.unwrap();

    // Sanity check: interrupt() must be callable from a sibling task without
    // holding the connection lock.
    let engine_arc = std::sync::Arc::new(engine);
    let e2 = engine_arc.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        e2.interrupt();
    });
    handle.await.unwrap();
    engine_arc.close().await.unwrap();
}
