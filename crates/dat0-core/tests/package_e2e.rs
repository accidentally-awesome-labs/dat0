//! P8 T10: end-to-end exercise of all five `.dat0` package verbs in one test.
//!
//! Drives the headless CLI async cores (`export_async` / `unpack_async` /
//! `replay_async` / `inspect_async` / `diff_async`) plus the package/format APIs
//! against a single fixture package, asserting the full export → inspect →
//! unpack → replay → diff lifecycle.
//!
//! CRITICAL (P8 T5 finding): derived-table provenance (`table_origins`) is
//! in-memory ONLY — never persisted — so a COLD CLI export (which reopens the
//! workspace via `recover_workspace`, fresh engine, empty origin map) flattens
//! every table to `Base`, losing replayable lineage. Therefore the fixture
//! package for the lineage/replay assertions is built from a LIVE session
//! (`session_to_contents` + `Writer::write`), NOT a cold `export_async` — this
//! is the only path that captures a genuine `Derived` table with parents.
//! The self-consistent all-`Base` round-trip (export → unpack → re-export → diff
//! is empty) is exercised separately via the CLI `export_async` path at the end.

use std::path::{Path, PathBuf};

use dat0_core::cli::{export_async, inspect_async, replay_async, unpack_async};
use dat0_core::package;
use dat0_core::session::Session;
use dat0_engine::{DerivedOrigin, QueryEngine, RegisterOpts};

const BUDGET: u64 = 128 * 1024 * 1024;

/// Read a single `count(*)`-style scalar (Int64) out of a one-row QueryResult,
/// mirroring the downcast pattern the app + sibling tests use.
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

/// Build a `.dat0` package from a LIVE in-memory session carrying:
/// - `sales` from a CSV (File origin → Base + a `PackageSource`, so replay can
///   rebind it), and
/// - `monthly` as a derived SQL table over `sales` (Derived, with a tracked
///   `parents: ["sales"]` lineage edge).
///
/// Returns `(pkg_path, original_csv_path)`. The package preserves lineage
/// because it is written from the live session BEFORE the workspace is reopened.
async fn build_live_package(tmp: &Path) -> (PathBuf, PathBuf) {
    // Original CSV: 5 rows.
    let csv = tmp.join("sales.csv");
    std::fs::write(&csv, "id,qty\n1,10\n2,20\n3,30\n4,40\n5,50\n").unwrap();

    // Scratch session (headless — no workspace flock needed).
    let state_root = tmp.join("state");
    let sess = Session::new(&state_root, BUDGET).await.unwrap();

    // Import sales.csv → File-origin base table with a PackageSource.
    let info = sess
        .engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .expect("register sales.csv");
    assert_eq!(info.name, "sales", "imported table must be named 'sales'");

    // Derived table whose origin SQL is RECORDED (so it exports as Derived with
    // a real `parents` edge to `sales`).
    let monthly_sql = "SELECT id % 3 AS bucket, SUM(qty) AS total FROM sales GROUP BY 1";
    sess.engine
        .create_table(
            "monthly",
            monthly_sql,
            DerivedOrigin::Sql(monthly_sql.to_string()),
        )
        .await
        .unwrap();

    // Export the LIVE session (lineage preserved) → pkg.dat0.
    let contents = package::session_to_contents(&sess).await.unwrap();
    assert_eq!(
        contents
            .recipe
            .tables
            .iter()
            .find(|t| t.name == "monthly")
            .expect("monthly in recipe")
            .kind,
        dat0_format::TableKind::Derived,
        "monthly must be Derived in the live-session package"
    );

    let pkg = tmp.join("pkg.dat0");
    dat0_format::Writer::write(&contents, sess.engine.as_ref(), &pkg)
        .await
        .unwrap();
    sess.engine.close().await.unwrap();

    (pkg, csv)
}

