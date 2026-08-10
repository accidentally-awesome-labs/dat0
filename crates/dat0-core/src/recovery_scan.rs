//! Boot-time scan for everything a previous run left behind.
//!
//! Two sources, one banner:
//!
//! 1. **Interrupted workspace promotions** among the user's recent workspaces
//!    (P7c T7 / design D4). The candidate set is `Recents` — there is no
//!    full-filesystem scan; an interrupted Save Workspace only matters for a
//!    folder the user has actually touched.
//! 2. **Orphan scratch directories** — a scratch subdir still holding a
//!    `session.json`, left by a session that never exited cleanly.
//!
//! [`recovery_scan_emit`] consolidates both into a single warning banner. It
//! counts rather than lists deliberately: N near-identical banners is not a
//! report, it is noise, and the `recovery.review` action opens the panel that
//! does list them.

use crate::workspace::Home;
use crate::workspace::promote::detect_incomplete;
use std::path::{Path, PathBuf};

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

/// Orphan scratch directories: scratch subdirs containing a `session.json`.
///
/// A subdir without one is not recoverable — there is nothing to restore and
/// nothing the user would recognise — so it is not counted.
pub fn count_orphan_scratch(scratch_root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(scratch_root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.path().join("session.json").is_file())
        .count()
}

/// Emit the boot recovery banner, or nothing when there is nothing to recover.
///
/// The count is `orphan scratch dirs + incomplete workspaces`. The banner is
/// pushed onto the global pending queue so first render picks it up, **and**
/// returned, so a caller (or a test) can inspect it without draining a queue
/// something else is also reading.
pub fn recovery_scan_emit(
    scratch_root: &Path,
    recent_roots: &[PathBuf],
) -> Option<crate::error_ux::Banner> {
    let count = count_orphan_scratch(scratch_root) + scan_incomplete_workspaces(recent_roots).len();
    if count == 0 {
        return None;
    }
    let title = dat0_i18n::t("recovery.banner.title").replace("{count}", &count.to_string());
    let banner =
        crate::error_ux::Banner::warning_with_body(title, dat0_i18n::t("recovery.banner.body"))
            .with_primary(
                dat0_i18n::t("recovery.banner.review"),
                crate::actions::builtin::ids::RECOVERY_REVIEW,
            );
    crate::error_ux::push(banner.clone());
    Some(banner)
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
