//! Force-quit, then relaunch: the scratch directory is found as an orphan and
//! recovers as a working session, not as a re-import.

use dat0_core::session::{Session, Tab, scan_orphans};
use dat0_engine::QueryEngine;

const BUDGET: u64 = 128 * 1024 * 1024;

#[tokio::test]
async fn a_force_quit_session_recovers_its_tabs_and_its_tables() {
    let tmp = tempfile::TempDir::new().unwrap();

    let scratch_dir = {
        let mut s = Session::new(tmp.path(), BUDGET).await.unwrap();
        let csv = tmp.path().join("survives.csv");
        std::fs::write(&csv, "a\n1\n").unwrap();
        // The app's import path MATERIALIZES a base table carrying
        // `__dat0_rowid`. Use the same variant so this mirrors the real drop
        // path rather than a cheaper one.
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
        // `s` drops: the engine closes and session.json persists, which is what
        // a force quit leaves behind.
    };

    // Relaunch: a new process treats the prior dir as an orphan.
    let orphans = scan_orphans(tmp.path(), &[]).unwrap();
    assert!(
        orphans.contains(&scratch_dir),
        "expected {scratch_dir:?} in orphans {orphans:?}"
    );

    // Recover: the engine re-attaches to the surviving scratch.duckdb and the
    // materialized base table is back in the catalog.
    let recovered = Session::recover(scratch_dir, BUDGET).await.unwrap();
    assert_eq!(recovered.tabs().len(), 1);
    let table_name = recovered.tabs()[0].table_name.clone();
    let cat = recovered.engine.get_tables().await.unwrap();
    assert!(cat.iter().any(|t| t.name == table_name));

    // Restore re-opens the persistent scratch.duckdb rather than re-running the
    // import, so the materialized base table — surrogate rowid and all —
    // survives as-is. Assert the recovered object is STILL a rowid-bearing base
    // table, not a view rebuilt from the source file.
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
