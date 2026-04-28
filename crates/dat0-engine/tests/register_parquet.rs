#![allow(deprecated)] // __debug_query_scalar is intentionally test-only

use std::path::PathBuf;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};

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
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}
