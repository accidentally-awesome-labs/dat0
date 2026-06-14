//! P8 T10: advisory 1M-row package export/import timing.
//!
//! A timed integration test (NOT a criterion bench) so it runs in the normal
//! `cargo test` gate — a criterion bench would not, and could be silently
//! forgotten. It times the two hot package paths over a 1M-row table:
//! - EXPORT: `session_to_contents` (catalog walk + per-table count) + `Writer::write`
//!   (parquet materialize of every table into the zip), and
//! - IMPORT: `contents_to_workspace` (parquet → concrete tables in a fresh `.dat0/`).
//!
//! The ceiling is intentionally GENEROUS (advisory only — hard perf gates lock
//! at P10) so it can never hang or flake CI: it asserts the round-trip stays
//! under 30s on the shared/warm target. Run with `-- --nocapture` to see the
//! measured elapsed times.

use std::time::{Duration, Instant};

use dat0_app::package;
use dat0_app::session::Session;
use dat0_engine::QueryEngine;

/// 256 MiB engine budget (matches the headless CLI `DEFAULT_BUDGET`).
const BUDGET: u64 = 256 * 1024 * 1024;

/// Advisory ceiling for the full 1M-row export + import round-trip. Generous on
/// purpose — see the module docs. If a genuinely slow shared target exceeds
/// this, raise the ceiling rather than failing (the test is advisory).
const CEILING: Duration = Duration::from_secs(30);

#[tokio::test]
async fn package_io_1m_rows_under_advisory_ceiling() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state_root = tmp.path().join("state");
    let sess = Session::new(&state_root, BUDGET).await.unwrap();

    // 1M-row base table. `range(N)` is the cheapest way to materialize N rows.
    sess.engine
        .execute("CREATE TABLE big AS SELECT * FROM range(1000000) AS r(id)")
        .await
        .unwrap();

    // EXPORT: map the live session → contents, then write the package (parquet).
    let pkg = tmp.path().join("big.dat0");
    let export_start = Instant::now();
    let contents = package::session_to_contents(&sess).await.unwrap();
    dat0_format::Writer::write(&contents, sess.engine.as_ref(), &pkg)
        .await
        .unwrap();
    let export_elapsed = export_start.elapsed();
    sess.engine.close().await.unwrap();
    drop(sess);

    // IMPORT: parse + materialize into a fresh workspace dir.
    let ws = tmp.path().join("ws");
    let parsed = dat0_format::Reader::open(&pkg).unwrap();
    let import_start = Instant::now();
    package::contents_to_workspace(&parsed, &ws, BUDGET)
        .await
        .unwrap();
    let import_elapsed = import_start.elapsed();

    // Sanity: the 1M rows survived the round-trip.
    let reopened = Session::recover_workspace(ws, BUDGET).await.unwrap();
    let r = reopened
        .engine
        .execute("SELECT count(*) FROM big")
        .await
        .unwrap();
    {
        use duckdb::arrow::array::{Array, Int64Array};
        let n = r.batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0);
        assert_eq!(n, 1_000_000, "1M rows must survive export → import");
    }
    reopened.engine.close().await.unwrap();

    let total = export_elapsed + import_elapsed;
    println!(
        "package_io 1M rows: export={:?} import={:?} total={:?} (ceiling {:?})",
        export_elapsed, import_elapsed, total, CEILING
    );

    assert!(
        total < CEILING,
        "1M-row export+import took {total:?}, exceeding the advisory ceiling {CEILING:?} \
         — investigate a regression, or (if the shared target is genuinely slow) raise \
         the ceiling: this assertion is advisory, hard perf gates lock at P10"
    );
}
