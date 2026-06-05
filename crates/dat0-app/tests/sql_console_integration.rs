//! Engine-level round-trip for the SQL console run paths (no GPUI). Validates
//! the VIEW path (result-producing -> create_or_replace_view -> execute_paged)
//! and the EXEC path (DDL/DML -> execute), plus the bad-SQL error contract.
//!
//! The real engine has no `open_in_memory` constructor: it is built via
//! `DuckDBEngine::new(scratch_path, MemoryBudget)` against an on-disk scratch
//! file in a `TempDir`, then `init().await`ed — mirroring every other app/engine
//! integration test (`view_lifecycle.rs`, `projection_e2e.rs`). `execute`,
//! `execute_paged`, and `create_or_replace_view` are `QueryEngine` trait methods,
//! so the trait is imported. Both `QueryResult` and `PagedQueryResult` expose a
//! `batches: Vec<RecordBatch>` field whose elements have `.num_rows()`.
use std::sync::Arc;

use dat0_app::query::statement::{ResultKind, classify};
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use tempfile::TempDir;

/// Build a fresh in-scratch engine (on-disk scratch file under `tmp`), matching
/// the construction every existing engine-backed app test uses.
async fn engine(tmp: &TempDir) -> Arc<DuckDBEngine> {
    let e = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .expect("DuckDBEngine::new");
    e.init().await.expect("init");
    Arc::new(e)
}

#[tokio::test]
async fn view_path_binds_select_result() {
    let tmp = TempDir::new().unwrap();
    let e = engine(&tmp).await;
    e.execute("CREATE TABLE t AS SELECT * FROM range(5) AS r(id)")
        .await
        .unwrap();
    let stmt = "SELECT id FROM t WHERE id >= 2";
    assert_eq!(classify(stmt), ResultKind::Result);
    e.create_or_replace_view("__dat0_qr_test_0", stmt)
        .await
        .unwrap();
    let paged = e
        .execute_paged("SELECT * FROM \"__dat0_qr_test_0\"", 0, 100)
        .await
        .unwrap();
    let rows: usize = paged.batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(rows, 3, "id in {{2,3,4}}");
}

#[tokio::test]
async fn exec_path_runs_ddl() {
    let tmp = TempDir::new().unwrap();
    let e = engine(&tmp).await;
    assert_eq!(classify("CREATE TABLE t2 (a INT)"), ResultKind::Exec);
    e.execute("CREATE TABLE t2 (a INT)").await.unwrap();
    e.execute("INSERT INTO t2 VALUES (1)").await.unwrap();
    let r = e.execute("SELECT count(*) FROM t2").await.unwrap();
    assert_eq!(r.batches.iter().map(|b| b.num_rows()).sum::<usize>(), 1);
}

#[tokio::test]
async fn bad_sql_surfaces_engine_error() {
    let tmp = TempDir::new().unwrap();
    let e = engine(&tmp).await;
    let err = e
        .create_or_replace_view("__dat0_qr_test_1", "SELECT * FROM nonesuch")
        .await;
    assert!(
        err.is_err(),
        "binding a bad query must error (-> inline error strip)"
    );
}

/// Proves `interrupt()` actually aborts an in-flight query, returning the
/// `EngineError::Interrupted` variant that `classify_run_err` maps to
/// `SqlRunOutcome::Cancelled` — i.e. the Cancel drop-guard path resolves a
/// running query to "Cancelled". DuckDB will not finish a 1e8×1e8 cross-join
/// count in 150ms, so the interrupt lands on a genuinely in-flight query.
#[tokio::test]
async fn interrupt_stops_a_long_running_query() {
    let tmp = TempDir::new().unwrap();
    let e = engine(&tmp).await;
    let e2 = Arc::clone(&e);
    // A deliberately slow query (large cross join + aggregate).
    let handle = tokio::spawn(async move {
        e2.execute("SELECT count(*) FROM range(100000000) a, range(100000000) b")
            .await
    });
    // Give it a moment to start, then interrupt.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    e.interrupt();
    let res = handle.await.unwrap();
    assert!(res.is_err(), "interrupted query should return an error");
    match res {
        Err(dat0_engine::EngineError::Interrupted) => {}
        Err(other) => panic!("expected Interrupted, got {other:?}"),
        Ok(_) => panic!("query unexpectedly completed"),
    }
}

/// The persisted SQL-tab shape (`SqlTabState` + `SessionState`) round-trips
/// through serde unchanged — the on-disk contract `Session::set_sql_tabs`
/// writes and `SqlConsole::new` reads back (P5a T10). Exercises the serialized
/// form directly (no GPUI, no disk) so the multi-tab persistence cadence has a
/// fast regression guard.
#[test]
fn session_round_trips_sql_tabs_via_setter() {
    use dat0_app::session::SqlTabState;
    use uuid::Uuid;
    // Exercise the serialized shape directly:
    let state = dat0_app::session::SessionState {
        schema_version: dat0_app::session::SESSION_SCHEMA_VERSION,
        tabs: vec![],
        active_tab: None,
        sql_tabs: vec![SqlTabState {
            id: Uuid::now_v7(),
            title: "Q1".into(),
            sql: "SELECT 1".into(),
        }],
        active_sql_tab: Some(0),
        query_history: vec![],
        saved_queries: vec![],
    };
    let json = serde_json::to_string_pretty(&state).unwrap();
    let back: dat0_app::session::SessionState = serde_json::from_str(&json).unwrap();
    assert_eq!(back.sql_tabs.len(), 1);
    assert_eq!(back.sql_tabs[0].sql, "SELECT 1");
    assert_eq!(back.active_sql_tab, Some(0));
}
