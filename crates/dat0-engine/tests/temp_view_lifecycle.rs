//! create_or_replace_view + drop_view end-to-end against real DuckDB.

use std::sync::Arc;
use tempfile::TempDir;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};

async fn build_engine_with_csv(tmp: &TempDir, rows: usize) -> Arc<DuckDBEngine> {
    let csv = tmp.path().join("t.csv");
    let mut s = String::from("a,b\n");
    for i in 0..rows {
        s.push_str(&format!("{},x{}\n", i, i));
    }
    std::fs::write(&csv, s).unwrap();

    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    Arc::new(engine)
}

#[tokio::test]
async fn create_view_and_page_through_it() {
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 100).await;
    let table_name = engine.get_tables().await.unwrap()[0].name.clone();
    let view_sql = format!(
        "SELECT * FROM \"{}\" WHERE a >= 50",
        table_name.replace('"', "\"\"")
    );
    engine
        .create_or_replace_view("v_test", &view_sql)
        .await
        .unwrap();

    let paged = engine
        .execute_paged("SELECT * FROM v_test", 0, 100)
        .await
        .unwrap();
    assert_eq!(paged.total_rows, Some(50), "filter should match a >= 50");
}

#[tokio::test]
async fn replace_view_with_new_predicate() {
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 100).await;
    let table_name = engine.get_tables().await.unwrap()[0].name.clone();
    let escape = |s: &str| s.replace('"', "\"\"");
    let tname = escape(&table_name);

    // First predicate: a >= 50 → 50 rows.
    engine
        .create_or_replace_view(
            "v_test",
            &format!("SELECT * FROM \"{}\" WHERE a >= 50", tname),
        )
        .await
        .unwrap();
    assert_eq!(
        engine
            .execute_paged("SELECT * FROM v_test", 0, 100)
            .await
            .unwrap()
            .total_rows,
        Some(50)
    );

    // Replace: a >= 90 → 10 rows.
    engine
        .create_or_replace_view(
            "v_test",
            &format!("SELECT * FROM \"{}\" WHERE a >= 90", tname),
        )
        .await
        .unwrap();
    assert_eq!(
        engine
            .execute_paged("SELECT * FROM v_test", 0, 100)
            .await
            .unwrap()
            .total_rows,
        Some(10)
    );
}

#[tokio::test]
async fn drop_view_then_select_errors() {
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 10).await;
    let table_name = engine.get_tables().await.unwrap()[0].name.clone();
    let tname = table_name.replace('"', "\"\"");
    engine
        .create_or_replace_view("v_test", &format!("SELECT * FROM \"{}\"", tname))
        .await
        .unwrap();
    engine.drop_view("v_test").await.unwrap();
    let res = engine.execute_paged("SELECT * FROM v_test", 0, 10).await;
    assert!(res.is_err(), "select from dropped view must error");
}

#[tokio::test]
async fn drop_nonexistent_view_is_ok() {
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 10).await;
    // No view created — drop must succeed via DROP VIEW IF EXISTS.
    engine.drop_view("nonexistent_view").await.unwrap();
}

#[tokio::test]
async fn create_view_with_special_chars_in_name() {
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 10).await;
    let table_name = engine.get_tables().await.unwrap()[0].name.clone();
    let tname = table_name.replace('"', "\"\"");
    // quote_ident must escape the embedded quote in the view name.
    engine
        .create_or_replace_view("v_weird\"name", &format!("SELECT * FROM \"{}\"", tname))
        .await
        .unwrap();
    let paged = engine
        .execute_paged("SELECT * FROM \"v_weird\"\"name\"", 0, 10)
        .await
        .unwrap();
    assert_eq!(paged.total_rows, Some(10));
}

#[tokio::test]
async fn recreate_view_after_drop() {
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 10).await;
    let table_name = engine.get_tables().await.unwrap()[0].name.clone();
    let tname = table_name.replace('"', "\"\"");
    let sql = format!("SELECT * FROM \"{}\"", tname);

    engine.create_or_replace_view("v_test", &sql).await.unwrap();
    engine.drop_view("v_test").await.unwrap();
    engine.create_or_replace_view("v_test", &sql).await.unwrap();
    // Second create must succeed without an error from a stale name.
    let paged = engine
        .execute_paged("SELECT * FROM v_test", 0, 10)
        .await
        .unwrap();
    assert_eq!(paged.total_rows, Some(10));
}

#[tokio::test]
async fn create_view_errors_when_engine_closed() {
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 10).await;
    let table_name = engine.get_tables().await.unwrap()[0].name.clone();
    let tname = table_name.replace('"', "\"\"");
    engine.close().await.unwrap();
    let res = engine
        .create_or_replace_view("v_test", &format!("SELECT * FROM \"{}\"", tname))
        .await;
    assert!(res.is_err(), "create after close must error");
}

#[tokio::test]
async fn get_tables_excludes_temp_views_created_via_create_or_replace_view() {
    // Regression for the T4 review concern: DuckDB's information_schema.tables
    // lists CREATE OR REPLACE TEMP VIEW objects alongside BASE TABLEs and
    // file-registered VIEWs — they are indistinguishable by table_type or
    // table_schema alone. After T13 starts calling create_or_replace_view on
    // every chain mutation, the sidebar (which calls get_tables()) would have
    // shown phantom entries for every active tab.
    //
    // Fix: catalog::get_tables now queries duckdb_views() with NOT temporary
    // instead of information_schema.tables. duckdb_views().temporary is a
    // bool column: false for file-registered views, true for TEMP VIEWs.
    let tmp = TempDir::new().unwrap();
    let engine = build_engine_with_csv(&tmp, 5).await;
    let file_table = engine.get_tables().await.unwrap()[0].name.clone();

    engine
        .create_or_replace_view(
            "v_phantom",
            &format!("SELECT * FROM \"{}\"", file_table.replace('"', "\"\"")),
        )
        .await
        .unwrap();

    let names: Vec<String> = engine
        .get_tables()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(
        names.iter().all(|n| n != "v_phantom"),
        "temp view v_phantom leaked into get_tables(): {names:?}"
    );
    assert!(
        names.iter().any(|n| n == &file_table),
        "file-registered view {file_table} disappeared from get_tables(): {names:?}"
    );
}
