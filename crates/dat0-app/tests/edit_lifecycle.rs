//! P4b edit lifecycle — grid hidden-key plumbing (T5) and beyond (T6/T8 expand
//! this file).
//!
//! T5 covers the `GridDataSource` row-key surface:
//!   - `__dat0_rowid` is present in the bound table's Arrow schema (the surrogate
//!     reaches the grid because views/tables `SELECT *` it),
//!   - it is hidden from `visible_column_names` (and therefore from the rendered
//!     grid columns), and
//!   - `row_key(screen_row)` resolves a screen row to its surrogate `i64`.
//!
//! The harness builds a *base table* via the real `create_table` CTAS path,
//! which T3 wired to auto-inject `__dat0_rowid` (eager surrogate at create
//! time). This deliberately exercises the present-surrogate path; PD-017 (file
//! imports are VIEWs without the surrogate) is handled separately by the
//! controller and is out of T5 scope.

use std::sync::Arc;
use tempfile::TempDir;

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

/// Build a DuckDB engine with an `orders` *base table* (columns `id`, `amount`)
/// that carries `__dat0_rowid`. `create_table` (CTAS) auto-injects the surrogate
/// eagerly at create time (T3), so the bound table genuinely has the key.
///
/// Returns the engine (Arc), the base table name (suitable for
/// `GridDataSource::new`), and the `TempDir` — the caller MUST keep the
/// `TempDir` bound for the test's duration, since the engine holds an open
/// handle to the on-disk db inside it (mirrors the `view_lifecycle.rs`
/// convention; lets T6/T8 add files to the scratch path after calling).
async fn test_engine_with_orders_rowid() -> (Arc<DuckDBEngine>, String, TempDir) {
    let tmp = TempDir::new().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    // CTAS path → base table → eager `__dat0_rowid` injection (T3).
    engine
        .create_table(
            "orders",
            "SELECT * FROM (VALUES (1, 100), (2, 200), (3, 300)) v(id, amount)",
            DerivedOrigin::Sql(
                "SELECT * FROM (VALUES (1, 100), (2, 200), (3, 300)) v(id, amount)".into(),
            ),
        )
        .await
        .unwrap();

    // Hand `tmp` back so the caller keeps the engine's scratch dir alive for the
    // test's duration (dropping it would remove the dir out from under the open
    // db handle).
    (Arc::new(engine), "orders".to_string(), tmp)
}

#[tokio::test]
async fn data_source_exposes_row_key_and_hides_column() {
    // After ensure_rowid + a view bind, the source yields __dat0_rowid per screen
    // row but does NOT list it among visible columns.
    let (engine, base, _tmp) = test_engine_with_orders_rowid().await;
    let ds = dat0_app::grid::GridDataSource::new(engine, base)
        .await
        .unwrap();

    // The surrogate is in the Arrow schema (SELECT * carries it) ...
    assert!(
        ds.schema
            .fields()
            .iter()
            .any(|f| f.name() == dat0_engine::ROWID_COL),
        "schema must carry the surrogate column"
    );
    // ... but it is hidden from the visible columns the grid paints.
    assert!(
        ds.visible_column_names()
            .iter()
            .all(|c| c != dat0_engine::ROWID_COL),
        "visible columns must not include the surrogate"
    );
    // The user's data columns are still visible.
    assert_eq!(
        ds.visible_column_names(),
        vec!["id".to_string(), "amount".to_string()],
    );

    // `row_key` is a synchronous cache lookup (it mirrors the synchronous
    // `render_td` contract and never triggers a fetch), so it returns `None`
    // until the row's page is loaded — exactly as it would before the paging
    // layer has prefetched.
    assert!(
        ds.row_key(0).is_none(),
        "row_key before any page load must be None (no panic, graceful)"
    );

    // After loading the row's page, the surrogate resolves per screen row.
    ds.page_for(0).await.unwrap();
    assert_eq!(
        ds.row_key(0),
        Some(0),
        "first screen row's surrogate is 0 (gap-free 0..n-1 scan order)"
    );
    assert!(ds.row_key(1).is_some());
    assert!(ds.row_key(2).is_some());
    // Beyond row count → None (no panic).
    assert!(ds.row_key(999).is_none());
}
