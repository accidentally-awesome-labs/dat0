//! P8 T0 (S1): Parquet type-fidelity round-trip spike.
//!
//! Guards the P6a `Decimal128`/NULL/temporal gotchas: writes a table with
//! DECIMAL(9,2) / DATE / TIMESTAMP / BIGINT plus NULLs through
//! `export_query_to_path(Parquet)`, re-reads via `read_parquet`, and asserts
//! the round-tripped DuckDB types survive. If DECIMAL widens or DATE promotes
//! to TIMESTAMP, T2's writer must pin types via an explicit `SELECT … CAST`
//! projection — see the spike notes for the observed verdict.

use dat0_engine::types::ExportFormat;
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

/// Extract the first column, first row as a String from a one-cell query.
/// Copied from the house pattern in `register_parquet.rs` rather than adding
/// a public API.
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

#[tokio::test]
async fn parquet_roundtrip_preserves_types() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    // Mixed types incl the gotchas: DECIMAL, NULL, DATE, TIMESTAMP, BIGINT.
    engine
        .execute(
            "CREATE TABLE t AS SELECT * FROM (VALUES \
         (1::BIGINT, 25.50::DECIMAL(9,2), DATE '2026-01-01', TIMESTAMP '2026-01-01 12:00:00'), \
         (2::BIGINT, NULL,                NULL,              NULL)) \
         AS v(id, amt, d, ts)",
        )
        .await
        .unwrap();
    let pq = dir.path().join("t.parquet");
    engine
        .export_query_to_path("SELECT * FROM t", ExportFormat::Parquet, &pq)
        .await
        .unwrap();
    engine
        .execute(&format!(
            "CREATE VIEW rt AS SELECT * FROM read_parquet('{}')",
            pq.display()
        ))
        .await
        .unwrap();
    let cols = engine.describe_table("rt", None).await.unwrap();
    let types: Vec<_> = cols
        .iter()
        .map(|c| (c.name.as_str(), c.data_type.as_str()))
        .collect();
    assert!(types.contains(&("id", "BIGINT")), "got {types:?}");
    assert!(
        types
            .iter()
            .any(|(n, t)| *n == "amt" && t.starts_with("DECIMAL(9,2)")),
        "got {types:?}"
    );
    assert!(types.contains(&("d", "DATE")), "got {types:?}");
    let amt = scalar(&engine, "SELECT amt::TEXT FROM rt WHERE id = 1").await;
    assert_eq!(amt, "25.50", "got {types:?}");
    engine.close().await.unwrap();
}
