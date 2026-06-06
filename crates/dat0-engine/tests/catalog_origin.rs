//! D-012 closure: TableInfo.origin reflects the real source.
//!
//! Plan note: the plan snippet used `engine.catalog().get_tables()` which does
//! not exist on `DuckDBEngine`. Adapted to call `engine.get_tables().await`
//! directly via the `QueryEngine` trait. Filed as PD-009.

use std::path::PathBuf;

use dat0_engine::{
    AttachOpts, DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts, TableOrigin,
};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn register_file_origin_is_file() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = tmp.path().join("a.csv");
    std::fs::write(&csv, "a,b\n1,x\n2,y\n").unwrap();

    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    let info = engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .expect("register");

    let tables = engine.get_tables().await.expect("get_tables");
    let entry = tables.iter().find(|t| t.name == info.name).expect("entry");
    match &entry.origin {
        TableOrigin::File(p) => assert_eq!(p, &csv),
        other => panic!("expected File origin, got {:?}", other),
    }
}

#[tokio::test]
// DuckDB Scratch mode: all user tables land in "main"; this test verifies
// information_schema lookup returns "main", not that it hardcodes "main".
// Multi-schema resolution coverage is P4 (workspace mode).
async fn create_table_returns_real_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    let info = engine
        .create_table(
            "t1",
            "SELECT 1 AS x",
            DerivedOrigin::Sql("SELECT 1 AS x".into()),
        )
        .await
        .expect("create_table");
    assert_eq!(info.schema, "main");
    // After create, the table is visible in catalog with the correct schema.
    let tables = engine.get_tables().await.unwrap();
    let entry = tables.iter().find(|t| t.name == "t1").expect("entry");
    assert_eq!(entry.schema, "main");
}

#[tokio::test]
async fn drop_table_removes_origin_entry() {
    // Use create_table (which creates a real TABLE) so drop_table can succeed.
    // register_file for CSV creates a VIEW; this test is about origin-map
    // maintenance, not the origin type.
    let tmp = tempfile::tempdir().unwrap();

    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    let info = engine
        .create_table(
            "t_drop",
            "SELECT 1 AS x",
            DerivedOrigin::Sql("SELECT 1 AS x".into()),
        )
        .await
        .unwrap();
    // origin is recorded
    assert!(
        engine.table_origin(&info.name).is_some(),
        "create_table should record origin"
    );

    engine
        .drop_table(&info.name, None)
        .await
        .expect("drop_table");
    // origin is removed
    assert!(
        engine.table_origin(&info.name).is_none(),
        "drop_table must remove origin entry"
    );
}

#[tokio::test]
async fn rename_table_rekeys_origin_entry() {
    // Use create_table (which creates a real TABLE) so rename_table can succeed.
    // register_file for CSV creates a VIEW; this test is about origin-map
    // maintenance, not the origin type.
    let tmp = tempfile::tempdir().unwrap();

    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    let info = engine
        .create_table(
            "t_rename",
            "SELECT 1 AS x",
            DerivedOrigin::Sql("SELECT 1 AS x".into()),
        )
        .await
        .unwrap();
    let old_name = info.name.clone();
    let new_name = format!("{}_renamed", old_name);

    // origin is recorded under old name
    assert!(
        engine.table_origin(&old_name).is_some(),
        "create_table should record origin under original name"
    );

    engine
        .rename_table(&old_name, &new_name, None)
        .await
        .expect("rename_table");

    // origin moved to new name; old entry gone
    assert!(
        engine.table_origin(&old_name).is_none(),
        "rename_table must remove old origin entry"
    );
    assert!(
        engine.table_origin(&new_name).is_some(),
        "rename_table must insert origin entry under new name"
    );
    // origin value is preserved (Derived::Sql pointing to the same SQL)
    match engine.table_origin(&new_name).unwrap() {
        TableOrigin::Derived(DerivedOrigin::Sql(sql)) => {
            assert_eq!(sql, "SELECT 1 AS x")
        }
        other => panic!("expected Derived(Sql) origin after rename, got {:?}", other),
    }
}

