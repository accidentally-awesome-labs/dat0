//! P8 T4: the headless CLI export→unpack round-trip is state-equivalent.
//!
//! Builds a real on-disk `.dat0/` workspace by promoting a scratch session (the
//! `workspace_promote.rs` pattern), then drives the CLI cores DIRECTLY:
//! - the async cores `export_async` / `unpack_async` under `#[tokio::test]`
//!   (the primary round-trip assertion), and
//! - the synchronous `cli::run(..)` end-to-end under `#[test]` (proves `run`
//!   builds its own runtime and returns exit code 0 — calling it from inside a
//!   `#[tokio::test]` would panic on the nested runtime).

use dat0_app::cli::{self, PackageCmd};
use dat0_app::session::{Session, Tab};
use dat0_app::workspace::{Home, promote};
use dat0_engine::QueryEngine;

const BUDGET: u64 = 128 * 1024 * 1024;

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

/// Promote a fresh scratch session carrying a `sales` table + tab into a
/// workspace under `target`. Returns once the workspace is on disk and the
/// session has been dropped (so the flock is free for the export reopen).
async fn build_workspace(tmp: &std::path::Path, target: &std::path::Path) {
    let state_root = tmp.join("state");
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

    sess.engine.close().await.unwrap();
    std::fs::create_dir_all(target).unwrap();
    let promoted =
        promote::promote_files(target, &scratch_dir, "2026-06-13T00:00:00Z".into()).unwrap();
    sess.adopt_workspace(promoted.root.clone(), promoted.lock, BUDGET)
        .await
        .unwrap();
    std::fs::remove_dir_all(&promoted.old_scratch_dir).unwrap();
    // Drop the session to release the workspace flock before export reopens it.
    drop(sess);
}

#[tokio::test]
async fn cli_export_then_unpack_round_trips_async() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("proj");
    build_workspace(tmp.path(), &workspace).await;

    // Export the workspace to a package.
    let out = tmp.path().join("p.dat0");
    cli::export_async(&workspace, &out).await.unwrap();
    assert!(out.exists(), "package file must be written");

    // Unpack into a fresh workspace dir.
    let unpacked = tmp.path().join("unpacked");
    cli::unpack_async(&out, &unpacked).await.unwrap();
    assert!(
        Home::dat0_dir_for(&unpacked)
            .join("workspace.duckdb")
            .exists(),
        "unpacked workspace.duckdb must exist"
    );

    // Reopen + assert rows survived (T6 guard: 42, not 0).
    let reopened = Session::recover_workspace(unpacked, BUDGET).await.unwrap();
    let r = reopened
        .engine
        .execute("SELECT count(*) FROM sales")
        .await
        .unwrap();
    assert_eq!(scalar_count(&r), 42, "sales rows survive CLI export→unpack");
    assert_eq!(reopened.tabs().len(), 1, "the sales tab round-trips");
    assert_eq!(reopened.tabs()[0].table_name, "sales");
    reopened.engine.close().await.unwrap();
}

/// End-to-end through the synchronous `cli::run` (which builds its OWN runtime).
/// MUST be a plain `#[test]` — calling `run` inside a `#[tokio::test]` would
/// panic ("Cannot start a runtime from within a runtime").
#[test]
fn cli_run_export_then_unpack_returns_zero() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("proj");
    let out = tmp.path().join("p.dat0");
    let unpacked = tmp.path().join("unpacked");

    // Build the workspace on a scratch runtime, dropped before `run` builds its own.
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(build_workspace(tmp.path(), &workspace));
    }

    let code = cli::run(PackageCmd::Export {
        workspace: workspace.clone(),
        out: out.clone(),
    });
    assert_eq!(code, 0, "export run must exit 0");
    assert!(out.exists());

    let code = cli::run(PackageCmd::Unpack {
        package: out.clone(),
        dir: unpacked.clone(),
    });
    assert_eq!(code, 0, "unpack run must exit 0");

    // Verify the unpacked workspace reopens with the rows (own short-lived runtime).
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let reopened = Session::recover_workspace(unpacked, BUDGET).await.unwrap();
        let r = reopened
            .engine
            .execute("SELECT count(*) FROM sales")
            .await
            .unwrap();
        assert_eq!(scalar_count(&r), 42);
        reopened.engine.close().await.unwrap();
    });
}

/// Unimplemented arms return a non-zero error code (not a panic).
#[test]
fn cli_run_diff_is_not_yet_implemented() {
    let code = cli::run(PackageCmd::Diff {
        a: PathBuf::from("/a.dat0"),
        b: PathBuf::from("/b.dat0"),
        json: false,
    });
    assert_eq!(code, 2, "diff is stubbed until T5");
}

use std::path::PathBuf;
