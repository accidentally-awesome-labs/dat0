use std::sync::Arc;
use std::time::Duration;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 512 * 1024 * 1024,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupt_isolates_per_engine() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = Arc::new(DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget()).unwrap());
    let b = Arc::new(DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget()).unwrap());
    a.init().await.unwrap();
    b.init().await.unwrap();

    // Engine A: long query (DuckDB CROSS JOIN of large ranges).
    let a_clone = a.clone();
    let long_query = tokio::spawn(async move {
        a_clone
            .execute("SELECT COUNT(*) FROM range(10000000) t1(i), range(1000) t2(j)")
            .await
    });

    // Engine B: short query, runs concurrently.
    let b_clone = b.clone();
    let short_query = tokio::spawn(async move { b_clone.execute("SELECT 1::INTEGER as v").await });

    // Issue interrupt repeatedly until A returns (or test-level timeout fires).
    // A 100ms sleep then a single interrupt is unreliable on slow CI runners
    // where the spawn_blocking thread may not yet be scheduled.
    let interrupter = {
        let a = a.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                a.interrupt();
            }
        })
    };

    // Cap A's wait at 30 seconds; if interrupt doesn't propagate by then,
    // something is broken — fail the test rather than hang.
    let a_result = tokio::time::timeout(Duration::from_secs(30), long_query)
        .await
        .expect("A's long query exceeded 30s timeout — interrupt did not propagate")
        .unwrap();
    let b_result = short_query.await.unwrap();
    interrupter.abort();

    // A must surface EngineError::Interrupted specifically (not just any Err).
    // T7's translate_duckdb_err normalizes DuckDB's interrupt-error to this variant.
    match a_result {
        Err(dat0_engine::EngineError::Interrupted) => {} // expected
        other => panic!("expected EngineError::Interrupted, got {other:?}"),
    }
    // B must complete cleanly, unaffected.
    assert!(
        b_result.is_ok(),
        "B should complete normally despite A's interrupt"
    );
    let qr = b_result.unwrap();
    assert!(qr.batches.iter().map(|b| b.num_rows()).sum::<usize>() == 1);

    a.close().await.unwrap();
    b.close().await.unwrap();
}
