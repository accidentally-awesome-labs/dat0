//! Boot-time scan for interrupted workspace promotions among the user's recent
//! workspaces (P7c T7 / design D4). The candidate set is `Recents` — there is
//! no full-filesystem scan; an interrupted Save Workspace only matters for a
//! folder the user has actually touched.
//!
//! This is the *workspace* half of recovery. The *orphan-scratch* half lives in
//! [`crate::window::orphan_scan_emit`]; both are consolidated into one boot
//! banner by [`crate::window::recovery_scan_emit`].

use crate::workspace::Home;
use crate::workspace::promote::detect_incomplete;
use std::path::PathBuf;

/// Recent workspace folders whose `.dat0/` exists but is missing required files
/// (interrupted Save Workspace). Delegates to P7a's
/// [`detect_incomplete`](crate::workspace::promote::detect_incomplete), which
/// flags a `.dat0/` dir that exists yet lacks `manifest.json` *or*
/// `workspace.duckdb`.
///
/// A recent root with no `.dat0/` at all (a plain folder, or a not-yet-promoted
/// recent) is *not* flagged — `detect_incomplete` returns `false` when the dir
/// doesn't exist.
pub fn scan_incomplete_workspaces(recent_roots: &[PathBuf]) -> Vec<PathBuf> {
    recent_roots
        .iter()
        .filter(|root| detect_incomplete(&Home::dat0_dir_for(root)))
        .cloned()
        .collect()
}

/// True iff there is at least one interrupted workspace promotion to recover.
/// (Orphan scratch dirs are counted separately by the caller.)
pub fn has_incomplete(recent_roots: &[PathBuf]) -> bool {
    recent_roots
        .iter()
        .any(|root| detect_incomplete(&Home::dat0_dir_for(root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn seed_complete(root: &std::path::Path) {
        let dat0 = root.join(".dat0");
        fs::create_dir_all(&dat0).unwrap();
        fs::write(dat0.join("manifest.json"), "{}").unwrap();
        fs::write(dat0.join("workspace.duckdb"), b"db").unwrap();
    }

    fn seed_incomplete(root: &std::path::Path) {
        let dat0 = root.join(".dat0");
        fs::create_dir_all(&dat0).unwrap();
        // db moved but manifest never written.
        fs::write(dat0.join("workspace.duckdb"), b"db").unwrap();
    }

    #[test]
    fn excludes_complete_and_bare_includes_incomplete() {
        let tmp = tempfile::TempDir::new().unwrap();
        let good = tmp.path().join("good");
        seed_complete(&good);
        let bad = tmp.path().join("bad");
        seed_incomplete(&bad);
        let bare = tmp.path().join("bare"); // no `.dat0/`
        fs::create_dir_all(&bare).unwrap();

        let roots = vec![good, bad.clone(), bare];
        assert_eq!(scan_incomplete_workspaces(&roots), vec![bad]);
        assert!(has_incomplete(&roots));
    }

    #[test]
    fn empty_when_all_complete() {
        let tmp = tempfile::TempDir::new().unwrap();
        let good = tmp.path().join("good");
        seed_complete(&good);
        let roots = vec![good];
        assert!(scan_incomplete_workspaces(&roots).is_empty());
        assert!(!has_incomplete(&roots));
    }
}
