//! Boot-time scan for everything a previous run left behind.
//!
//! Two sources, one banner:
//!
//! 1. **Interrupted workspace promotions** among the user's recent workspaces
//!    (P7c T7 / design D4). The candidate set is `Recents` — there is no
//!    full-filesystem scan; an interrupted Save Workspace only matters for a
//!    folder the user has actually touched.
//! 2. **Orphan scratch directories** — a scratch subdir left by a window that
//!    is no longer open, whose `session.json` holds something that exists
//!    nowhere else: a view, typed or saved SQL, a table no file backs.
//!
//! Every window leaves its scratch directory behind, closed or crashed. The
//! ones holding nothing to recover — a copy of files the user still has — are
//! removed at boot ([`sweep_scratch`]) rather than counted: a banner that
//! counted every window ever opened would say nothing.
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

/// Orphan scratch directories: those of windows no longer open whose
/// `session.json` holds something to recover, in no particular order.
///
/// A subdir without a `session.json` is not recoverable — there is nothing to
/// restore and nothing the user would recognise. One whose session cannot be
/// read is: what it holds is unknown, so it is offered rather than lost.
pub fn recoverable_scratch(scratch_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(scratch_root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|dir| !crate::globals::is_live_scratch_dir(dir))
        .filter(|dir| {
            let json = dir.join("session.json");
            json.is_file()
                && !crate::session::migrate::load(&json).is_ok_and(|s| s.nothing_to_recover())
        })
        .collect()
}

/// How many orphan scratch directories there are ([`recoverable_scratch`]).
pub fn count_orphan_scratch(scratch_root: &Path) -> usize {
    recoverable_scratch(scratch_root).len()
}

/// Remove the scratch directories of windows no longer open that hold nothing
/// to recover: no `session.json` at all, or one whose every tab is a file
/// still on disk, shown as it is, with nothing typed, run or saved
/// (`SessionState::nothing_to_recover`). Returns how many went.
///
/// Run at boot, before any window opens. Only directories named for a window
/// id are touched; a session that cannot be read is kept, and so is anything
/// that fails to delete — the next boot tries again.
pub fn sweep_scratch(scratch_root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(scratch_root) else {
        return 0;
    };
    let mut removed = 0;
    for dir in entries.flatten().map(|e| e.path()) {
        let named_for_a_window = dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| uuid::Uuid::parse_str(n).is_ok());
        if !dir.is_dir() || !named_for_a_window || crate::globals::is_live_scratch_dir(&dir) {
            continue;
        }
        let json = dir.join("session.json");
        let disposable = !json.exists()
            || crate::session::migrate::load(&json).is_ok_and(|s| s.nothing_to_recover());
        if !disposable {
            continue;
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(dir = %dir.display(), error = %e, "sweep: could not remove"),
        }
    }
    removed
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

    use crate::session::{SessionState, SqlTabState, Tab};
    use dat0_engine::{SortDirection, SortKey, Transformation};

    fn tab(table: &str, path: Option<&std::path::Path>) -> Tab {
        Tab {
            table_name: table.into(),
            source_path: path.map(Into::into),
            transform_stack: Vec::new(),
            undo_cursor: 0,
            extra: Default::default(),
        }
    }

    /// A window's scratch directory, holding `state` (or no session at all).
    fn window_dir(scratch: &Path, state: Option<&SessionState>) -> PathBuf {
        let dir = scratch.join(uuid::Uuid::now_v7().to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("scratch.duckdb"), b"db").unwrap();
        if let Some(state) = state {
            fs::write(dir.join("session.json"), serde_json::to_vec(state).unwrap()).unwrap();
        }
        dir
    }

    #[test]
    fn the_sweep_removes_copies_of_files_and_keeps_everything_else() {
        let tmp = tempfile::TempDir::new().unwrap();
        let scratch = tmp.path().join("scratch");
        let csv = tmp.path().join("sales.csv");
        fs::write(&csv, "a\n1\n").unwrap();

        let opened = || SessionState {
            tabs: vec![tab("sales", Some(&csv))],
            ..SessionState::default()
        };
        let bare = window_dir(&scratch, None);
        let looked_at = window_dir(&scratch, Some(&opened()));
        let empty = window_dir(&scratch, Some(&SessionState::default()));

        let mut sorted = opened();
        sorted.tabs[0].transform_stack = vec![Transformation::Sort {
            keys: vec![SortKey {
                column: "a".into(),
                direction: SortDirection::Desc,
            }],
        }];
        sorted.tabs[0].undo_cursor = 1;
        let mut typed = opened();
        typed.sql_tabs = vec![SqlTabState {
            id: uuid::Uuid::now_v7(),
            title: "Query 1".into(),
            sql: "select 1".into(),
        }];
        let gone_file = SessionState {
            tabs: vec![tab("old", Some(&tmp.path().join("deleted.csv")))],
            ..SessionState::default()
        };
        let made_by_sql = SessionState {
            tabs: vec![tab("summary", None)],
            ..SessionState::default()
        };
        let kept = [
            window_dir(&scratch, Some(&sorted)),
            window_dir(&scratch, Some(&typed)),
            window_dir(&scratch, Some(&gone_file)),
            window_dir(&scratch, Some(&made_by_sql)),
        ];
        let unreadable = window_dir(&scratch, None);
        fs::write(unreadable.join("session.json"), "{ not json").unwrap();
        let not_ours = scratch.join("notes");
        fs::create_dir_all(&not_ours).unwrap();
        let live = window_dir(&scratch, Some(&SessionState::default()));
        let live_id = uuid::Uuid::parse_str(live.file_name().unwrap().to_str().unwrap()).unwrap();
        crate::globals::register_live_window(live_id);

        assert_eq!(sweep_scratch(&scratch), 3);
        crate::globals::unregister_live_window(live_id);

        for gone in [&bare, &looked_at, &empty] {
            assert!(
                !gone.exists(),
                "{} holds nothing to recover",
                gone.display()
            );
        }
        for dir in kept.iter().chain([&unreadable, &not_ours, &live]) {
            assert!(dir.exists(), "{} is kept", dir.display());
        }
        // What the banner counts and the panel lists. The window that was
        // open is closed now, and blank, so it is not offered either.
        let mut offered = recoverable_scratch(&scratch);
        offered.sort();
        let mut want: Vec<PathBuf> = kept.iter().chain([&unreadable]).cloned().collect();
        want.sort();
        assert_eq!(offered, want);
    }

    #[test]
    fn an_open_window_is_neither_swept_nor_offered() {
        let tmp = tempfile::TempDir::new().unwrap();
        let scratch = tmp.path().join("scratch");
        let state = SessionState {
            tabs: vec![tab("summary", None)],
            ..SessionState::default()
        };
        let dir = window_dir(&scratch, Some(&state));
        let id = uuid::Uuid::parse_str(dir.file_name().unwrap().to_str().unwrap()).unwrap();
        crate::globals::register_live_window(id);
        assert!(recoverable_scratch(&scratch).is_empty(), "its own window's");
        crate::globals::unregister_live_window(id);
        assert_eq!(recoverable_scratch(&scratch), [dir], "once it closes");
    }
}
