use std::path::PathBuf;

use dat0_engine::{DuckDBEngine, FileFormat, MemoryBudget, QueryEngine, RegisterOpts};

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
async fn register_csv_basic() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let info = engine
        .register_file(&fixture("basic.csv"), RegisterOpts::default())
        .await
        .unwrap();
    assert_eq!(info.columns.len(), 4, "id,name,score,active");
    assert!(info.columns.iter().any(|c| c.name == "id"));
    assert!(info.columns.iter().any(|c| c.name == "score"));

    // Sanity scalar via execute() (T7).
    let v = scalar(
        &engine,
        &format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name),
    )
    .await;
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_csv_edge_cases_quoting_bom() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let info = engine
        .register_file(&fixture("edge_cases.csv"), RegisterOpts::default())
        .await
        .unwrap();
    let v = scalar(
        &engine,
        &format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name),
    )
    .await;
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_tsv_via_explicit_format() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    // Author a small TSV at runtime
    let tsv_path = dir.path().join("simple.tsv");
    std::fs::write(&tsv_path, "id\tname\n1\ta\n2\tb\n").unwrap();

    let opts = RegisterOpts {
        format: Some(FileFormat::Tsv),
        ..RegisterOpts::default()
    };
    let info = engine.register_file(&tsv_path, opts).await.unwrap();
    assert_eq!(info.columns.len(), 2);
    let v = scalar(
        &engine,
        &format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name),
    )
    .await;
    assert_eq!(v, "2");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_file_unknown_extension_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let path = dir.path().join("data.xyz");
    std::fs::write(&path, "id,name\n1,a\n").unwrap();
    let err = engine
        .register_file(&path, RegisterOpts::default())
        .await
        .expect_err("unknown extension must error");
    assert!(matches!(
        err,
        dat0_engine::EngineError::UnsupportedFormat(_)
    ));
    engine.close().await.unwrap();
}
