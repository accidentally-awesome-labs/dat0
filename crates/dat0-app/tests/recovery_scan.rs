//! P7c T7: boot-time scan of the user's `Recents` workspace folders for
//! interrupted promotions (a `.dat0/` that exists but is missing required
//! files). Candidate set is `Recents` — no full-filesystem scan.

use dat0_app::recovery_scan::scan_incomplete_workspaces;
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

#[test]
fn flags_recent_workspace_with_partial_dat0() {
    let tmp = tempdir().unwrap();

    // Complete workspace: `.dat0/` has BOTH manifest.json + workspace.duckdb
    // (the real `detect_incomplete` contract — both files required).
    let good = tmp.path().join("good");
    let good_dat0 = good.join(".dat0");
    fs::create_dir_all(&good_dat0).unwrap();
    fs::write(good_dat0.join("manifest.json"), "{}").unwrap();
    fs::write(good_dat0.join("workspace.duckdb"), b"db").unwrap();

    // Incomplete: `.dat0/` exists but manifest.json is missing (an interrupted
    // Save/promote — the db moved but the manifest was never written).
    let bad = tmp.path().join("bad");
    let bad_dat0 = bad.join(".dat0");
    fs::create_dir_all(&bad_dat0).unwrap();
    fs::write(bad_dat0.join("workspace.duckdb"), b"db").unwrap();

    // A plain folder recent with NO `.dat0/` at all must NOT be flagged —
    // it's just a not-yet-promoted recent, not an interrupted workspace.
    let good2 = tmp.path().join("good2_no_dat0");
    fs::create_dir_all(&good2).unwrap();

    let recents = vec![good.clone(), bad.clone(), good2.clone()];
    let found = scan_incomplete_workspaces(&recents);
    assert_eq!(found, vec![bad]);
}

/// The boot emit consolidates orphan scratch dirs AND incomplete workspaces
/// into ONE banner whose count is the sum, with the `recovery.review` action.
///
/// Touches the process-global `error_ux::banner::PENDING` queue (`push` /
/// `drain_pending`) — `#[serial]` serialises it against the other queue-touching
/// test so they can't race (the P6a/P6b banner-PENDING flake class).
#[test]
#[serial]
fn recovery_emit_consolidates_orphans_and_incompletes() {
    let _ = dat0_app::error_ux::banner::drain_pending();
    let tmp = tempdir().unwrap();

    // Two orphan scratch dirs (each has a session.json).
    let scratch_root = tmp.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();
    for i in 0..2 {
        let d = scratch_root.join(format!("orphan-{i}"));
        fs::create_dir(&d).unwrap();
        fs::write(d.join("session.json"), r#"{"tabs":[],"active_tab":null}"#).unwrap();
    }

    // One incomplete workspace recent (`.dat0/` missing manifest.json).
    let bad = tmp.path().join("bad");
    let bad_dat0 = bad.join(".dat0");
    fs::create_dir_all(&bad_dat0).unwrap();
    fs::write(bad_dat0.join("workspace.duckdb"), b"db").unwrap();

    let banner = dat0_app::window::recovery_scan_emit(&scratch_root, &[bad])
        .expect("banner emitted when there is work to recover");
    // 2 orphans + 1 incomplete = 3.
    assert!(
        banner.title.contains('3'),
        "consolidated count in title: {}",
        banner.title
    );
    assert_eq!(
        banner.primary.as_ref().unwrap().action_id,
        "recovery.review"
    );

    // The same banner is pushed onto the global pending queue.
    let drained = dat0_app::error_ux::banner::drain_pending();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0], banner);
}

/// Nothing to recover → no banner.
#[test]
#[serial]
fn recovery_emit_silent_when_nothing_to_recover() {
    let _ = dat0_app::error_ux::banner::drain_pending();
    let tmp = tempdir().unwrap();
    let scratch_root = tmp.path().join("scratch");
    fs::create_dir_all(&scratch_root).unwrap();
    assert!(dat0_app::window::recovery_scan_emit(&scratch_root, &[]).is_none());
    assert!(dat0_app::error_ux::banner::drain_pending().is_empty());
}
