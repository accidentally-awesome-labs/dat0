//! `start_view_change` supersedes its own lane, and only its own lane.
//!
//! The old contract said the *caller* had to interrupt before issuing a newer
//! `ViewChange`. None of the four call sites did, so a rapid sequence of
//! filter/sort changes left every stale round-trip running to completion.
//! Superseding now lives inside the function and is scoped to `QueryLane::View`.
//!
//! Determinism: the lane is claimed SYNCHRONOUSLY, before the returned future
//! is polled. These tests run on the default current-thread runtime and never
//! `.await` between issuing two changes, so a spawned task cannot have been
//! polled when the assertion runs. No sleeps, no wall clock.
//!
//! `DuckDBEngine::interrupts_fired()` is the recorder: it counts interrupts
//! that actually reached the connection through the scoped surface.

use std::sync::Arc;

use dat0_core::view::{ViewChange, ViewModel, start_view_change};
use dat0_engine::{
    DuckDBEngine, FilterOp, FilterValue, MemoryBudget, QueryEngine, QueryLane, RegisterOpts,
    Scalar, Transformation,
};
use tempfile::TempDir;

/// A ready engine with a 100-row CSV registered, plus the quoted base-table
/// name.
async fn engine_with_table(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
    let csv = tmp.path().join("t.csv");
    let mut s = String::from("a,b\n");
    for i in 0..100_i64 {
        s.push_str(&format!("{i},x{i}\n"));
    }
    std::fs::write(&csv, s).unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let quoted = format!("\"{}\"", info.name.replace('"', "\"\""));
    (Arc::new(engine), quoted)
}

fn filter_a_gte(n: i64) -> Transformation {
    Transformation::Filter {
        column: "a".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Int(n),
        },
    }
}

#[tokio::test]
async fn a_second_view_change_supersedes_the_first_exactly_once() {
    let tmp = TempDir::new().unwrap();
    let (engine, base) = engine_with_table(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base.clone());

    let first = vm.apply(filter_a_gte(10));
    let second = vm.apply(filter_a_gte(20));
    assert!(!first.is_display_only() && !second.is_display_only());

    assert_eq!(engine.interrupts_fired(), 0);

    tokio::spawn(start_view_change(Arc::clone(&engine), base.clone(), first));
    assert_eq!(
        engine.interrupts_fired(),
        0,
        "the first change has nothing to supersede"
    );

    tokio::spawn(start_view_change(Arc::clone(&engine), base.clone(), second));
    assert_eq!(
        engine.interrupts_fired(),
        1,
        "the second change must supersede the first — exactly one interrupt"
    );
}

#[tokio::test]
async fn a_view_change_never_supersedes_a_console_run() {
    let tmp = TempDir::new().unwrap();
    let (engine, base) = engine_with_table(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base.clone());

    // A console run holds the connection.
    let _console = engine.begin_query(QueryLane::Console);

    let change = vm.apply(filter_a_gte(10));
    tokio::spawn(start_view_change(Arc::clone(&engine), base.clone(), change));

    assert_eq!(
        engine.interrupts_fired(),
        0,
        "a View-lane supersede must never abort a Console run"
    );
}

#[tokio::test]
async fn a_display_only_change_neither_supersedes_nor_claims_the_lane() {
    let tmp = TempDir::new().unwrap();
    let (engine, base) = engine_with_table(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base.clone());

    // A real data change claims the View lane.
    let real = vm.apply(filter_a_gte(10));
    tokio::spawn(start_view_change(Arc::clone(&engine), base.clone(), real));
    assert_eq!(engine.interrupts_fired(), 0);

    // A projection op recompiles to identical SQL — no engine round-trip. It
    // must not abort the filter that is still in flight and still wanted.
    let display_only = vm.apply(Transformation::Rename {
        column: "a".into(),
        to: "A".into(),
    });
    assert!(display_only.is_display_only());
    tokio::spawn(start_view_change(
        Arc::clone(&engine),
        base.clone(),
        display_only,
    ));
    assert_eq!(
        engine.interrupts_fired(),
        0,
        "a display-only change must not supersede an in-flight data change"
    );

    // And it did not steal the slot: the next real change still supersedes the
    // ORIGINAL claim rather than finding an empty lane.
    let next = vm.apply(filter_a_gte(20));
    assert!(!next.is_display_only());
    tokio::spawn(start_view_change(Arc::clone(&engine), base.clone(), next));
    assert_eq!(
        engine.interrupts_fired(),
        1,
        "the still-live View claim must be superseded by the next real change"
    );
}

/// The invariant-violation shape `(None, Some)` bails inside the round-trip.
/// That is NOT an interrupt, so its banner must still be raised — this is what
/// keeps the suppression narrowed to `EngineError::Interrupted` instead of
/// swallowing every view-change failure.
#[tokio::test]
async fn a_non_interrupt_failure_still_banners_and_retires_its_token() {
    let tmp = TempDir::new().unwrap();
    let (engine, base) = engine_with_table(&tmp).await;

    let bad = ViewChange {
        new_active_view: None,
        previous_active_view: None,
        sql: Some("SELECT 1".into()),
    };
    assert!(!bad.is_display_only());
    tokio::spawn(start_view_change(Arc::clone(&engine), base, bad));

    // The claim is live until the task runs; one yield lets it bail and retire.
    tokio::task::yield_now().await;
    assert!(
        !engine.interrupt_lane(QueryLane::View),
        "a failed round-trip must retire its token, leaving the lane empty"
    );

    // `PENDING` is process-global, so assert presence rather than an exact
    // count — a sibling test in this binary may have queued its own banner.
    let banners = dat0_core::error_ux::drain_pending();
    assert!(
        banners
            .iter()
            .any(|b| b.body.contains("invariant violated")),
        "a non-interrupt view-change failure must still raise a banner, got {banners:?}"
    );
}
