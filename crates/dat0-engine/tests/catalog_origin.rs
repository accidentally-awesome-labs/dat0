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
