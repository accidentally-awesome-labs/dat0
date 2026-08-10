//! The boot recovery scan and the single banner it emits.
//!
//! Two sources — orphan scratch directories and interrupted workspace
//! promotions — consolidate into ONE warning banner carrying the total count
//! and a `recovery.review` primary action. The per-orphan loop this replaced
//! emitted N near-identical banners; the count is the whole point.
//!
//! Every test here touches the process-global `error_ux::banner::PENDING`
//! queue, so all of them are `#[serial]`: a concurrent drain on one side would
//! empty banners another side is mid-asserting.

use std::fs;

use dat0_core::error_ux::drain_pending;
use dat0_core::recovery_scan::recovery_scan_emit;
use serial_test::serial;
use tempfile::tempdir;

/// Seed `n` orphan scratch dirs, each holding a `session.json`.
///
/// Directory names need not be UUIDs: an orphan is *any* subdir containing a
/// `session.json`, and readable names keep the failure output legible.
fn seed_orphans(scratch_root: &std::path::Path, n: usize) {
    fs::create_dir_all(scratch_root).unwrap();
    for i in 0..n {
        let dir = scratch_root.join(format!("session-{i:02}"));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("session.json"), r#"{"tabs":[],"active_tab":null}"#).unwrap();
    }
}

/// A workspace folder whose `.dat0/` has the database but never got a
/// manifest — an interrupted Save Workspace.
fn seed_incomplete_workspace(root: &std::path::Path) {
    let dat0 = root.join(".dat0");
    fs::create_dir_all(&dat0).unwrap();
    fs::write(dat0.join("workspace.duckdb"), b"db").unwrap();
}

#[test]
#[serial]
fn many_orphans_collapse_into_one_banner_carrying_the_count() {
    let _ = drain_pending();
    let tmp = tempdir().unwrap();
    let scratch_root = tmp.path().join("scratch");
    seed_orphans(&scratch_root, 3);

    let banner = recovery_scan_emit(&scratch_root, &[]).expect("3 orphans must be reported");
    assert!(
        banner.title.contains('3'),
        "the title carries the count: {}",
        banner.title
    );
    assert_eq!(
        banner.primary.as_ref().unwrap().action_id,
        "recovery.review"
    );

    // The same banner is on the global pending queue, so first render shows it.
    let drained = drain_pending();
    assert_eq!(drained.len(), 1, "one consolidated banner for N orphans");
    assert_eq!(drained[0], banner);
}

#[test]
#[serial]
fn orphans_and_interrupted_promotions_are_counted_together() {
    let _ = drain_pending();
    let tmp = tempdir().unwrap();
    let scratch_root = tmp.path().join("scratch");
    seed_orphans(&scratch_root, 2);

    let bad = tmp.path().join("bad");
    seed_incomplete_workspace(&bad);

    let banner = recovery_scan_emit(&scratch_root, &[bad]).expect("there is work to recover");
    // 2 orphans + 1 incomplete = 3, in one line rather than two banners.
    assert!(
        banner.title.contains('3'),
        "consolidated count in title: {}",
        banner.title
    );
    assert_eq!(
        banner.primary.as_ref().unwrap().action_id,
        "recovery.review"
    );

    let drained = drain_pending();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0], banner);
}

#[test]
#[serial]
fn a_recent_workspace_with_a_complete_dat0_is_not_recoverable() {
    let _ = drain_pending();
    let tmp = tempdir().unwrap();
    let scratch_root = tmp.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();

    // Both required files present: a finished promotion, not wreckage.
    let good = tmp.path().join("good");
    let good_dat0 = good.join(".dat0");
    fs::create_dir_all(&good_dat0).unwrap();
    fs::write(good_dat0.join("manifest.json"), "{}").unwrap();
    fs::write(good_dat0.join("workspace.duckdb"), b"db").unwrap();

    // No `.dat0/` at all: a not-yet-promoted recent, not an interrupted one.
    let bare = tmp.path().join("bare");
    fs::create_dir_all(&bare).unwrap();

    assert!(recovery_scan_emit(&scratch_root, &[good, bare]).is_none());
    assert!(drain_pending().is_empty());
}

#[test]
#[serial]
fn nothing_to_recover_emits_nothing() {
    let _ = drain_pending();
    let tmp = tempdir().unwrap();
    let scratch_root = tmp.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();

    assert!(recovery_scan_emit(&scratch_root, &[]).is_none());
    assert!(
        drain_pending().is_empty(),
        "a silent scan must not queue a banner"
    );
}

/// A scratch subdir with no `session.json` holds nothing restorable, so it must
/// not inflate the count the user is asked to act on.
#[test]
#[serial]
fn a_scratch_dir_without_a_session_is_not_counted() {
    let _ = drain_pending();
    let tmp = tempdir().unwrap();
    let scratch_root = tmp.path().join("scratch");
    seed_orphans(&scratch_root, 1);
    fs::create_dir(scratch_root.join("empty")).unwrap();

    let banner = recovery_scan_emit(&scratch_root, &[]).expect("the one real orphan is reported");
    assert!(
        banner.title.contains('1'),
        "only the recoverable dir counts: {}",
        banner.title
    );
    let _ = drain_pending();
}
