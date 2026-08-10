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
