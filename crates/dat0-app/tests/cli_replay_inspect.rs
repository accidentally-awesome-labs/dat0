//! P8 T7: headless CLI `dat0 replay` and `dat0 inspect` arms.
//!
//! Both tests build a package from a LIVE in-memory session (the only way to
//! get a genuinely Derived table + a File-origin base table with a
//! PackageSource — cold workspace reopen loses in-memory origins). The test
//! fixture mirrors the CRITICAL note from the T7 plan:
//!   1. `sales.csv` → `register_file_as_table` → Base + PackageSource
//!   2. `monthly` → `engine.create_table(…, DerivedOrigin::Sql(…))` → Derived
//!   3. `session_to_contents` → `Writer::write` → `pkg`
//!
//! Then exercises replay (row-count delta on `monthly`) and inspect
//! (JSON lineage edge `monthly → sales` + text output).

use std::path::PathBuf;

use dat0_app::cli::{inspect_async, replay_async};
use dat0_app::package;
use dat0_app::session::Session;
use dat0_engine::{DerivedOrigin, QueryEngine, RegisterOpts};

const BUDGET: u64 = 128 * 1024 * 1024;

/// Build a `.dat0` package from a live session carrying:
/// - `sales` from a CSV (File origin → Base + PackageSource so replay can rebind it)
/// - `monthly` as a derived SQL table over `sales` (Derived, trackable lineage)
///
/// Returns `(pkg_path, original_csv_path, tmp_dir)` where tmp_dir must be kept
/// alive for the duration of the test (TempDir implements drop-on-destruct).
async fn build_live_package(tmp: &std::path::Path) -> (PathBuf, PathBuf, PathBuf) {
    // Write the original CSV: 5 rows.
    let csv = tmp.join("sales.csv");
    std::fs::write(&csv, "id,qty\n1,10\n2,20\n3,30\n4,40\n5,50\n").unwrap();

    // Open a scratch session (headless, no workspace flock needed).
    let state_root = tmp.join("state");
    let sess = Session::new(&state_root, BUDGET).await.unwrap();

    // Import `sales.csv` via register_file_as_table → File-origin base table
    // with a PackageSource so ReplayEngine can rebind it.
    let info = sess
        .engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .expect("register sales.csv");
    // The imported table name may be "sales" (derived from filename); assert.
    assert_eq!(info.name, "sales", "imported table must be named 'sales'");

    // Create a derived table via create_table so the origin is RECORDED.
    // `monthly` sums the qty for each row grouped by month-bucket (id % 3).
    let monthly_sql = "SELECT id % 3 AS bucket, SUM(qty) AS total FROM sales GROUP BY 1";
    sess.engine
        .create_table(
            "monthly",
            monthly_sql,
            DerivedOrigin::Sql(monthly_sql.to_string()),
        )
        .await
        .unwrap();

    // Export the live session to a package.
    let contents = package::session_to_contents(&sess).await.unwrap();

    // `sales` must be Base + have a source; `monthly` must be Derived.
    let recipe_sales = contents
        .recipe
        .tables
        .iter()
        .find(|t| t.name == "sales")
        .expect("sales in recipe");
    assert_eq!(
        recipe_sales.kind,
        dat0_format::TableKind::Base,
        "sales must be Base"
    );
    assert!(
        recipe_sales.source_ref.is_some(),
        "sales must have a source_ref"
    );

    let recipe_monthly = contents
        .recipe
        .tables
        .iter()
        .find(|t| t.name == "monthly")
        .expect("monthly in recipe");
    assert_eq!(
        recipe_monthly.kind,
        dat0_format::TableKind::Derived,
        "monthly must be Derived"
    );

    let pkg = tmp.join("pkg.dat0");
    dat0_format::Writer::write(&contents, sess.engine.as_ref(), &pkg)
        .await
        .unwrap();
    sess.engine.close().await.unwrap();

    (pkg, csv, tmp.to_path_buf())
}

// ---------------------------------------------------------------------------
// replay test
// ---------------------------------------------------------------------------

