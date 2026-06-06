//! P6a exit-criteria — the engine-observable foundations of the Catalog +
//! Inspector feature. Deterministic, no fixtures, no network.
//!
//!  1. `profile_table` returns per-column stats + an exact row count (the data
//!     the Inspector renders).
//!  2. `create_table` with a `Transform` origin surfaces through `get_tables`
//!     as `TableOrigin::Derived(DerivedOrigin::Transform { parent })` — the
//!     reverse-lineage basis the app's `dependents_of` consumes to drive the
//!     Inspector's live "Dependents" section.

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, TableOrigin};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 128 * 1024 * 1024,
    }
}

#[tokio::test]
async fn profile_table_yields_column_stats_and_row_count() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(tmp.path().join("e.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    engine
        .create_table(
            "orders",
            "SELECT * FROM (VALUES (10.0,'paid'),(20.0,'open'),(NULL,'paid'),(40.0,'paid')) AS v(amount, status)",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();

    let prof = engine.profile_table("orders", None).await.expect("profile");
    assert_eq!(prof.rows, 4, "exact row count");
    // `create_table` injects an internal `__dat0_rowid` surrogate, so the profile
    // carries it too — assert the user columns are present by name rather than an
    // exact count.
    assert!(prof.columns.len() >= 2, "user columns profiled");

    let amount = prof
        .columns
        .iter()
        .find(|c| c.name == "amount")
        .expect("amount column");
    let n = amount
        .numeric
        .as_ref()
        .expect("numeric stats for a DOUBLE column");
    assert_eq!(n.min, 10.0);
    assert_eq!(n.max, 40.0);
    assert!((amount.null_pct - 25.0).abs() < 0.01, "1 of 4 null → 25%");

    let status = prof
        .columns
        .iter()
        .find(|c| c.name == "status")
        .expect("status column");
    assert!(
        status.numeric.is_none(),
        "a VARCHAR column has no numeric stats"
    );
    assert!(status.approx_distinct >= 2, "paid/open are distinct");

    engine.close().await.unwrap();
}

#[tokio::test]
async fn transform_origin_surfaces_for_dependents() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(tmp.path().join("e.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    // Base table (local), then a Transform-derived child of it.
    engine
        .create_table(
            "base",
            "SELECT * FROM (VALUES (1),(2),(3)) AS v(n)",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();
    engine
        .create_table(
            "base_filtered",
            "SELECT n FROM base WHERE n > 1",
            DerivedOrigin::Transform {
                parent: "base".into(),
                ops: vec![],
            },
        )
        .await
        .unwrap();

    let tables = engine.get_tables().await.unwrap();
    let child = tables
        .iter()
        .find(|t| t.name == "base_filtered")
        .expect("derived table enumerated");

    match &child.origin {
        TableOrigin::Derived(DerivedOrigin::Transform { parent, .. }) => {
            assert_eq!(parent, "base", "reverse-lineage parent recorded");
        }
        other => panic!("expected Derived(Transform), got {other:?}"),
    }

    engine.close().await.unwrap();
}
