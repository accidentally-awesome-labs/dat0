//! Multi-window: two Sessions in one process, engine isolation.

use dat0_core::session::{Session, Tab};
use dat0_engine::QueryEngine;

const BUDGET: u64 = 128 * 1024 * 1024;

#[tokio::test]
async fn two_sessions_isolated_scratch_and_engine() {
    let tmp = tempfile::TempDir::new().unwrap();

    let mut a = Session::new(tmp.path(), BUDGET).await.unwrap();
    let mut b = Session::new(tmp.path(), BUDGET).await.unwrap();
    assert_ne!(a.window_id, b.window_id);
    assert_ne!(a.home.root_dir(), b.home.root_dir());

    let csv_a = tmp.path().join("a.csv");
    std::fs::write(&csv_a, "x\n1\n").unwrap();
    let csv_b = tmp.path().join("b.csv");
    std::fs::write(&csv_b, "x\n2\n").unwrap();

    let info_a = a
        .engine
        .register_file(&csv_a, dat0_engine::RegisterOpts::default())
        .await
        .unwrap();
    let info_b = b
        .engine
        .register_file(&csv_b, dat0_engine::RegisterOpts::default())
        .await
        .unwrap();

    a.add_tab(Tab {
        table_name: info_a.name.clone(),
        source_path: Some(csv_a.clone()),
        transform_stack: Vec::new(),
        undo_cursor: 0,
        extra: Default::default(),
    })
    .unwrap();
    b.add_tab(Tab {
        table_name: info_b.name.clone(),
        source_path: Some(csv_b.clone()),
        transform_stack: Vec::new(),
        undo_cursor: 0,
        extra: Default::default(),
    })
    .unwrap();

    assert_eq!(a.tabs().len(), 1);
    assert_eq!(b.tabs().len(), 1);

    // Engine isolation: each engine sees only its own table.
    let cat_a = a.engine.get_tables().await.unwrap();
    let cat_b = b.engine.get_tables().await.unwrap();
    assert!(cat_a.iter().any(|t| t.name == info_a.name));
    assert!(cat_b.iter().any(|t| t.name == info_b.name));
    // Cross-check: a's table is NOT visible in b's catalog (engines are
    // independent DuckDB instances against distinct scratch.duckdb files).
    if info_a.name != info_b.name {
        assert!(
            !cat_a.iter().any(|t| t.name == info_b.name),
            "engine A must not see engine B's table"
        );
        assert!(
            !cat_b.iter().any(|t| t.name == info_a.name),
            "engine B must not see engine A's table"
        );
    }
}
