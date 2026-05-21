//! D-012 closure: TableInfo.origin reflects the real source.
//!
//! Plan note: the plan snippet used `engine.catalog().get_tables()` which does
//! not exist on `DuckDBEngine`. Adapted to call `engine.get_tables().await`
//! directly via the `QueryEngine` trait. Filed as PD-009.

use dat0_engine::{
    DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts, TableOrigin,
};

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
