use std::path::PathBuf;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};

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

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

#[tokio::test]
async fn register_parquet_basic() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.parquet"), RegisterOpts::default())
        .await
        .unwrap();
    assert!(info.columns.iter().any(|c| c.name == "id"));
    let v = scalar(
        &engine,
        &format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name),
    )
    .await;
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}
