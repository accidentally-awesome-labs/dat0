//! `__dat0_rowid` surrogate injection + idempotent `ensure_rowid` migration,
//! end-to-end against real DuckDB.
//!
//! Adapted to the real engine API:
//! - The constructor is `DuckDBEngine::new(scratch_path, budget)` (there is no
//!   `in_memory()`); we back it with a TempDir scratch file and `init()`.
//! - There is no `execute_batch` / `query_i64_col` / `column_names` on the
//!   engine; the tiny `#[cfg(test)]` helpers below run raw SQL and read a
//!   BIGINT column / list column names directly off the pooled connection.

use std::sync::Arc;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use tempfile::TempDir;

/// Build a fresh in-scratch engine (real ctor + init).
async fn engine(tmp: &TempDir) -> Arc<DuckDBEngine> {
    let eng = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    eng.init().await.unwrap();
    Arc::new(eng)
}

// --- test-only helpers (read raw SQL off the engine) -----------------------
// These mirror what `execute_batch` / `query_i64_col` / `column_names` would do
// if they existed. We reach the connection through the public test hooks added
// for T3 (`__test_execute_batch`, `__test_query_i64_col`, `__test_column_names`).

async fn execute_batch(eng: &DuckDBEngine, sql: &str) {
    eng.__test_execute_batch(sql).await.unwrap();
}

async fn query_i64_col(eng: &DuckDBEngine, sql: &str) -> Vec<i64> {
    eng.__test_query_i64_col(sql).await.unwrap()
}

async fn column_names(eng: &DuckDBEngine, table: &str) -> Vec<String> {
    eng.__test_column_names(table).await.unwrap()
}

#[tokio::test]
async fn import_injects_deterministic_rowid() {
    let tmp = TempDir::new().unwrap();
    let eng = engine(&tmp).await;
    execute_batch(
        &eng,
        "CREATE TABLE t AS SELECT * FROM (VALUES ('a'),('b'),('c')) v(name)",
    )
    .await;
    eng.ensure_rowid("t").await.unwrap();
    let keys = query_i64_col(&eng, "SELECT __dat0_rowid FROM t ORDER BY name").await;
    assert_eq!(keys, vec![0, 1, 2]); // gap-free, scan-order stable
}

#[tokio::test]
async fn ensure_rowid_is_idempotent_and_renames_collision() {
    let tmp = TempDir::new().unwrap();
    let eng = engine(&tmp).await;
    execute_batch(
        &eng,
        "CREATE TABLE t AS SELECT 'x' AS name, 5 AS __dat0_rowid",
    )
    .await;
    eng.ensure_rowid("t").await.unwrap(); // colliding source col → renamed to __dat0_rowid__src
    eng.ensure_rowid("t").await.unwrap(); // idempotent second call
    let cols = column_names(&eng, "t").await;
    assert!(cols.contains(&"__dat0_rowid".to_string()));
    assert!(cols.contains(&"__dat0_rowid__src".to_string()));
}

#[tokio::test]
async fn injected_rowid_is_marked_and_idempotent_noop() {
    // A second ensure_rowid on a freshly-injected (marked) column must be a
    // no-op: it must NOT rename our own surrogate to __dat0_rowid__src.
    let tmp = TempDir::new().unwrap();
    let eng = engine(&tmp).await;
    execute_batch(
        &eng,
        "CREATE TABLE t AS SELECT * FROM (VALUES ('a'),('b')) v(name)",
    )
    .await;
    eng.ensure_rowid("t").await.unwrap();
    eng.ensure_rowid("t").await.unwrap(); // idempotent — marker present
    let cols = column_names(&eng, "t").await;
    assert!(cols.contains(&"__dat0_rowid".to_string()));
    assert!(
        !cols.contains(&"__dat0_rowid__src".to_string()),
        "idempotent call must not rename our own marked surrogate: {cols:?}"
    );
    let keys = query_i64_col(&eng, "SELECT __dat0_rowid FROM t ORDER BY name").await;
    assert_eq!(keys, vec![0, 1]);
}

#[tokio::test]
async fn rowid_tracks_scan_order_not_value_order() {
    // Values are deliberately non-monotonic so the surrogate must follow rowid
    // (scan order), not the value column.
    let tmp = TempDir::new().unwrap();
    let eng = engine(&tmp).await;
    execute_batch(
        &eng,
        "CREATE TABLE t AS SELECT * FROM (VALUES ('zebra'),('apple'),('mango')) v(name)",
    )
    .await;
    eng.ensure_rowid("t").await.unwrap();
    // ORDER BY name → apple, mango, zebra. Their rowids were 1, 2, 0 → keys 1,2,0.
    let by_name = query_i64_col(&eng, "SELECT __dat0_rowid FROM t ORDER BY name").await;
    assert_eq!(by_name, vec![1, 2, 0]);
    // Gap-free 0..n-1 overall.
    let sorted = query_i64_col(&eng, "SELECT __dat0_rowid FROM t ORDER BY __dat0_rowid").await;
    assert_eq!(sorted, vec![0, 1, 2]);
}
