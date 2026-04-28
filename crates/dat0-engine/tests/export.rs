use dat0_engine::{DerivedOrigin, DuckDBEngine, ExportFormat, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

async fn engine_with_things() -> (DuckDBEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    engine
        .create_table(
            "things",
            "SELECT 1::INTEGER as id, 'a'::VARCHAR as name UNION ALL SELECT 2, 'b'",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();
    // Return both so the caller's scope keeps the tempdir alive alongside the
    // engine. Avoids the `mem::forget` leak of an earlier draft.
    (engine, dir)
}

#[tokio::test]
async fn export_csv() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = engine
        .export_table("things", ExportFormat::Csv)
        .await
        .unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("id"));
    assert!(s.contains("name"));
    assert!(s.contains("\n1"));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_json() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = engine
        .export_table("things", ExportFormat::Json)
        .await
        .unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("\"id\""));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_parquet_yields_nonempty_bytes() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = engine
        .export_table("things", ExportFormat::Parquet)
        .await
        .unwrap();
    // Parquet magic: starts with 'PAR1'
    assert!(bytes.starts_with(b"PAR1"));
    assert!(bytes.ends_with(b"PAR1"));
    engine.close().await.unwrap();
}
