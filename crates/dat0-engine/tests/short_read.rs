//! EN3: the counted paging path reconciles its stream against its own count.
//!
//! At duckdb-rs 1.4.4 `Arrow::next` is `Some(RecordBatch::from(&self.stmt?.step()?))`
//! (`duckdb-1.4.4/src/arrow_batch.rs:30-32`) — `step()` returns `Option`, so a
//! mid-stream error terminates the batch loop exactly as end-of-stream does. There
//! is no statement-error accessor to probe (D-030). The one detector available is
//! arithmetic: `run_paged` already computed `COUNT(*)`, so if fewer rows arrive
//! than the count says should, the stream was truncated.
//!
//! ## How the "fake" short read is built
//! We need a query whose `COUNT(*)` and whose windowed `SELECT` legitimately
//! disagree, without patching the driver. A DuckDB SEQUENCE is stateful across
//! statements, so `nextval` in the predicate gives exactly that: the COUNT
//! statement burns the first N values and the window statement sees the next N,
//! which no longer satisfy the predicate. That is precisely the shape of a
//! truncated stream from `run_paged`'s point of view — and the point of the test
//! is that `run_paged` refuses to hand such a result to the grid.

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn count_and_stream_disagreement_is_reported_not_silently_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let e = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    e.init().await.unwrap();

    e.execute("CREATE SEQUENCE short_read_seq START 1")
        .await
        .unwrap();

    // COUNT(*) pass consumes nextval 1..=16 → 15 rows satisfy `< 16`.
    // Window pass consumes 17..=32 → 0 rows satisfy it.
    let sql = "SELECT i FROM range(16) t(i) WHERE nextval('short_read_seq') < 16";
    let err = e
        .execute_paged(sql, 0, 1024)
        .await
        .expect_err("a result shorter than its own count must not be returned as success");

    match err {
        dat0_engine::EngineError::EngineFailed(msg) => {
            assert!(
                msg.contains("ended early"),
                "message must name the truncation, got: {msg}"
            );
        }
        other => panic!("expected EngineError::EngineFailed, got {other:?}"),
    }

    e.close().await.unwrap();
}

/// The reconcile must not fire on the ordinary case, or every page load breaks.
///
/// Covers the two arithmetic edges the check has to get right: a window that ends
/// exactly at EOF (`limit` larger than the rows remaining), and an offset past
/// EOF (`total.saturating_sub(offset)` must floor at zero, not underflow).
#[tokio::test]
async fn honest_windows_pass_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let e = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    e.init().await.unwrap();

    // Full window.
    let pq = e
        .execute_paged("SELECT i FROM range(100) t(i)", 0, 10)
        .await
        .unwrap();
    assert_eq!(pq.total_rows, Some(100));

    // Window straddling EOF: 100 rows, offset 95, limit 10 → 5 rows expected.
    let pq = e
        .execute_paged("SELECT i FROM range(100) t(i)", 95, 10)
        .await
        .unwrap();
    assert_eq!(pq.batches.iter().map(|b| b.num_rows()).sum::<usize>(), 5);

    // Offset entirely past EOF → 0 rows expected, no underflow.
    let pq = e
        .execute_paged("SELECT i FROM range(100) t(i)", 5_000, 10)
        .await
        .unwrap();
    assert_eq!(pq.total_rows, Some(100));
    assert_eq!(pq.batches.iter().map(|b| b.num_rows()).sum::<usize>(), 0);

    e.close().await.unwrap();
}