/// THE P8 T10 end-to-end test: one fixture package, all five verbs.
#[tokio::test]
async fn package_lifecycle_all_five_verbs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (pkg, _csv) = build_live_package(tmp.path()).await;

    // ----------------------------------------------------------------------
    // 1) INSPECT — lists sales + monthly AND a lineage edge monthly -> sales.
    // ----------------------------------------------------------------------
    let json_str = inspect_async(&pkg, true).await.unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&json_str).expect("inspect --json must produce valid JSON");

    let table_names: Vec<&str> = v["tables"]
        .as_array()
        .expect("tables array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        table_names.contains(&"sales"),
        "inspect must list 'sales'; got {table_names:?}"
    );
    assert!(
        table_names.contains(&"monthly"),
        "inspect must list 'monthly'; got {table_names:?}"
    );

    let lineage = v["lineage"].as_array().expect("lineage array");
    let has_edge = lineage
        .iter()
        .any(|e| e["table"].as_str() == Some("monthly") && e["parent"].as_str() == Some("sales"));
    assert!(
        has_edge,
        "inspect lineage must contain monthly->sales edge; got: {lineage:?}"
    );

    // ----------------------------------------------------------------------
    // 2) UNPACK — materialize into a workspace dir; rows survive the reopen.
    // ----------------------------------------------------------------------
    let ws_dir = tmp.path().join("unpacked_ws");
    unpack_async(&pkg, &ws_dir).await.unwrap();

    let reopened = Session::recover_workspace(ws_dir.clone(), BUDGET)
        .await
        .unwrap();
    let r = reopened
        .engine
        .execute("SELECT count(*) FROM sales")
        .await
        .unwrap();
    assert_eq!(
        scalar_count(&r),
        5,
        "sales rows (5) must survive unpack → recover_workspace"
    );
    // `monthly` re-materialized too: 5 ids over (id % 3) → 3 buckets.
    let rm = reopened
        .engine
        .execute("SELECT count(*) FROM monthly")
        .await
        .unwrap();
    assert_eq!(scalar_count(&rm), 3, "monthly rows survive unpack");
    reopened.engine.close().await.unwrap();
    // Drop the Session to release the workspace flock — step 5 re-exports this
    // same `ws_dir` (a cold `export_async` reopens + re-locks the workspace).
    drop(reopened);

    // ----------------------------------------------------------------------
    // 3) REPLAY — rebind `sales` to a NEW CSV with more rows → pkg2.
    // ----------------------------------------------------------------------
    let new_csv = tmp.path().join("sales_v2.csv");
    std::fs::write(
        &new_csv,
        "id,qty\n1,10\n2,20\n3,30\n4,40\n5,50\n6,60\n7,70\n8,80\n",
    )
    .unwrap();

    let pkg2 = tmp.path().join("replayed.dat0");
    // The source's logical_name is the CSV filename ("sales.csv").
    let source_spec = format!("sales.csv={}", new_csv.display());
    let out_path = replay_async(&pkg, &[source_spec], Some(pkg2.clone()))
        .await
        .unwrap();
    assert!(
        out_path.exists(),
        "replayed package must exist at {}",
        out_path.display()
    );
    assert_eq!(out_path, pkg2, "replay honored the explicit -o path");

    // ----------------------------------------------------------------------
    // 4) DIFF — original vs replayed is NON-empty (row-count delta on sales).
    // ----------------------------------------------------------------------
    let orig = dat0_format::Reader::open(&pkg).unwrap();
    let replayed = dat0_format::Reader::open(&pkg2).unwrap();
    let d = dat0_format::diff::diff(&orig, &replayed);
    assert!(
        !d.is_empty(),
        "original vs replayed diff must be NON-empty (the new CSV has more rows): {}",
        d.render_text()
    );
    // The row-count delta is on `sales` (5 → 8).
    let sales_delta = d
        .row_count_deltas
        .iter()
        .find(|(name, _, _)| name == "sales");
    assert!(
        sales_delta.is_some(),
        "expected a row_count_delta for `sales`, got: {:?}",
        d.row_count_deltas
    );
    let (_, old, new) = sales_delta.unwrap();
    assert_eq!((*old, *new), (5, 8), "sales row_count 5 → 8 after replay");

    // The replayed package still carries both tables (recipe intact).
    let replayed_names: Vec<_> = replayed
        .recipe
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(replayed_names.contains(&"sales"), "replayed has sales");
    assert!(replayed_names.contains(&"monthly"), "replayed has monthly");

    // ----------------------------------------------------------------------
    // 5) DIFF (round-trip) — export → unpack → re-export of the UNPACKED
    //    workspace is loss-free at the recipe level (empty diff). This is the
    //    self-consistent all-Base round-trip from T5: a COLD CLI export of a
    //    workspace directory classifies every table identically in both
    //    packages, so the diff is genuinely empty.
    // ----------------------------------------------------------------------
    let rt_pkg1 = tmp.path().join("rt1.dat0");
    export_async(&ws_dir, &rt_pkg1).await.unwrap();

    let rt_unpacked = tmp.path().join("rt_unpacked");
    unpack_async(&rt_pkg1, &rt_unpacked).await.unwrap();

    let rt_pkg2 = tmp.path().join("rt2.dat0");
    export_async(&rt_unpacked, &rt_pkg2).await.unwrap();

    let a = dat0_format::Reader::open(&rt_pkg1).unwrap();
    let b = dat0_format::Reader::open(&rt_pkg2).unwrap();
    let rt_diff = dat0_format::diff::diff(&a, &b);
    assert!(
        rt_diff.is_empty(),
        "export → unpack → re-export must be loss-free at the recipe level; got diff:\n{}",
        rt_diff.render_text()
    );
    // Both tables survived the cold round-trip into both packages.
    for pkg in [&a, &b] {
        let names: Vec<_> = pkg.recipe.tables.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"sales"), "round-trip package has sales");
        assert!(names.contains(&"monthly"), "round-trip package has monthly");
    }
}
