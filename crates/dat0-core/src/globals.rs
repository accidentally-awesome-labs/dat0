//! Process-wide slots that outlive any one window and carry no toolkit types.
//!
//! Split out of `dat0-app`'s `window_registry.rs`, which mixed these with
//! genuinely renderer-shaped state (the live window handles, the focused-view
//! weak entity). Only the toolkit-free half belongs here: it is read from
//! background tasks, from the UDS handler, and from `catalog::tree` — none of
//! which should have to know what is drawing.
//!
//! Every slot is a write-once [`OnceLock`] installed during boot before the
//! event loop starts. Accessors return `None` before installation rather than
//! panicking, so sub-modules stay unit-testable in isolation.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::recents::{RecentEntry, Recents};

/// Where per-user state (session files, recents, logs) lives. Installed by
/// `run_app` so `window.new` can spawn a window without re-deriving it.
static STATE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The recents store. Installed by `run_app` once `AppContext::boot` has
/// resolved its path, so the open/save flows can push entries without
/// re-reading the file.
static RECENTS: OnceLock<Arc<Mutex<Recents>>> = OnceLock::new();

/// Install the state-root path. Idempotent: a second call is a no-op.
pub fn install_state_root(p: PathBuf) {
    let _ = STATE_ROOT.set(p);
}

/// The installed state-root path, or `None` before [`install_state_root`].
pub fn state_root() -> Option<&'static Path> {
    STATE_ROOT.get().map(PathBuf::as_path)
}

/// Install the recents store. Idempotent.
pub fn install_recents(r: Arc<Mutex<Recents>>) {
    let _ = RECENTS.set(r);
}

/// The windows whose session lives in this process right now.
///
/// A window's scratch directory is `$state_root/scratch/<window_id>`, so this
/// set is exactly the scratch directories that are **not** orphans. The
/// recovery panel listed every directory holding a `session.json` — the
/// running window's own included — and its Discard removes the directory
/// outright, so it offered to delete the live session's database from under
/// its engine. Unlike the slots above this one changes after boot: a window
/// registers when it mounts and deregisters when it closes.
static LIVE_WINDOWS: Mutex<std::collections::BTreeSet<uuid::Uuid>> =
    Mutex::new(std::collections::BTreeSet::new());

/// Record that the window `id` is open. Idempotent.
pub fn register_live_window(id: uuid::Uuid) {
    live_windows_guard().insert(id);
}

/// Record that the window `id` has closed. A no-op for an unknown id.
pub fn unregister_live_window(id: uuid::Uuid) {
    live_windows_guard().remove(&id);
}

/// The ids of every window currently open in this process.
pub fn live_windows() -> Vec<uuid::Uuid> {
    live_windows_guard().iter().copied().collect()
}

/// Whether `dir` is the scratch directory of a window that is still open —
/// its final component parses as a live window id.
pub fn is_live_scratch_dir(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| uuid::Uuid::parse_str(n).ok())
        .is_some_and(|id| live_windows_guard().contains(&id))
}

/// A poisoned lock still holds a valid set: the only writers are `insert` and
/// `remove`, which cannot leave it half-updated. Recover rather than panic, so
/// a panic elsewhere cannot make every scratch directory look like an orphan.
fn live_windows_guard() -> std::sync::MutexGuard<'static, std::collections::BTreeSet<uuid::Uuid>> {
    LIVE_WINDOWS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The installed recents store, or `None` before [`install_recents`].
pub fn recents() -> Option<Arc<Mutex<Recents>>> {
    RECENTS.get().cloned()
}

/// Snapshot the recent **workspace** roots, in recents order.
///
/// `Package` recents are excluded — only `.dat0/` workspace folders can be an
/// interrupted promotion. Returns an empty `Vec` if the store is not installed
/// or its lock is poisoned, so a poisoned mutex degrades the recovery sheet
/// rather than taking the process down.
///
/// This mirrors the boot-scan extraction in `run_app`, so the Recovery Sheet
/// and the boot banner see exactly the same candidate set.
pub fn recents_snapshot() -> Vec<PathBuf> {
    recents()
        .and_then(|r| {
            r.lock().ok().map(|g| {
                g.list()
                    .iter()
                    .filter_map(|e| match e {
                        RecentEntry::Workspace { path } => Some(path.clone()),
                        RecentEntry::Package { .. } => None,
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fresh v7 ids per test, so tests sharing the process-wide set cannot
    // observe each other's windows.

    #[test]
    fn a_registered_window_is_live_until_it_closes() {
        let id = uuid::Uuid::now_v7();
        let dir = Path::new("/state/scratch").join(id.to_string());
        assert!(!is_live_scratch_dir(&dir), "not yet open");

        register_live_window(id);
        assert!(live_windows().contains(&id));
        assert!(
            is_live_scratch_dir(&dir),
            "an open window's directory is live"
        );

        unregister_live_window(id);
        assert!(!live_windows().contains(&id));
        assert!(
            !is_live_scratch_dir(&dir),
            "once closed, its directory is an orphan like any other"
        );
    }

    #[test]
    fn only_a_uuid_named_directory_can_be_live() {
        let id = uuid::Uuid::now_v7();
        register_live_window(id);
        assert!(!is_live_scratch_dir(Path::new("/state/scratch/not-a-uuid")));
        assert!(!is_live_scratch_dir(Path::new("/")));
        unregister_live_window(id);
    }
}
