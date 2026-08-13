//! EN2 — lane-scoped cancellation.
//!
//! DuckDB hands out one interrupt handle per connection, so "cancel my query"
//! is not expressible at the driver level. `begin_query` / `end_query` /
//! `interrupt_scoped` / `interrupt_lane` are the layer above that makes it so.
//! These tests pin the three behaviours the SQL console and the view pipeline
//! depend on: a foreign token never fires, a retired token never fires, and an
//! interrupt aimed at one lane leaves another lane's query running.
//!
//! Setup mirrors `tests/interrupt.rs`: a DuckDB CROSS JOIN large enough that it
//! cannot plausibly finish inside the test, plus a repeated-interrupt loop
//! because a single shot is unreliable before the `spawn_blocking` thread is
//! scheduled on a loaded runner.

use std::sync::Arc;
use std::time::Duration;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, QueryLane};

/// Large enough that completing it inside the test window is impossible, so
/// "the task is still running" is a sound assertion rather than a race.
const LONG_QUERY: &str = "SELECT COUNT(*) FROM range(10000000) t1(i), range(1000) t2(j)";

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 512 * 1024 * 1024,
    }
}

async fn ready_engine(dir: &tempfile::TempDir) -> Arc<DuckDBEngine> {
    let engine = Arc::new(DuckDBEngine::new(dir.path().join("e.duckdb"), budget()).unwrap());
    engine.init().await.unwrap();
    engine
}

/// Fire `f` every 50 ms until `task` returns, capped at 30 s.
async fn drain_until_interrupted(
    engine: &Arc<DuckDBEngine>,
    task: tokio::task::JoinHandle<dat0_engine::Result<dat0_engine::QueryResult>>,
    mut f: impl FnMut(&DuckDBEngine) + Send + 'static,
) {
    let interrupter = {
        let engine = Arc::clone(engine);
        tokio::spawn(async move {
            for _ in 0..600 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                f(&engine);
            }
        })
    };
    let result = tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .expect("long query exceeded 30s — the scoped interrupt did not propagate")
        .unwrap();
    interrupter.abort();
    match result {
        Err(dat0_engine::EngineError::Interrupted) => {}
        other => panic!("expected EngineError::Interrupted, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scoped_interrupt_fires_only_for_the_in_flight_token() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ready_engine(&dir).await;

    // `stale` is minted first and immediately displaced by `live`. Displacement
    // is the ordinary way a token goes stale in production (a second run starts
    // before the first is retired), so this is the realistic shape.
    let stale = engine.begin_query(QueryLane::Console);
    let live = engine.begin_query(QueryLane::Console);
    assert_ne!(stale, live, "tokens must be monotonic, never reused");

    let e = Arc::clone(&engine);
    let long = tokio::spawn(async move { e.execute(LONG_QUERY).await });
    // Give the query time to reach the connection before asserting it survives.
    tokio::time::sleep(Duration::from_millis(500)).await;

    for _ in 0..20 {
        assert!(
            !engine.interrupt_scoped(stale),
            "a stale token must be a silent no-op returning false"
        );
    }
    assert_eq!(
        engine.interrupts_fired(),
        0,
        "no interrupt may reach the connection on behalf of a stale token"
    );
    assert!(
        !long.is_finished(),
        "the query must survive interrupts aimed at another token"
    );

    // The token that actually owns the slot kills it.
    assert!(engine.interrupt_scoped(live), "the live token must fire");
    drain_until_interrupted(&engine, long, move |e| {
        e.interrupt_scoped(live);
    })
    .await;

    // Retired token: silent false, so a late Cmd+. cannot hit the next query.
    engine.end_query(live);
    assert!(
        !engine.interrupt_scoped(live),
        "a retired token must not fire"
    );

    engine.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lane_interrupt_leaves_other_lanes_running() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ready_engine(&dir).await;

    let console = engine.begin_query(QueryLane::Console);
    let e = Arc::clone(&engine);
    let long = tokio::spawn(async move { e.execute(LONG_QUERY).await });
    tokio::time::sleep(Duration::from_millis(500)).await;

    // A superseding view change (or a grid supersede) must not touch the console.
    for _ in 0..20 {
        assert!(!engine.interrupt_lane(QueryLane::View));
        assert!(!engine.interrupt_lane(QueryLane::Grid));
        assert!(!engine.interrupt_lane(QueryLane::Other));
    }
    assert_eq!(
        engine.interrupts_fired(),
        0,
        "a foreign lane must not reach the connection"
    );
    assert!(
        !long.is_finished(),
        "a Console query must survive interrupt_lane(View)"
    );

    // Its own lane does reach it.
    assert!(engine.interrupt_lane(QueryLane::Console));
    drain_until_interrupted(&engine, long, |e| {
        e.interrupt_lane(QueryLane::Console);
    })
    .await;

    engine.end_query(console);
    assert!(
        !engine.interrupt_lane(QueryLane::Console),
        "an empty slot must not fire"
    );

    engine.close().await.unwrap();
}

/// Slot bookkeeping, with no query running — fast and hermetic.
#[tokio::test]
async fn end_query_only_clears_its_own_claim() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ready_engine(&dir).await;

    let first = engine.begin_query(QueryLane::View);
    let second = engine.begin_query(QueryLane::View);

    // The displaced claim must not be able to disarm the live one: if it could,
    // a finishing older round-trip would silently un-cancel the newer one.
    engine.end_query(first);
    assert!(
        engine.interrupt_lane(QueryLane::View),
        "end_query on a displaced token must leave the live claim in place"
    );

    engine.end_query(second);
    assert!(!engine.interrupt_lane(QueryLane::View));
    assert!(!engine.interrupt_scoped(second));

    engine.close().await.unwrap();
}
