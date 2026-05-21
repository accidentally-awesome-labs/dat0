use std::sync::Arc;

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use futures::StreamExt;

/// Extract the first column, first row as a String from a one-cell query.
async fn scalar(engine: &DuckDBEngine, sql: &str) -> String {
    let res = engine.execute(sql).await.expect("execute");
    let batch = res.batches.first().expect("at least one batch");
    let col = batch.column(0);
    let arr = col
        .as_any()
        .downcast_ref::<duckdb::arrow::array::StringArray>()
        .expect("StringArray");
    arr.value(0).to_string()
}

fn budget_512mb() -> MemoryBudget {
    MemoryBudget {
        bytes: 512 * 1024 * 1024,
    }
}
fn budget_1gb() -> MemoryBudget {
    MemoryBudget {
        bytes: 1024 * 1024 * 1024,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_engines_no_cross_talk() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = Arc::new(DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget_512mb()).unwrap());
    let b = Arc::new(DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget_1gb()).unwrap());
    a.init().await.unwrap();
    b.init().await.unwrap();

    a.create_table(
        "in_a",
        "SELECT i, 'a' AS tag FROM range(1000) t(i)",
        DerivedOrigin::Sql("seed".into()),
    )
    .await
    .unwrap();
    b.create_table(
        "in_b",
        "SELECT i, 'b' AS tag FROM range(2000) t(i)",
        DerivedOrigin::Sql("seed".into()),
    )
    .await
    .unwrap();

    // Tables in A should not be visible in B.
    let tables_a = a.get_tables().await.unwrap();
    let tables_b = b.get_tables().await.unwrap();
    assert!(tables_a.iter().any(|t| t.name == "in_a"));
    assert!(!tables_a.iter().any(|t| t.name == "in_b"));
    assert!(tables_b.iter().any(|t| t.name == "in_b"));
    assert!(!tables_b.iter().any(|t| t.name == "in_a"));

    // Concurrent execution.
    let (ra, rb) = tokio::join!(
        async {
            let mut s = a.execute_streaming("SELECT i FROM in_a").await.unwrap();
            let mut n = 0_usize;
            while let Some(b) = s.next().await {
                n += b.unwrap().num_rows();
            }
            n
        },
        async {
            let mut s = b.execute_streaming("SELECT i FROM in_b").await.unwrap();
            let mut n = 0_usize;
            while let Some(b) = s.next().await {
                n += b.unwrap().num_rows();
            }
            n
        },
    );
    assert_eq!(ra, 1000);
    assert_eq!(rb, 2000);

    a.close().await.unwrap();
    b.close().await.unwrap();
}

#[tokio::test]
async fn per_engine_memory_budgets_are_independent() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget_512mb()).unwrap();
    let b = DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget_1gb()).unwrap();
    a.init().await.unwrap();
    b.init().await.unwrap();
    let la = scalar(&a, "SELECT current_setting('memory_limit')").await;
    let lb = scalar(&b, "SELECT current_setting('memory_limit')").await;
    assert_ne!(la, lb, "memory_limit should differ per engine");
    a.close().await.unwrap();
    b.close().await.unwrap();
}

#[tokio::test]
async fn same_file_concurrent_register() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let csv = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small/basic.csv");

    let a = DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget_512mb()).unwrap();
    let b = DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget_512mb()).unwrap();
    a.init().await.unwrap();
    b.init().await.unwrap();
    let info_a = a
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let info_b = b
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    assert_eq!(info_a.name, info_b.name);
    let count_a = scalar(
        &a,
        &format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info_a.name),
    )
    .await;
    let count_b = scalar(
        &b,
        &format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info_b.name),
    )
    .await;
    assert_eq!(count_a, count_b);
    assert_eq!(count_a, "3");
    a.close().await.unwrap();
    b.close().await.unwrap();
}
