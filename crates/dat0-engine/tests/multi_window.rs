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

/// DuckDB's file lock is per process, so it lets a second engine in this
/// process open a database another engine holds, and both would write it. A
/// session recovered moments after its window closed can meet the old engine
/// still alive in a task; the engine refuses the second open instead.
#[tokio::test]
async fn a_database_open_in_this_process_is_not_opened_twice() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("w.duckdb");
    let first = DuckDBEngine::new(db.clone(), budget_512mb()).unwrap();
    first.init().await.unwrap();

    let again = DuckDBEngine::new(db.clone(), budget_512mb());
    assert!(
        matches!(again, Err(dat0_engine::EngineError::AlreadyOpen(_))),
        "a second engine on the same file"
    );
    // By another name for the same file, too.
    let dotted = dir.path().join(".").join("w.duckdb");
    assert!(DuckDBEngine::new(dotted, budget_512mb()).is_err());

    drop(first);
    let reopened = DuckDBEngine::new(db, budget_512mb()).expect("once the first is gone");
    reopened.init().await.unwrap();
}

/// Save Workspace moves a scratch database into a workspace folder. An engine
/// still holding it holds the moved file, so one opening the new name is
/// refused until the old one is gone — then it sees what the old one wrote.
#[cfg(unix)]
#[tokio::test]
async fn a_moved_database_still_counts_as_open() {
    let dir = tempfile::tempdir().unwrap();
    let (before, after) = (
        dir.path().join("scratch.duckdb"),
        dir.path().join("w.duckdb"),
    );
    let first = DuckDBEngine::new(before.clone(), budget_512mb()).unwrap();
    first.init().await.unwrap();
    first
        .create_table("t", "SELECT 7 AS v", DerivedOrigin::Sql("seed".into()))
        .await
        .unwrap();
    first.close().await.unwrap();
    std::fs::rename(&before, &after).unwrap();

    assert!(
        matches!(
            DuckDBEngine::new(after.clone(), budget_512mb()),
            Err(dat0_engine::EngineError::AlreadyOpen(_))
        ),
        "the moved file is still open"
    );
    drop(first);
    let second = DuckDBEngine::new(after, budget_512mb()).expect("once the first is gone");
    second.init().await.unwrap();
    assert_eq!(
        scalar(&second, "SELECT v::VARCHAR FROM t").await,
        "7",
        "and it sees what the first wrote"
    );
}

/// A query's worker holds the connection until its query ends, which can be
/// after the engine is gone: the task that asked was dropped, and a blocking
/// worker cannot be. The file counts as open until the connection closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_query_still_running_keeps_the_file_open() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("w.duckdb");
    let engine = DuckDBEngine::new(db.clone(), budget_512mb()).unwrap();
    engine.init().await.unwrap();
    // Many batches: the worker waits to send the second until the first is
    // taken, holding the connection all the while.
    let mut rows = engine
        .execute_streaming("SELECT i FROM range(1000000) t(i)")
        .await
        .unwrap();
    rows.next().await.expect("a first batch").unwrap();
    drop(engine);

    assert!(
        matches!(
            DuckDBEngine::new(db.clone(), budget_512mb()),
            Err(dat0_engine::EngineError::AlreadyOpen(_))
        ),
        "the worker still has the file open"
    );
    drop(rows);
    // The worker finds nobody listening at its next send, and ends.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let reopened = loop {
        match DuckDBEngine::new(db.clone(), budget_512mb()) {
            Ok(e) => break e,
            Err(dat0_engine::EngineError::AlreadyOpen(_))
                if std::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Err(e) => panic!("the file should come free once the query ends: {e}"),
        }
    };
    reopened.init().await.unwrap();
}
