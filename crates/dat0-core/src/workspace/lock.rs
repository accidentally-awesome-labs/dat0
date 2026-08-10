//! Per-workspace advisory flock. Mirrors `crate::app_lock`'s pattern:
//! a non-blocking `try_lock_exclusive` on `.dat0/lock`, held for the
//! session lifetime via an owned `File`, auto-released on `Drop` /
//! process exit. Cross-machine / sync-drive locking is P7b.

use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result};

/// RAII flock guard. While alive, this process holds the exclusive advisory
/// lock on the workspace's `lock` file.
pub struct WorkspaceLock {
    // Held for its flock RAII lifetime; released when the handle drops.
    #[expect(dead_code, reason = "held for RAII flock lifetime, not read directly")]
    file: File,
}

impl WorkspaceLock {
    /// Try to acquire the workspace lock at `lock_path`. Returns
    /// `Ok(Some(guard))` if acquired, `Ok(None)` if another live holder has
    /// it (contention), `Err` only on I/O failure (e.g. unwritable dir).
    pub fn try_acquire(lock_path: &Path) -> Result<Option<Self>> {
        use fs4::fs_std::FileExt;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .with_context(|| format!("open workspace lock {}", lock_path.display()))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(_) => Ok(None),
        }
    }
}
