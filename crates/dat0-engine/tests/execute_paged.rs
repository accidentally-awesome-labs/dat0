use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn execute_paged_returns_window() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let pq = engine
        .execute_paged("SELECT i FROM range(100) t(i)", 10, 5)
        .await
        .unwrap();
    assert_eq!(pq.total_rows, 100);
    assert_eq!(pq.offset, 10);
    let sum: usize = pq.batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(sum, 5);
    engine.close().await.unwrap();
}
