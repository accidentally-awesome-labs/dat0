//! A table's origin, and how its file was read, outlive the engine that
//! recorded them (PD-031, PD-037).
//!
//! An engine opened on an existing database — a recovered session, a
//! workspace, a file Save Workspace moved — knew its tables but not where
//! they came from: `get_tables` reported each as `Derived(Sql(""))`, the
//! engine's "unknown", and a file read with a dialect of its own was read
//! again without it. These make tables in a database, close it, open it
//! again, and read the origins back.

use std::path::Path;

use dat0_engine::{
    DerivedOrigin, DuckDBEngine, FileRead, MemoryBudget, QueryEngine, RegisterOpts, TableOrigin,
};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 128 * 1024 * 1024,
    }
}

async fn open(db: &Path) -> DuckDBEngine {
    let engine = DuckDBEngine::new(db.to_path_buf(), budget()).expect("open the database");
    engine.init().await.expect("init");
    engine
}

async fn close(engine: DuckDBEngine) {
    engine.close().await.expect("close");
}

async fn origin_of(engine: &DuckDBEngine, name: &str) -> TableOrigin {
    engine
        .get_tables()
        .await
        .expect("get_tables")
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no table {name}"))
        .origin
}

/// The engine's "unknown": what it reports for a table it has no origin for.
fn is_unknown(origin: &TableOrigin) -> bool {
    matches!(origin, TableOrigin::Derived(DerivedOrigin::Sql(s)) if s.is_empty())
}

#[tokio::test]
async fn a_files_origin_and_how_it_was_read_outlive_the_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("session.duckdb");
    let csv = tmp.path().join("semi.csv");
    std::fs::write(&csv, "a;b\n1;x\n2;y\n").unwrap();
    let opts = RegisterOpts {
        delimiter: Some(';'),
        has_header: Some(true),
        ..Default::default()
    };

    let engine = open(&db).await;
    let name = engine
        .register_file_as_table(&csv, opts.clone())
        .await
        .expect("register")
        .name;
    assert_eq!(
        engine.file_read(&name),
        Some(FileRead {
            opts: opts.clone(),
            shape: Vec::new(),
        }),
        "a registration records how it read the file"
    );
    let shaped = FileRead {
        opts,
        shape: vec![("a".into(), None), ("b".into(), Some("label".into()))],
    };
    engine
        .set_file_read(&name, shaped.clone())
        .await
        .expect("set_file_read");
    close(engine).await;

    let engine = open(&db).await;
    match origin_of(&engine, &name).await {
        TableOrigin::File(p) => assert_eq!(p, csv),
        other => panic!("expected the file, got {other:?}"),
    }
    assert_eq!(engine.file_read(&name), Some(shaped), "and how it was read");
    close(engine).await;
}

#[tokio::test]
async fn a_derived_tables_derivation_outlives_the_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("session.duckdb");

    let engine = open(&db).await;
    engine
        .create_table(
            "base",
            "SELECT 1 AS x",
            DerivedOrigin::Sql("SELECT 1 AS x".into()),
        )
        .await
        .expect("create base");
    engine
        .create_table(
            "kept",
            "SELECT * FROM base",
            DerivedOrigin::Transform {
                parent: "base".into(),
                ops: Vec::new(),
            },
        )
        .await
        .expect("create kept");
    close(engine).await;

    let engine = open(&db).await;
    match origin_of(&engine, "base").await {
        TableOrigin::Derived(DerivedOrigin::Sql(sql)) => assert_eq!(sql, "SELECT 1 AS x"),
        other => panic!("expected its SQL, got {other:?}"),
    }
    match origin_of(&engine, "kept").await {
        TableOrigin::Derived(DerivedOrigin::Transform { parent, .. }) => {
            assert_eq!(parent, "base")
        }
        other => panic!("expected its parent, got {other:?}"),
    }
    close(engine).await;
}

#[tokio::test]
async fn a_rename_carries_the_origin_and_a_drop_forgets_it() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("session.duckdb");

    let engine = open(&db).await;
    engine
        .create_table(
            "old",
            "SELECT 1 AS x",
            DerivedOrigin::Sql("SELECT 1 AS x".into()),
        )
        .await
        .expect("create old");
    engine
        .rename_table("old", "new", None)
        .await
        .expect("rename");
    engine
        .create_table(
            "gone",
            "SELECT 2 AS y",
            DerivedOrigin::Sql("SELECT 2 AS y".into()),
        )
        .await
        .expect("create gone");
    engine.drop_table("gone", None).await.expect("drop");
    close(engine).await;

    let engine = open(&db).await;
    assert!(
        matches!(origin_of(&engine, "new").await, TableOrigin::Derived(DerivedOrigin::Sql(s)) if s == "SELECT 1 AS x"),
        "the renamed table keeps its derivation"
    );
    // A new table of the dropped one's name inherits nothing.
    engine
        .execute("CREATE TABLE gone AS SELECT 'other' AS z")
        .await
        .expect("remake gone by SQL");
    assert!(is_unknown(&origin_of(&engine, "gone").await));
    close(engine).await;
}

#[tokio::test]
async fn a_table_dropped_by_sql_leaves_nothing_for_the_next_of_its_name() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("session.duckdb");

    let engine = open(&db).await;
    engine
        .create_table(
            "t",
            "SELECT 1 AS x",
            DerivedOrigin::Sql("SELECT 1 AS x".into()),
        )
        .await
        .expect("create t");
    // Dropped by the console, which the engine does not watch: its row stays
    // until the database is opened again.
    engine.execute("DROP TABLE t").await.expect("drop by SQL");
    close(engine).await;

    let engine = open(&db).await;
    engine
        .execute("CREATE TABLE t AS SELECT 'other' AS z")
        .await
        .expect("remake t by SQL");
    assert!(
        is_unknown(&origin_of(&engine, "t").await),
        "the row of the table that went is not the new one's"
    );
    close(engine).await;
}
