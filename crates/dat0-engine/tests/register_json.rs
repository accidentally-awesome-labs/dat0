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
async fn register_json_array() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.json"), RegisterOpts::default())
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.jsonl"), RegisterOpts::default())
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_ndjson() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.ndjson"), RegisterOpts::default())
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_json_rejects_type_overrides_p2() {
    // P2: type_overrides for JSON would silently drop columns due to DuckDB
    // read_json's subset semantics on `columns={}`. Reject explicitly.
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let mut opts = RegisterOpts::default();
    opts.type_overrides.insert("id".into(), "BIGINT".into());
    let err = engine
        .register_file(&fixture("simple.json"), opts)
        .await
        .expect_err("must reject type_overrides for JSON in P2");
    assert!(matches!(
        err,
        dat0_engine::EngineError::InvalidOption {
            field: "type_overrides",
            ..
        }
    ));
    engine.close().await.unwrap();
}
