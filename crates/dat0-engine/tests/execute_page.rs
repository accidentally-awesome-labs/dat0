//! `execute_page` — the count-free paging form added by EN1.
//!
//! What distinguishes it from `execute_paged` is the absence of
//! `SELECT COUNT(*) FROM (<sql>) sub` before each window: the grid holds a count
//! from bind time, so re-counting made every scroll page an O(N) scan. These
//! tests pin the three properties callers depend on — no total, graceful EOF, and
//! a correctly-discriminated interrupt.

use std::sync::Arc;
use std::time::Duration;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

async fn engine(dir: &tempfile::TempDir) -> DuckDBEngine {
    let e = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    e.init().await.unwrap();
    e
}

#[tokio::test]
async fn execute_page_returns_window_without_a_total() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir).await;
    let pq = e
        .execute_page("SELECT i FROM range(100) t(i)", 10, 5)
        .await
        .unwrap();

    assert_eq!(
        pq.total_rows, None,
        "execute_page must not report a total — reporting one would mean it paid for a COUNT(*)"
    );
    assert_eq!(pq.offset, 10);
    let sum: usize = pq.batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(sum, 5);
    e.close().await.unwrap();
}

#[tokio::test]
async fn execute_page_past_eof_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir).await;
    // The grid asks for pages past EOF on any fast scroll; an error there would
    // surface as a red banner instead of a blank row.
    let pq = e
        .execute_page("SELECT i FROM range(10) t(i)", 5_000, 1_024)
        .await
        .expect("a window past EOF is a legitimate request, not a failure");

    assert_eq!(pq.total_rows, None);
    let sum: usize = pq.batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(sum, 0, "no rows exist past EOF");
    e.close().await.unwrap();
}

/// Mirrors `interrupt.rs`'s discrimination assertion for the new path.
///
/// This is the concrete reason EN1 routed both paging forms through
/// `execute::translate_duckdb_err`: `run_paged` used a bare `?`, which converts
/// through `EngineError`'s `#[from] duckdb::Error` impl and yields
/// `DuckDb(_)` — so an interrupted grid page could not be told apart from a
/// genuine query failure, and the SQL console's Cmd+. UX reads exactly that
/// discriminator.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupted_execute_page_is_interrupted_not_duckdb() {
    let dir = tempfile::tempdir().unwrap();
    let e = Arc::new(engine(&dir).await);

    let runner = {
        let e = Arc::clone(&e);
        tokio::spawn(async move {
            e.execute_page(
                "SELECT COUNT(*) FROM range(10000000) t1(i), range(1000) t2(j)",
                0,
                1,
            )
            .await
        })
    };

    // Repeat the interrupt: a single one after a fixed sleep is unreliable when
    // the spawn_blocking thread has not been scheduled yet (interrupt.rs:33-35).
    let interrupter = {
        let e = Arc::clone(&e);
        tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                e.interrupt();
            }
        })
    };

    let result = tokio::time::timeout(Duration::from_secs(30), runner)
        .await
        .expect("interrupt did not propagate within 30s")
        .unwrap();
    interrupter.abort();

    match result {
        Err(dat0_engine::EngineError::Interrupted) => {}
        other => panic!("expected EngineError::Interrupted, got {other:?}"),
    }

    e.close().await.unwrap();
}
