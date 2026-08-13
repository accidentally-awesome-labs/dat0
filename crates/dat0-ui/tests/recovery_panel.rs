//! Recovering — and discarding — what a previous session left behind.
//!
//! The destructive half is the point. Discarding an ORPHAN removes the whole
//! scratch directory, because dat0 created every byte of it. Discarding an
//! INTERRUPTED WORKSPACE removes only its `.dat0/` subdirectory: the folder is
//! the user's, and so is everything sitting beside the half-written promotion.
//! That asymmetry is why there are two functions rather than one with a flag.

use std::fs;

use dat0_ui::components::recovery;
use tempfile::tempdir;

#[test]
fn discarding_an_orphan_removes_its_scratch_directory() {
    let state = tempdir().unwrap();
    let scratch_root = state.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();
    let dir = scratch_root.join("session-discard-me");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("session.json"), r#"{"tabs":[],"active_tab":null}"#).unwrap();

    recovery::discard(&dir).unwrap();

    assert!(!dir.exists(), "the orphan directory must be gone");
    assert!(
        scratch_root.exists(),
        "discarding one orphan must not take the scratch root with it"
    );
}

#[test]
fn opening_an_orphan_restores_the_tabs_it_recorded() {
    let state = tempdir().unwrap();
    let dir = state.path().join("session-restore-me");
    fs::create_dir_all(&dir).unwrap();
    // The on-disk shape is `dat0_core::session::SessionState`'s: `table_name` /
    // `source_path`. `RestoredTab` renames them to `table` / `path`, so this
    // also pins that the rename still matches what `Session::persist` writes.
    fs::write(
        dir.join("session.json"),
        r#"{"tabs":[{"source_path":"/tmp/sales.csv","table_name":"sales"}],"active_tab":0}"#,
    )
    .unwrap();

    let restored = recovery::load_for_open(&dir).unwrap();

    assert_eq!(restored.tabs.len(), 1);
    assert_eq!(
        restored.tabs[0].path,
        Some(std::path::PathBuf::from("/tmp/sales.csv"))
    );
    assert_eq!(restored.tabs[0].table, "sales");
}

/// An orphan whose session names no source file is still recoverable: the
/// tables live in the scratch database, not in the paths.
#[test]
fn a_session_with_no_source_paths_still_restores_its_tables() {
    let state = tempdir().unwrap();
    let dir = state.path().join("session-pathless");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("session.json"),
        r#"{"tabs":[{"source_path":null,"table_name":"scratch_t"}],"active_tab":0}"#,
    )
    .unwrap();

    let restored = recovery::load_for_open(&dir).unwrap();

    assert_eq!(restored.tabs.len(), 1);
    assert_eq!(restored.tabs[0].table, "scratch_t");
    assert_eq!(restored.tabs[0].path, None);
}

#[test]
fn opening_an_unreadable_session_fails_rather_than_inventing_tabs() {
    let state = tempdir().unwrap();
    let dir = state.path().join("session-broken");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("session.json"), "{ not json").unwrap();

    assert!(
        recovery::load_for_open(&dir).is_err(),
        "a corrupt session must not silently restore as an empty one"
    );
}
