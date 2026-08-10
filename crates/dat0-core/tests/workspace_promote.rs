//! P7a T6: promote a real scratch session → reopen as workspace → lossless.
//!
//! EMPIRICAL FILE-LOCK FINDING (macOS, DuckDB 1.4.x):
//! When promote_files() moves scratch.duckdb → .dat0/workspace.duckdb while
//! the old DuckDBEngine Arc is still alive (only close()-flagged), DuckDB
//! allows opening a SECOND connection to the moved file WITHOUT a lock-conflict
//! error. HOWEVER, the second connection sees the database as empty — the data
//! written by the first connection is buffered in its still-alive WAL/connection
//! state and is NOT visible to the new connection until the first connection
//! drops. This is silent data loss, not a clean error.
//!
//! Therefore the drop-first fix in `adopt_workspace` IS required for data
//! integrity: the old engine Arc must be dropped (releasing its connection)
//! before `build_engine` opens a new connection to the moved file.
use dat0_core::session::{Session, Tab};
use dat0_core::workspace::{Home, promote};
use dat0_engine::QueryEngine;

const BUDGET: u64 = 128 * 1024 * 1024;

#[tokio::test]
async fn promote_then_recover_is_lossless() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state_root = tmp.path().join("state");

    // 1. Scratch session with a materialized table + a tab.
    let mut sess = Session::new(&state_root, BUDGET).await.unwrap();
    sess.engine
        .execute("CREATE TABLE sales AS SELECT * FROM range(42) AS r(id)")
        .await
        .unwrap();
    sess.add_tab(Tab {
        table_name: "sales".into(),
        source_path: None,
        transform_stack: vec![],
        undo_cursor: 0,
        extra: Default::default(),
    })
    .unwrap();
    let scratch_dir = sess.home.root_dir().to_path_buf();

    // 2. Promote: close the engine (flags it closed but does NOT drop the Arc),
    //    move the files, then call adopt_workspace which handles the drop ordering.
    //    The KEY HAZARD: promote_files moves the DB while the engine Arc is still
    //    alive. adopt_workspace must drop the old engine before opening the new
    //    connection on the moved file (see module-level doc comment for why).
    sess.engine.close().await.unwrap();
    let target = tmp.path().join("proj");
    std::fs::create_dir_all(&target).unwrap();
    let promoted =
        promote::promote_files(&target, &scratch_dir, "2026-06-10T00:00:00Z".into()).unwrap();
    sess.adopt_workspace(promoted.root.clone(), promoted.lock, BUDGET)
        .await
        .unwrap();
    std::fs::remove_dir_all(&promoted.old_scratch_dir).unwrap();

    assert!(sess.is_workspace(), "session must be in workspace mode");
    assert!(
        Home::dat0_dir_for(&target)
            .join("workspace.duckdb")
            .exists(),
        "workspace.duckdb must exist at new location"
    );

    // 3. Drop the live session (releases lock), reopen from disk — data intact.
    drop(sess);
    let reopened = Session::recover_workspace(target.clone(), BUDGET)
        .await
        .unwrap();
    assert_eq!(reopened.tabs().len(), 1, "one tab must survive promotion");
    assert_eq!(
        reopened.tabs()[0].table_name,
        "sales",
        "tab must point at the sales table"
    );
    let result = reopened
        .engine
        .execute("SELECT count(*) AS n FROM sales")
        .await
        .unwrap();
    let batch = result.batches.first().unwrap();
    use duckdb::arrow::array::{Array, Int64Array};
    let n = batch
        .column(batch.schema().index_of("n").unwrap())
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap()
        .value(0);
    assert_eq!(n, 42, "all rows survive promotion + reopen");
}
