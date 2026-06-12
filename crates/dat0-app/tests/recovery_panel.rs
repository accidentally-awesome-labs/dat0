//! Multi-orphan recovery flow: 3 orphans → banner count=3 → Review opens
//! panel → per-row Open spawns new window with restored tabs → Discard
//! deletes the dir + decrements count.
//!
//! NOTE on session.json shape: the on-disk shape is owned by
//! `crate::session::SessionState` (private) which serialises `Vec<Tab>`
//! where `Tab { table_name, source_path }`. `recovery_panel::RestoredTab`
//! uses `serde(rename)` to surface those JSON keys as `path` / `table`
//! in-memory so test assertions read `restored.tabs[0].path` as a
//! `PathBuf` directly (per T5 plan note).

use std::fs;
use tempfile::tempdir;

#[test]
fn three_orphans_surface_count_banner() {
    let _ = dat0_app::error_ux::banner::drain_pending();
    let state = tempdir().unwrap();
    let scratch_root = state.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();

    // Synthesize 3 orphan dirs, each with a session.json (P3a shape:
    // `tabs[].table_name` + `tabs[].source_path`). Names don't need to be
    // UUIDs here because `orphan_scan_emit` counts any subdir containing
    // a `session.json`.
    for i in 0..3 {
        let dir = scratch_root.join(format!("session-{i:02}"));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("session.json"), r#"{"tabs":[],"active_tab":null}"#).unwrap();
    }

    let banners = dat0_app::window::orphan_scan_emit(&scratch_root);
    assert_eq!(banners.len(), 1, "one consolidated banner for N orphans");
    let b = &banners[0];
    assert!(
        b.title.contains("3"),
        "title carries the count: {}",
        b.title
    );
    assert_eq!(b.primary.as_ref().unwrap().action_id, "recovery.review");
}

#[test]
fn discard_removes_orphan_dir() {
    let state = tempdir().unwrap();
    let scratch_root = state.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();
    let dir = scratch_root.join("session-discard-me");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("session.json"), r#"{"tabs":[],"active_tab":null}"#).unwrap();

    dat0_app::recovery_panel::discard(&dir).unwrap();
    assert!(!dir.exists(), "orphan dir should be removed");
}

#[test]
fn open_loads_session_json_and_returns_paths() {
    let state = tempdir().unwrap();
    let scratch_root = state.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();
    let dir = scratch_root.join("session-restore-me");
    fs::create_dir(&dir).unwrap();
    // On-disk shape uses `table_name` / `source_path` (see session.rs:23-29
    // SessionState/Tab). `RestoredTab` uses serde(rename) to surface them
    // as `table` / `path` so the assertion below reads naturally.
    fs::write(
        dir.join("session.json"),
        r#"{"tabs":[{"source_path":"/tmp/sales.csv","table_name":"sales"}],"active_tab":0}"#,
    )
    .unwrap();

    let restored = dat0_app::recovery_panel::load_for_open(&dir).unwrap();
    assert_eq!(restored.tabs.len(), 1);
    assert_eq!(
        restored.tabs[0].path,
        std::path::PathBuf::from("/tmp/sales.csv")
    );
}

#[test]
fn discard_incomplete_removes_dat0() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    let dat0 = root.join(".dat0");
    std::fs::create_dir_all(&dat0).unwrap();
    std::fs::write(dat0.join("workspace.duckdb"), b"db").unwrap();

    dat0_app::recovery_panel::discard_incomplete(&root).unwrap();
    assert!(!dat0.exists(), ".dat0/ should be removed");
    assert!(
        root.exists(),
        "the user's folder itself must NOT be deleted"
    );
}
