//! `check_single_query` and `confine_to`: the two limits replay puts on SQL
//! that arrived inside a package rather than from the person running dat0
//! (see `src/confine.rs`).

use std::path::Path;

use dat0_engine::{DuckDBEngine, EngineError, ExportFormat, MemoryBudget, QueryEngine};

async fn engine(dir: &Path) -> DuckDBEngine {
    let e = DuckDBEngine::new(
        dir.join("c.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    e.init().await.unwrap();
    e
}

#[tokio::test]
async fn a_single_query_of_any_shape_passes() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path()).await;
    for sql in [
        "SELECT 1",
        "WITH t AS (SELECT 1 AS a) SELECT a FROM t",
        "SELECT 1 UNION ALL SELECT 2",
        "VALUES (1), (2)",
        "FROM range(3)",
        "SELECT 'it''s' AS s",
        "SELECT 1;",
    ] {
        assert!(
            e.check_single_query(sql).await.is_ok(),
            "{sql:?} is one query"
        );
    }
    e.close().await.unwrap();
}

#[tokio::test]
async fn anything_else_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path()).await;
    for sql in [
        "SELECT 1; SELECT 2",
        "COPY (SELECT 1) TO 'x.csv'",
        "ATTACH 'x.db'",
        "PRAGMA version",
        "CREATE TABLE t AS SELECT 1",
        "SET threads = 1",
        "INSTALL httpfs",
        "",
        "-- a comment and nothing else",
        "SELECT FROM WHERE",
    ] {
        assert!(
            matches!(
                e.check_single_query(sql).await,
                Err(EngineError::NotASingleQuery(_))
            ),
            "{sql:?} must be refused"
        );
    }
    e.close().await.unwrap();
}

#[tokio::test]
async fn a_confined_engine_reaches_nothing_outside_its_directory() {
    let work = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("e.csv"), "a\n1\n").unwrap();
    std::fs::write(work.path().join("w.csv"), "a\n1\n").unwrap();

    let e = engine(work.path()).await;
    e.execute("CREATE TABLE t AS SELECT 1 AS a").await.unwrap();
    e.confine_to(work.path()).await.unwrap();

    let read = |p: &Path| format!("SELECT * FROM read_csv('{}')", p.display());
    assert!(
        e.execute(&read(&elsewhere.path().join("e.csv")))
            .await
            .is_err(),
        "reading outside the directory"
    );
    assert!(
        e.execute(&read(&work.path().join("w.csv"))).await.is_ok(),
        "reading inside it"
    );

    let out = elsewhere.path().join("x.parquet");
    assert!(
        e.export_query_to_path("SELECT * FROM t", ExportFormat::Parquet, &out)
            .await
            .is_err(),
        "writing outside the directory"
    );
    assert!(!out.exists());
    assert!(
        e.export_query_to_path(
            "SELECT * FROM t",
            ExportFormat::Parquet,
            &work.path().join("x.parquet")
        )
        .await
        .is_ok(),
        "writing inside it"
    );

    assert!(
        e.execute("SET enable_external_access = true")
            .await
            .is_err(),
        "nothing the engine runs afterwards can lift the confinement"
    );
    assert!(
        e.execute("SELECT a FROM t").await.is_ok(),
        "tables already loaded stay queryable"
    );
    e.close().await.unwrap();
}
