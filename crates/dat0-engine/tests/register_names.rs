//! A file's table is named for its stem, and never takes another's.
//!
//! The name used to be the stem, full stop, imported with `CREATE OR REPLACE`:
//! a second `data.csv` from another folder replaced the first one's table, and
//! the first tab went on showing the second file's rows under its own name.

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};

async fn count(engine: &DuckDBEngine, table: &str) -> String {
    let res = engine
        .execute(&format!("SELECT count(*)::VARCHAR FROM \"{table}\""))
        .await
        .expect("count");
    let batch = res.batches.first().expect("a batch");
    batch
        .column(0)
        .as_any()
        .downcast_ref::<duckdb::arrow::array::StringArray>()
        .expect("StringArray")
        .value(0)
        .to_string()
}

#[tokio::test]
async fn files_of_one_stem_are_tables_of_their_own_and_a_file_reread_is_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(
        dir.path().join("t.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let (first, second) = (a.join("data.csv"), b.join("data.csv"));
    std::fs::write(&first, "x\n1\n").unwrap();
    std::fs::write(&second, "x\n2\n3\n").unwrap();

    let one = engine
        .register_file_as_table(&first, RegisterOpts::default())
        .await
        .unwrap();
    let two = engine
        .register_file_as_table(&second, RegisterOpts::default())
        .await
        .unwrap();
    assert_eq!(one.name, "data");
    assert_eq!(
        two.name, "data_2",
        "the second file takes a name of its own"
    );
    assert_eq!(
        count(&engine, "data").await,
        "1",
        "and leaves the first alone"
    );
    assert_eq!(count(&engine, "data_2").await, "2");

    // The first file, changed and read again, replaces its own table.
    std::fs::write(&first, "x\n1\n4\n5\n").unwrap();
    let again = engine
        .register_file_as_table(&first, RegisterOpts::default())
        .await
        .unwrap();
    assert_eq!(again.name, "data");
    assert_eq!(count(&engine, "data").await, "3");
    assert_eq!(count(&engine, "data_2").await, "2");

    // A table made another way keeps its name too.
    engine
        .create_table(
            "sales",
            "SELECT 1 AS v",
            DerivedOrigin::Sql("SELECT 1 AS v".into()),
        )
        .await
        .unwrap();
    let sales = dir.path().join("sales.csv");
    std::fs::write(&sales, "v\n7\n8\n").unwrap();
    let imported = engine
        .register_file_as_table(&sales, RegisterOpts::default())
        .await
        .unwrap();
    assert_eq!(imported.name, "sales_2");
    assert_eq!(count(&engine, "sales").await, "1");
}
