//! PD-017 (Path A): `register_file_as_table` materializes a file import into a
//! DuckDB BASE TABLE carrying the `__dat0_rowid` surrogate, instead of the lazy
//! VIEW that `register_file` emits. The base table is what the P4b edit/delete
//! overlay (`WHERE __dat0_rowid = …`) needs to resolve against.
//!
//! These assert, against a real temp CSV:
//!   - the bound object is a BASE TABLE (visible in duckdb_tables(), not duckdb_views());
//!   - it carries a gap-free 0..n-1 `__dat0_rowid`;
//!   - the user's data columns + row count are preserved (P3b sniffing intact);
//!   - `ALTER TABLE` succeeds on it (a VIEW would reject this).

use std::sync::Arc;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use tempfile::TempDir;

async fn engine(tmp: &TempDir) -> Arc<DuckDBEngine> {
    let eng = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    eng.init().await.unwrap();
    Arc::new(eng)
}

/// Is `name` a BASE TABLE (vs a VIEW) in the `main` schema?
async fn is_base_table(eng: &DuckDBEngine, name: &str) -> bool {
    let n = eng
        .__test_query_i64_col(&format!(
            "SELECT count(*)::BIGINT FROM duckdb_tables() \
             WHERE NOT internal AND schema_name = 'main' AND table_name = '{}'",
            name.replace('\'', "''")
        ))
        .await
        .unwrap();
    n.first().copied().unwrap_or(0) > 0
}

async fn is_view(eng: &DuckDBEngine, name: &str) -> bool {
    let n = eng
        .__test_query_i64_col(&format!(
            "SELECT count(*)::BIGINT FROM duckdb_views() \
             WHERE NOT internal AND schema_name = 'main' AND view_name = '{}'",
            name.replace('\'', "''")
        ))
        .await
        .unwrap();
    n.first().copied().unwrap_or(0) > 0
}

#[tokio::test]
async fn register_file_as_table_yields_base_table_with_rowid() {
    let tmp = TempDir::new().unwrap();
    let eng = engine(&tmp).await;

    // 3-row CSV with two user columns. DuckDB sniffs delimiter + types (P3b).
    let csv = tmp.path().join("orders.csv");
    std::fs::write(&csv, "name,score\nalice,10\nbob,20\ncarol,30\n").unwrap();

    let info = eng
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .unwrap();

    // Bound object is a BASE TABLE, not a VIEW.
    assert!(
        is_base_table(&eng, &info.name).await,
        "import must be a base table (found in duckdb_tables())"
    );
    assert!(
        !is_view(&eng, &info.name).await,
        "import must NOT be a view"
    );

    // Carries the surrogate, gap-free 0..n-1 in scan order.
    let cols = eng.__test_column_names(&info.name).await.unwrap();
    assert!(
        cols.contains(&"__dat0_rowid".to_string()),
        "base table must carry __dat0_rowid: {cols:?}"
    );
    let rowids = eng
        .__test_query_i64_col(&format!(
            "SELECT __dat0_rowid FROM \"{}\" ORDER BY __dat0_rowid",
            info.name
        ))
        .await
        .unwrap();
    assert_eq!(rowids, vec![0, 1, 2], "gap-free 0..n-1 surrogate");

    // Sniffing preserved: user columns + row count intact.
    let names: Vec<&str> = cols.iter().map(|s| s.as_str()).collect();
    assert!(
        names.contains(&"name"),
        "user column 'name' preserved: {cols:?}"
    );
    assert!(
        names.contains(&"score"),
        "user column 'score' preserved: {cols:?}"
    );
    let count = eng
        .__test_query_i64_col(&format!("SELECT count(*)::BIGINT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(count, vec![3], "row count preserved");

    // ALTER TABLE succeeds (a VIEW would reject this) — proves base-table-ness
    // beyond the catalog lookup.
    eng.__test_execute_batch(&format!(
        "ALTER TABLE \"{}\" ADD COLUMN __probe INTEGER;",
        info.name
    ))
    .await
    .expect("ALTER TABLE must succeed on a base table");
}

#[tokio::test]
async fn register_file_as_table_leaves_no_intermediate_view() {
    // A1 materializes via a transient `__dat0_import_tmp_<name>` view; on the
    // happy path neither the transient NOR a view sharing the final table name
    // may remain alongside the base table.
    let tmp = TempDir::new().unwrap();
    let eng = engine(&tmp).await;

    let csv = tmp.path().join("t.csv");
    std::fs::write(&csv, "a,b\n1,x\n2,y\n").unwrap();

    let info = eng
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .unwrap();

    // No view by the table's name should remain.
    assert!(
        !is_view(&eng, &info.name).await,
        "no leftover intermediate view should share the table name"
    );

    // The transient intermediate view itself must be dropped.
    assert!(
        !is_view(&eng, &format!("__dat0_import_tmp_{}", info.name)).await,
        "transient __dat0_import_tmp_<name> view must not survive the happy path"
    );

    // get_tables (sidebar catalog) lists exactly one object for this name.
    let tables = eng.get_tables().await.unwrap();
    let matches = tables.iter().filter(|t| t.name == info.name).count();
    assert_eq!(matches, 1, "exactly one catalog entry for the import");
}

#[tokio::test]
async fn register_file_as_table_ctas_failure_rolls_back_transient_view() {
    // ERROR PATH (regression guard for the autocommit-batch leak): the
    // materialization runs `CREATE OR REPLACE VIEW __dat0_import_tmp_<name>`
    // then `CREATE OR REPLACE TABLE <name> AS …`. duckdb-rs 1.4.4's
    // `execute_batch` is AUTOCOMMIT and does NOT roll back prior statements when
    // a later one fails, so without an explicit transaction + ROLLBACK a failing
    // CTAS would leak the transient view permanently.
    //
    // Deterministic failure trigger: pre-create a VIEW named `orders`, then
    // import a file deriving table name `orders`. `CREATE OR REPLACE TABLE
    // orders …` errors ("Existing object orders is of type View, trying to
    // replace with type Table"), which fires AFTER the transient-view create.
    let tmp = TempDir::new().unwrap();
    let eng = engine(&tmp).await;

    // Pre-create a VIEW that collides with the derived table name.
    eng.__test_execute_batch("CREATE VIEW orders AS SELECT 1 AS x;")
        .await
        .unwrap();

    let csv = tmp.path().join("orders.csv");
    std::fs::write(&csv, "name,score\nalice,10\nbob,20\n").unwrap();

    // (a) The import fails (CTAS cannot replace a view with a table).
    let res = eng
        .register_file_as_table(&csv, RegisterOpts::default())
        .await;
    assert!(
        res.is_err(),
        "import must fail when the derived name is an existing VIEW: {res:?}"
    );

    // (b) The transient intermediate must NOT be leaked — the transaction's
    // explicit ROLLBACK unwound the `CREATE OR REPLACE VIEW`.
    assert!(
        !is_view(&eng, "__dat0_import_tmp_orders").await,
        "transient __dat0_import_tmp_orders view leaked after a failed CTAS"
    );

    // The pre-existing `orders` view is untouched (rollback restored state).
    assert!(
        is_view(&eng, "orders").await,
        "pre-existing 'orders' view must survive the rolled-back import"
    );
}
