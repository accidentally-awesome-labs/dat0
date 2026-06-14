//! P8 T9: the read-only Inspect open path registers each package table as a
//! non-mutable `read_parquet` view.
//!
//! Builds a real 2-table package (a base `sales` + a derived `monthly`) by
//! mapping a live `Session` to `PackageContents` and writing it with
//! `Writer::write` (the live-session fixture pattern from `cli_roundtrip.rs`),
//! reopens it via `Reader::open`, then drives `package::inspect::open_readonly`
//! and asserts:
//!   - `SELECT count(*)` over each registered view returns the right counts, and
//!   - a mutating `INSERT INTO sales VALUES (...)` ERRORS (views are non-mutable).

use dat0_app::package;
use dat0_app::session::{Session, Tab};
use dat0_engine::{DerivedOrigin, QueryEngine};

const BUDGET: u64 = 128 * 1024 * 1024;

fn scalar_count(result: &dat0_engine::QueryResult) -> i64 {
    use duckdb::arrow::array::{Array, Int64Array};
    let batch = result.batches.first().expect("one batch");
    batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("count(*) is Int64")
        .value(0)
}

/// Build a live scratch session carrying a base `sales` (42 rows) + a genuinely
/// derived `monthly` (12 rows, `id < 12`), map it to package contents, and write
/// a `.dat0` package at `out`. Drops the session (closing the engine) before
/// returning so the package is fully on disk.
async fn write_two_table_package(state_root: &std::path::Path, out: &std::path::Path) {
    let mut sess = Session::new(state_root, BUDGET).await.unwrap();
    sess.engine
        .execute("CREATE TABLE sales AS SELECT * FROM range(42) AS r(id)")
        .await
        .unwrap();
    sess.engine.ensure_rowid("sales").await.unwrap();
    let derived_sql = "SELECT id FROM sales WHERE id < 12";
    sess.engine
        .create_table(
            "monthly",
            derived_sql,
            DerivedOrigin::Sql(derived_sql.to_string()),
        )
        .await
        .unwrap();
    sess.add_tab(Tab {
        table_name: "sales".into(),
        source_path: None,
        transform_stack: vec![],
        undo_cursor: 0,
        extra: Default::default(),
    })
    .unwrap();

    let contents = package::session_to_contents(&sess).await.unwrap();
    dat0_format::Writer::write(&contents, sess.engine.as_ref(), out)
        .await
        .unwrap();
    sess.engine.close().await.unwrap();
    drop(sess);
}

#[tokio::test]
async fn open_readonly_registers_queryable_non_mutable_views() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state_root = tmp.path().join("state");
    let pkg = tmp.path().join("p.dat0");
    write_two_table_package(&state_root, &pkg).await;

    let parsed = dat0_format::Reader::open(&pkg).unwrap();
    let scratch = tmp.path().join("inspect_scratch");
    std::fs::create_dir_all(&scratch).unwrap();

    let (engine, views) = package::inspect::open_readonly(&parsed, &scratch, BUDGET)
        .await
        .expect("open_readonly");

    // Both tables registered as views (recipe order is sales, monthly).
    assert!(
        views.contains(&"sales".to_string()),
        "sales view registered"
    );
    assert!(
        views.contains(&"monthly".to_string()),
        "monthly view registered"
    );

    // Counts are queryable through the read_parquet views.
    let r = engine
        .execute("SELECT count(*) FROM sales")
        .await
        .expect("count sales");
    assert_eq!(scalar_count(&r), 42, "sales view returns 42 rows");

    let r = engine
        .execute("SELECT count(*) FROM monthly")
        .await
        .expect("count monthly");
    assert_eq!(scalar_count(&r), 12, "monthly view returns 12 rows");

    // A mutating statement against a view must ERROR (views are non-mutable).
    let err = engine.execute("INSERT INTO sales VALUES (999)").await;
    assert!(
        err.is_err(),
        "INSERT into a read_parquet view must error (read-only guarantee)"
    );

    engine.close().await.unwrap();
}
