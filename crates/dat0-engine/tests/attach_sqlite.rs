use std::path::PathBuf;

use dat0_engine::extension_bootstrap::__test_install_sqlite_scanner;
use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine};

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

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

#[tokio::test]
async fn attach_sqlite_exposes_tables() {
    __test_install_sqlite_scanner().expect("ext install");

    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let dsn = format!("sqlite:{}", fixture("simple.sqlite").display());
    engine
        .attach(
            &dsn,
            "sq",
            AttachOpts {
                read_only: true,
                schema_filter: None,
                token: None,
            },
        )
        .await
        .unwrap();

    let v = scalar(&engine, "SELECT COUNT(*)::TEXT FROM sq.items").await;
    assert_eq!(v, "3");

    engine.detach("sq").await.unwrap();
    engine.close().await.unwrap();
}

/// ATTACH opens a SQLite file lazily, so a file that cannot be read attached
/// all the same, and every catalog query after it failed on it, this engine's
/// own tables included. Now the attach fails and leaves nothing behind.
#[tokio::test]
async fn a_file_that_cannot_be_read_is_not_left_attached() {
    __test_install_sqlite_scanner().expect("ext install");

    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    engine
        .execute("CREATE TABLE mine AS SELECT 1 AS x")
        .await
        .unwrap();

    let gone = dir.path().join("gone.sqlite");
    let attached = engine
        .attach(
            &format!("sqlite:{}", gone.display()),
            "gone",
            AttachOpts {
                read_only: true,
                schema_filter: None,
                token: None,
            },
        )
        .await;

    assert!(
        attached.is_err(),
        "a file that cannot be read does not attach"
    );
    let tables = engine.get_tables().await.expect("the engine's own tables");
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["mine"]);
    // And the alias is free for the file once it is back.
    std::fs::copy(fixture("simple.sqlite"), &gone).unwrap();
    engine
        .attach(
            &format!("sqlite:{}", gone.display()),
            "gone",
            AttachOpts {
                read_only: true,
                schema_filter: None,
                token: None,
            },
        )
        .await
        .expect("attached once it is back");
    assert_eq!(engine.attached_tables("gone").await.unwrap(), ["items"]);
    engine.close().await.unwrap();
}
