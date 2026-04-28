use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use futures::StreamExt;

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn streaming_yields_all_rows() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let mut stream = engine
        .execute_streaming("SELECT i FROM range(50000) t(i)")
        .await
        .unwrap();
    let mut total = 0_usize;
    while let Some(batch) = stream.next().await {
        let b = batch.unwrap();
        total += b.num_rows();
    }
    assert_eq!(total, 50000);
    engine.close().await.unwrap();
}

#[tokio::test]
async fn streaming_respects_consumer_drop() {
    // Drop the stream before draining; producer should clean up without panic.
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    {
        let mut stream = engine
            .execute_streaming("SELECT i FROM range(1000000) t(i)")
            .await
            .unwrap();
        let _ = stream.next().await; // pull one batch
        // stream drops here
    }
    // Engine still functional after a dropped stream.
    let qr = engine.execute("SELECT 1::INTEGER as v").await.unwrap();
    assert_eq!(qr.batches.iter().map(|b| b.num_rows()).sum::<usize>(), 1);
    engine.close().await.unwrap();
}
