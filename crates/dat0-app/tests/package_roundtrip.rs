//! P8 T4: Session ⇄ PackageContents round-trip is state-equivalent.
//!
//! Exercises BOTH a base table (raw CTAS `sales`, no tracked origin → exported
//! as `TableKind::Base`) and a genuinely Derived-origin table (`monthly` created
//! via `engine.create_table(.., DerivedOrigin::Sql(..))` so `table_origins`
//! records the derived SQL → exported as `TableKind::Derived`).
//!
//! The critical assertion (P7a T6 lesson): after `contents_to_workspace`, the
//! reopened `SELECT count(*) FROM sales` MUST be 42, not 0 — a 0 means the
//! throwaway engine was not dropped before `recover_workspace` reopened the
//! same DuckDB file (silent-empty-db bug).

use dat0_app::package;
use dat0_app::session::queries::SavedQuery;
use dat0_app::session::{Session, Tab};
use dat0_engine::{DerivedOrigin, QueryEngine, TableOrigin};

const BUDGET: u64 = 128 * 1024 * 1024;

/// Read a single `count(*)`-style scalar (Int64) out of a one-row QueryResult,
/// mirroring the downcast pattern the app + `workspace_promote.rs` use.
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

#[tokio::test]
async fn export_then_unpack_is_state_equivalent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state_root = tmp.path().join("state");
    let mut sess = Session::new(&state_root, BUDGET).await.unwrap();

    // Base table via raw CTAS — no tracked origin (becomes TableKind::Base).
    sess.engine
        .execute("CREATE TABLE sales AS SELECT * FROM range(42) AS r(id)")
        .await
        .unwrap();

    // Derived table via the engine's create_table so its origin is RECORDED in
    // `table_origins` (raw CTAS does NOT populate it → would fall back to an
    // empty-SQL derived origin). This makes the Derived arm genuinely exercised.
    let monthly_sql = "SELECT id % 12 AS m, count(*) c FROM sales GROUP BY 1";
    sess.engine
        .create_table(
            "monthly",
            monthly_sql,
            DerivedOrigin::Sql(monthly_sql.into()),
        )
        .await
        .unwrap();

    // Sanity: confirm the engine reports the two origins as expected before we
    // map them (so the test fails loudly if origin-tracking behavior changes).
    let tables = sess.engine.get_tables().await.unwrap();
    let monthly = tables.iter().find(|t| t.name == "monthly").unwrap();
    assert!(
        matches!(&monthly.origin, TableOrigin::Derived(DerivedOrigin::Sql(s)) if !s.is_empty()),
        "monthly must carry a non-empty Derived(Sql) origin, got {:?}",
        monthly.origin
    );

    sess.add_tab(Tab {
        table_name: "sales".into(),
        source_path: None,
        transform_stack: vec![],
        undo_cursor: 0,
        extra: Default::default(),
    })
    .unwrap();

    // A saved query, to assert PackageQuery -> SavedQuery survives the round-trip.
    sess.set_saved_queries(vec![SavedQuery {
        id: uuid::Uuid::now_v7(),
        name: "top".into(),
        sql: "SELECT * FROM sales LIMIT 5".into(),
        saved_at: 0,
    }])
    .unwrap();

    // Export.
    let contents = package::session_to_contents(&sess).await.unwrap();
    // The recipe must carry both tables, classified correctly.
    let recipe_sales = contents
        .recipe
        .tables
        .iter()
        .find(|t| t.name == "sales")
        .expect("sales in recipe");
    assert_eq!(recipe_sales.kind, dat0_format::TableKind::Base);
    let recipe_monthly = contents
        .recipe
        .tables
        .iter()
        .find(|t| t.name == "monthly")
        .expect("monthly in recipe");
    assert_eq!(recipe_monthly.kind, dat0_format::TableKind::Derived);
    assert!(
        matches!(
            &recipe_monthly.derivation,
            Some(dat0_format::Derivation::Sql { sql, .. }) if sql.contains("GROUP BY")
        ),
        "monthly must carry a Derivation::Sql, got {:?}",
        recipe_monthly.derivation
    );
    // One view (the sales tab) and the row_count populated.
    assert_eq!(contents.views.views.len(), 1);
    assert_eq!(contents.views.views[0].table_name, "sales");
    assert_eq!(recipe_sales.row_count, 42);

    let out = tmp.path().join("p.dat0");
    dat0_format::Writer::write(&contents, sess.engine.as_ref(), &out)
        .await
        .unwrap();
    sess.engine.close().await.unwrap();
    drop(sess);

    // Unpack into a fresh workspace dir, reopen, assert rows.
    let ws = tmp.path().join("ws");
    let parsed = dat0_format::Reader::open(&out).unwrap();
    package::contents_to_workspace(&parsed, &ws, BUDGET)
        .await
        .unwrap();

    let reopened = Session::recover_workspace(ws, BUDGET).await.unwrap();

    // THE T6 GUARD: 42, not 0.
    let r = reopened
        .engine
        .execute("SELECT count(*) FROM sales")
        .await
        .unwrap();
    assert_eq!(
        scalar_count(&r),
        42,
        "sales rows must survive export→unpack"
    );

    let rm = reopened
        .engine
        .execute("SELECT count(*) FROM monthly")
        .await
        .unwrap();
    // 42 ids over (id % 12) → 12 distinct buckets.
    assert_eq!(scalar_count(&rm), 12, "monthly rows must survive");

    // The view (tab) must round-trip into the recovered session.
    assert_eq!(reopened.tabs().len(), 1);
    assert_eq!(reopened.tabs()[0].table_name, "sales");

    // The saved query must round-trip (PackageQuery -> SavedQuery).
    assert_eq!(reopened.saved_queries().len(), 1);
    assert_eq!(reopened.saved_queries()[0].name, "top");
    assert_eq!(reopened.saved_queries()[0].sql, "SELECT * FROM sales LIMIT 5");

    reopened.engine.close().await.unwrap();
}
