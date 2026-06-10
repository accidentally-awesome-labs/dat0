//! P7a T13: opening the same workspace twice is blocked by the flock.
use dat0_app::session::Session;
use dat0_app::workspace::{Home, manifest};
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

const BUDGET: u64 = 128 * 1024 * 1024;

async fn seed_workspace(root: &std::path::Path) {
    let dat0 = Home::dat0_dir_for(root);
    std::fs::create_dir_all(dat0.join("lineage")).unwrap();
    let eng = DuckDBEngine::new(
        dat0.join("workspace.duckdb"),
        MemoryBudget { bytes: BUDGET },
    )
    .unwrap();
    eng.init().await.unwrap();
    eng.close().await.unwrap();
    drop(eng); // release the file handle (close() only flags; drop releases) before recover reopens it
    std::fs::write(dat0.join("session.json"), r#"{"schema_version":8}"#).unwrap();
    manifest::write(
        &dat0.join("manifest.json"),
        &manifest::Manifest::new("t".into()),
    )
    .unwrap();
}

#[tokio::test]
async fn second_open_of_same_workspace_is_blocked() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("proj");
    seed_workspace(&root).await;

    let first = Session::recover_workspace(root.clone(), BUDGET)
        .await
        .unwrap();
    let second = Session::recover_workspace(root.clone(), BUDGET).await;
    assert!(second.is_err(), "second open must be blocked by the flock");
    drop(first);
    // After the holder drops, reopening succeeds (stale-lock self-heal).
    let third = Session::recover_workspace(root.clone(), BUDGET).await;
    assert!(third.is_ok(), "reopen succeeds once the lock is released");
}
