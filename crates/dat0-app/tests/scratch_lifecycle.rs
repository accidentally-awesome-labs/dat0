//! Scratch lifecycle: drop CSV, simulate crash, relaunch surfaces recovery.

use dat0_app::error_ux::banner::drain_pending;
use dat0_app::session::{Session, Tab, scan_orphans};
use dat0_engine::QueryEngine;

const BUDGET: u64 = 128 * 1024 * 1024;

#[tokio::test]
async fn force_quit_then_relaunch_finds_orphan_and_tab_state() {
    let _ = drain_pending();
    let tmp = tempfile::TempDir::new().unwrap();

    let scratch_dir = {
        let mut s = Session::new(tmp.path(), BUDGET).await.unwrap();
        let csv = tmp.path().join("survives.csv");
        std::fs::write(&csv, "a\n1\n").unwrap();
        let info = s
            .engine
            .register_file(&csv, dat0_engine::RegisterOpts::default())
            .await
            .unwrap();
        s.add_tab(Tab {
            table_name: info.name,
            source_path: Some(csv),
        })
        .unwrap();
        s.scratch_dir.clone()
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
    // registered table is visible in the catalog.
    let recovered = Session::recover(scratch_dir, BUDGET).await.unwrap();
    assert_eq!(recovered.tabs().len(), 1);
    let cat = recovered.engine.get_tables().await.unwrap();
    assert!(cat.iter().any(|t| t.name == recovered.tabs()[0].table_name));
}