/// P5b T11: `create_table` HONORS its `origin` param and records the distinct
/// `DerivedOrigin` variant (Sql vs Transform). This is the exit-criterion proof
/// that the previously-discarded `Transform { parent, ops }` lineage is now
/// genuinely populated.
#[tokio::test]
async fn create_table_records_both_origins() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    // Seed a real base table `t` (Sql origin) so `SELECT * FROM t` is valid.
    engine
        .create_table(
            "t",
            "SELECT 1 AS a",
            DerivedOrigin::Sql("SELECT 1 AS a".into()),
        )
        .await
        .unwrap();

    // 1) Sql origin (raw statement string).
    engine
        .create_table(
            "d_sql",
            "SELECT 1 AS a",
            DerivedOrigin::Sql("SELECT 1 AS a".into()),
        )
        .await
        .unwrap();

    // 2) Transform origin (parent + empty op stack). The element type is
    //    `dat0_engine::transform::Transformation`; an empty vec needs the type
    //    annotation so inference resolves it.
    let ops: Vec<dat0_engine::transform::Transformation> = vec![];
    engine
        .create_table(
            "d_tf",
            "SELECT * FROM t",
            DerivedOrigin::Transform {
                parent: "t".into(),
                ops,
            },
        )
        .await
        .unwrap();

    let tables = engine.get_tables().await.unwrap();
    let names: Vec<_> = tables.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"d_sql"));
    assert!(names.contains(&"d_tf"));

    // The engine actually RECORDED the distinct origins (the T11 exit criterion).
    match engine.table_origin("d_sql").unwrap() {
        TableOrigin::Derived(DerivedOrigin::Sql(s)) => assert_eq!(s, "SELECT 1 AS a"),
        other => panic!("expected Sql origin, got {other:?}"),
    }
    match engine.table_origin("d_tf").unwrap() {
        TableOrigin::Derived(DerivedOrigin::Transform { parent, ops }) => {
            assert_eq!(parent, "t");
            assert!(ops.is_empty());
        }
        other => panic!("expected Transform origin, got {other:?}"),
    }
}

/// P6a T4 (closes D-012): attaching a database enumerates its tables/views into
/// the origin registry as `TableOrigin::Attached { alias, source }`, surfaced via
/// `get_tables()`; `detach` prunes them.
///
/// Uses the real sqlite_scanner attach mechanism (a deterministic on-disk SQLite
/// fixture `simple.sqlite`, which holds a table `items` with 3 rows) — the same
/// path exercised by tests/attach_sqlite.rs. No network.
#[tokio::test]
async fn attach_records_per_table_attached_origin() {
    dat0_engine::extension_bootstrap::__test_install_sqlite_scanner().expect("ext install");
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("main.duckdb"), budget()).unwrap();
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

    // The attached table is enumerated in get_tables() WITH its columns and
    // carries an Attached origin tagged with the attach alias (= catalog name).
    let tables = engine.get_tables().await.unwrap();
    let items = tables
        .iter()
        .find(|t| t.name == "items")
        .expect("attached table enumerated in get_tables");
    assert!(
        !items.columns.is_empty(),
        "attached table must describe its columns cross-database"
    );
    match &items.origin {
        TableOrigin::Attached { alias, source } => {
            assert_eq!(alias, "sq");
            assert!(source.contains("simple.sqlite"), "source dsn: {source}");
        }
        other => panic!("expected Attached origin, got {other:?}"),
    }

    // detach prunes the attached entries from both the catalog and the origin map.
    engine.detach("sq").await.unwrap();
    let after = engine.get_tables().await.unwrap();
    assert!(
        !after.iter().any(|t| t.name == "items"),
        "detach removes attached entries from get_tables"
    );
    assert!(
        engine.table_origin("items").is_none(),
        "detach removes attached entries from the origin map"
    );

    engine.close().await.unwrap();
}
