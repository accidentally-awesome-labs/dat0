//! Export round-trips through `export_query_to_path`.
//!
//! These asserted against `export_table` (bytes-returning) until EN3 deleted it:
//! it had no production consumer — `dat0_format::writer` calls
//! `export_query_to_path` directly (`writer.rs:66-68`) — and it `read_to_end`'d
//! an entire export into a `Vec<u8>`, defeating the point of a streaming COPY.
//! The assertions are unchanged in substance; they now read the file the engine
//! actually writes.

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

/// Export `things` to a temp file of the given extension and read it back.
async fn export_things_bytes(engine: &DuckDBEngine, format: ExportFormat, suffix: &str) -> Vec<u8> {
    let tmp = tempfile::Builder::new()
        .prefix("dat0-export-test-")
        .suffix(suffix)
        .tempfile()
        .unwrap();
    engine
        .export_query_to_path("SELECT * FROM \"things\"", format, tmp.path())
        .await
        .unwrap();
    std::fs::read(tmp.path()).unwrap()
}

#[tokio::test]
async fn export_csv() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = export_things_bytes(&engine, ExportFormat::Csv, ".csv").await;
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("id"));
    assert!(s.contains("name"));
    assert!(s.contains("\n1"));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_json() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = export_things_bytes(&engine, ExportFormat::Json, ".json").await;
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("\"id\""));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_parquet_yields_nonempty_bytes() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = export_things_bytes(&engine, ExportFormat::Parquet, ".parquet").await;
    // Parquet magic: starts with 'PAR1'
    assert!(bytes.starts_with(b"PAR1"));
    assert!(bytes.ends_with(b"PAR1"));
    engine.close().await.unwrap();
}
