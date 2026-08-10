//! P7b exit criterion: a networked workspace with a foreign live lock.json
//! is detected as a cross-machine conflict; same-machine dead/absent records
//! are reclaimable; a clean session-lifetime claim tombstones on drop.

use std::path::Path;

use dat0_core::settings::Workspace as WorkspaceSettings;
use dat0_core::workspace::lock_manifest::{AcquireOutcome, LockManifest, acquire, claim};
use dat0_core::workspace::networked::is_networked;

fn write(path: &Path, rec: &LockManifest) {
    std::fs::write(path, serde_json::to_string_pretty(rec).unwrap()).unwrap();
}

#[test]
fn foreign_live_record_is_a_cross_machine_conflict() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock_json = tmp.path().join("lock.json");
    write(
        &lock_json,
        &LockManifest {
            pid: 4321,
            hostname: "another-machine".into(),
            started_at: "2026-06-11T10:04:00Z".into(),
            dat0_version: "0.1.0".into(),
            tombstoned: false,
        },
    );
    match acquire(&lock_json, "this-machine").unwrap() {
        AcquireOutcome::ConflictForeign(h) => {
            assert_eq!(h.hostname, "another-machine");
            assert_eq!(h.started_at, "2026-06-11T10:04:00Z");
        }
        other => panic!("expected ConflictForeign, got {other:?}"),
    }
}

#[test]
fn claim_then_drop_makes_next_open_available() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock_json = tmp.path().join("lock.json");
    {
        let _g = claim(&lock_json, "2026-06-11T10:04:00Z".into()).unwrap();
        // While held, a same-host opener sees it held (our own live pid).
        let host = dat0_core::workspace::identity::hostname();
        assert!(matches!(
            acquire(&lock_json, &host).unwrap(),
            AcquireOutcome::HeldSameMachine(_)
        ));
    } // guard drops → tombstone
    let host = dat0_core::workspace::identity::hostname();
    assert_eq!(
        acquire(&lock_json, &host).unwrap(),
        AcquireOutcome::Available
    );
}

#[test]
fn dropbox_path_is_networked_local_is_not() {
    let s = WorkspaceSettings::default();
    assert!(is_networked(Path::new("/Users/x/Dropbox/proj"), &s));
    assert!(!is_networked(Path::new("/Users/x/Projects/proj"), &s));
}
