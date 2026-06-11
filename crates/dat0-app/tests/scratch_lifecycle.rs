//! Scratch lifecycle: drop CSV, simulate crash, relaunch surfaces recovery.

use dat0_app::error_ux::banner::drain_pending;
use dat0_app::session::{Session, Tab, scan_orphans};
use dat0_app::window::orphan_scan_emit;
use dat0_engine::QueryEngine;
use serial_test::serial;

const BUDGET: u64 = 128 * 1024 * 1024;

// Both tests in this file touch the global `error_ux::banner::PENDING`
// queue (`drain_pending` / `push`). `#[serial]` serialises them so a
// concurrent drain on one side never empties banners the other side
// is mid-asserting.
#[tokio::test]
#[serial]
async fn force_quit_then_relaunch_finds_orphan_and_tab_state() {
    let _ = drain_pending();
    let tmp = tempfile::TempDir::new().unwrap();

    let scratch_dir = {
        let mut s = Session::new(tmp.path(), BUDGET).await.unwrap();
        let csv = tmp.path().join("survives.csv");
        std::fs::write(&csv, "a\n1\n").unwrap();
        // PD-017 (Path A): the app import path materializes a base table that
        // carries `__dat0_rowid`. Use the same materializing variant here so
        // the lifecycle test mirrors the real drop path.
        let info = s
            .engine
            .register_file_as_table(&csv, dat0_engine::RegisterOpts::default())
            .await
            .unwrap();
        s.add_tab(Tab {
            table_name: info.name,
            source_path: Some(csv),
            transform_stack: Vec::new(),
            undo_cursor: 0,
            extra: Default::default(),
        })
        .unwrap();
        s.home.root_dir().to_path_buf()
        // s drops — engine closes; session.json persists; "force quit" simulated.
    };

    // Relaunch: new process treats the prior dir as orphan.
    let orphans = scan_orphans(tmp.path(), &[]).unwrap();
    assert!(
        orphans.contains(&scratch_dir),
        "expected {:?} in orphans {:?}",
        scratch_dir,
        orphans
    );

    // Recover. Engine attaches to the surviving scratch.duckdb; the
    // materialized base table is visible in the catalog.
    let recovered = Session::recover(scratch_dir, BUDGET).await.unwrap();
    assert_eq!(recovered.tabs().len(), 1);
    let table_name = recovered.tabs()[0].table_name.clone();
    let cat = recovered.engine.get_tables().await.unwrap();
    assert!(cat.iter().any(|t| t.name == table_name));

    // PD-017 (Path A): session restore re-opens the persistent scratch.duckdb
    // rather than re-running the import — so the materialized base table (with
    // its `__dat0_rowid` surrogate) survives recovery as-is, no re-import
    // needed. Assert the recovered object is STILL a rowid-bearing base table.
    let cols = recovered
        .engine
        .__test_column_names(&table_name)
        .await
        .unwrap();
    assert!(
        cols.contains(&"__dat0_rowid".to_string()),
        "recovered import must still carry __dat0_rowid: {cols:?}"
    );
    recovered
        .engine
        .__test_execute_batch(&format!(
            "ALTER TABLE \"{table_name}\" ADD COLUMN __probe INTEGER;"
        ))
        .await
        .expect("recovered import must be a base table (ALTER TABLE succeeds)");
}

/// P3b T5: multi-orphan scan consolidates into a single count Banner
/// with a "Review" primary action wired to `recovery.review`. Replaces
/// the per-orphan loop the P3a T15 path emitted (and the T2 banner-
/// shape migration carried forward as `push_warning`).
#[tokio::test]
#[serial]
async fn multi_orphan_emits_count_banner() {
    let _ = drain_pending();
    let tmp = tempfile::TempDir::new().unwrap();
    let scratch_root = tmp.path().join("scratch");
    std::fs::create_dir_all(&scratch_root).unwrap();

    // Two orphan scratch dirs, each with a real (empty) session.json.
    for i in 0..2 {
        let dir = scratch_root.join(format!("orphan-{i}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("session.json"), r#"{"tabs":[],"active_tab":null}"#).unwrap();
    }

    let banners = orphan_scan_emit(&scratch_root);
    assert_eq!(banners.len(), 1, "one consolidated banner for N orphans");
    assert!(
        banners[0].title.contains("2"),
        "title carries the count: {}",
        banners[0].title
    );
    assert_eq!(
        banners[0].primary.as_ref().unwrap().action_id,
        "recovery.review"
    );

    // The same banner should also have been pushed onto the global
    // pending queue (so the boot path picks it up at first-render).
    let drained = drain_pending();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0], banners[0]);
}