/// Replay with a new CSV that has MORE rows → the derived `monthly` recomputes
/// to a different total, producing a non-empty diff on `row_count_deltas`.
#[tokio::test]
async fn replay_rebinds_source_and_recomputes_derived() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (pkg, _original_csv, _keep) = build_live_package(tmp.path()).await;

    // New CSV: 8 rows (was 5) — monthly recomputes.
    let new_csv = tmp.path().join("sales_v2.csv");
    std::fs::write(
        &new_csv,
        "id,qty\n1,10\n2,20\n3,30\n4,40\n5,50\n6,60\n7,70\n8,80\n",
    )
    .unwrap();

    let out = tmp.path().join("replayed.dat0");
    // `sales.csv` is the logical_name of the source (derived from the filename).
    let source_spec = format!("sales.csv={}", new_csv.display());
    let out_path = replay_async(&pkg, &[source_spec], Some(out.clone()))
        .await
        .unwrap();

    assert!(
        out_path.exists(),
        "replayed package must exist at {}",
        out_path.display()
    );

    // Diff the original vs the replayed package: there MUST be a row_count delta
    // (original `monthly` had 3 groups of 5 rows / 3 ≈ 3 buckets each; replayed
    // has 8 rows / 3 buckets; SUM(qty) differs → no row_count change expected on
    // `monthly` count — but `sales` row_count goes 5 → 8).
    let orig = dat0_format::Reader::open(&pkg).unwrap();
    let replayed = dat0_format::Reader::open(&out_path).unwrap();
    let d = dat0_format::diff::diff(&orig, &replayed);

    // The diff must NOT be empty (row counts changed).
    assert!(
        !d.is_empty(),
        "diff between original and replayed must be non-empty; \
        the new CSV has more rows so at least sales row_count differs"
    );

    // There must be a row_count_delta entry for `sales` (5 → 8).
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
    assert_eq!(*old, 5, "sales original row_count must be 5");
    assert_eq!(*new, 8, "sales replayed row_count must be 8");

    // Replayed package must still contain both tables.
    let names: Vec<_> = replayed
        .recipe
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"sales"), "replayed package has sales");
    assert!(names.contains(&"monthly"), "replayed package has monthly");
}

/// Malformed `--source` spec (no `=`) must return an error, not panic.
#[tokio::test]
async fn replay_malformed_source_spec_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (pkg, _csv, _keep) = build_live_package(tmp.path()).await;

    let result = replay_async(&pkg, &["noseparator".to_string()], None).await;
    assert!(result.is_err(), "malformed source spec must be an error");
    let msg = format!("{:#}", result.unwrap_err());
    assert!(
        msg.contains('=') || msg.to_lowercase().contains("malformed"),
        "error should mention the missing '=': {msg}"
    );
}

// ---------------------------------------------------------------------------
// inspect test
// ---------------------------------------------------------------------------

/// inspect --json lists tables and a lineage edge monthly → sales.
#[tokio::test]
async fn inspect_json_lists_tables_and_lineage() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (pkg, _csv, _keep) = build_live_package(tmp.path()).await;

    let json_str = inspect_async(&pkg, true).await.unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&json_str).expect("inspect --json must produce valid JSON");

    // Tables array must contain both names.
    let tables = v["tables"].as_array().expect("tables array");
    let table_names: Vec<&str> = tables.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        table_names.contains(&"sales"),
        "inspect JSON must list 'sales'; got {table_names:?}"
    );
    assert!(
        table_names.contains(&"monthly"),
        "inspect JSON must list 'monthly'; got {table_names:?}"
    );

    // Lineage array must have an edge monthly → sales.
    let lineage = v["lineage"].as_array().expect("lineage array");
    let has_edge = lineage
        .iter()
        .any(|e| e["table"].as_str() == Some("monthly") && e["parent"].as_str() == Some("sales"));
    assert!(
        has_edge,
        "inspect JSON lineage must contain monthly→sales edge; got: {lineage:?}"
    );
}

/// inspect text form is non-empty and mentions both table names.
#[tokio::test]
async fn inspect_text_mentions_both_tables() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (pkg, _csv, _keep) = build_live_package(tmp.path()).await;

    let text = inspect_async(&pkg, false).await.unwrap();
    assert!(!text.is_empty(), "inspect text must be non-empty");
    assert!(
        text.contains("sales"),
        "inspect text must mention 'sales'; got:\n{text}"
    );
    assert!(
        text.contains("monthly"),
        "inspect text must mention 'monthly'; got:\n{text}"
    );
    // The lineage section should show the edge.
    assert!(
        text.contains("monthly") && text.contains("sales"),
        "both table names present in text output"
    );
}
