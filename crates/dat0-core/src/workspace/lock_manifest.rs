//! `.dat0/lock.json` — the cross-machine holder record (design D1). Written
//! only for networked workspaces. acquire/tombstone state machine; NO
//! heartbeat (sync-drive lag makes TTL staleness a corruption risk).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::identity;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockManifest {
    pub pid: u32,
    pub hostname: String,
    pub started_at: String, // epoch-secs string (e.g. `now_epoch_secs().to_string()`); display-only, never used for a liveness decision
    pub dat0_version: String,
    #[serde(default)]
    pub tombstoned: bool,
}

impl LockManifest {
    /// Build a record describing THIS process, now.
    pub fn current(started_at: String) -> Self {
        Self {
            pid: std::process::id(),
            hostname: identity::hostname(),
            started_at,
            dat0_version: env!("CARGO_PKG_VERSION").to_string(),
            tombstoned: false,
        }
    }
}

/// Outcome of reading the existing `lock.json` and deciding what an opener
/// should do. `acquire` does NOT write — `claim` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireOutcome {
    /// No live holder (absent / tombstoned) — safe to claim.
    Available,
    /// Same machine, holder pid dead — a crashed prior run; safe to claim.
    Reclaimable,
    /// Same machine, holder pid alive — owned in-process (registry handles it).
    HeldSameMachine(LockManifest),
    /// Another machine holds a live (non-tombstoned) record — WARN; do not auto-resolve.
    ConflictForeign(LockManifest),
}

/// Read `lock.json` and classify (no write). `me_host` is this machine's
/// hostname (passed in so tests can simulate a foreign host).
pub fn acquire(lock_json: &Path, me_host: &str) -> Result<AcquireOutcome> {
    let raw = match std::fs::read_to_string(lock_json) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(AcquireOutcome::Available),
        Err(e) => return Err(e).with_context(|| format!("lock.json read {}", lock_json.display())),
    };
    let rec: LockManifest = match serde_json::from_str(&raw) {
        Ok(r) => r,
        // A corrupt record must not wedge the open — treat as available, log.
        Err(e) => {
            tracing::warn!(?e, path = %lock_json.display(), "lock.json malformed; treating as available");
            return Ok(AcquireOutcome::Available);
        }
    };
    if rec.tombstoned {
        return Ok(AcquireOutcome::Available);
    }
    if rec.hostname == me_host {
        if identity::pid_alive(rec.pid) {
            Ok(AcquireOutcome::HeldSameMachine(rec))
        } else {
            Ok(AcquireOutcome::Reclaimable)
        }
    } else {
        Ok(AcquireOutcome::ConflictForeign(rec))
    }
}

/// Write our record atomically and return a guard that tombstones on drop.
pub fn claim(lock_json: &Path, started_at: String) -> Result<LockManifestGuard> {
    write(lock_json, &LockManifest::current(started_at))?;
    Ok(LockManifestGuard {
        lock_json: lock_json.to_path_buf(),
    })
}

fn write(lock_json: &Path, rec: &LockManifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(rec).context("lock.json serialize")?;
    let tmp = lock_json.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes)
        .with_context(|| format!("lock.json write tmp {}", tmp.display()))?;
    std::fs::rename(&tmp, lock_json)
        .with_context(|| format!("lock.json rename {}", lock_json.display()))?;
    Ok(())
}

/// Mark the record tombstoned (clean release). A missing file is a no-op;
/// an unreadable file (permissions / I/O error) propagates the error.
pub fn tombstone(lock_json: &Path) -> Result<()> {
    let mut rec = match std::fs::read_to_string(lock_json) {
        Ok(s) => serde_json::from_str::<LockManifest>(&s)
            .unwrap_or_else(|_| LockManifest::current(String::new())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("lock.json tombstone read {}", lock_json.display()));
        }
    };
    rec.tombstoned = true;
    write(lock_json, &rec)
}

/// RAII: while alive, our record is the live holder; on drop we tombstone so
/// the next opener (this machine or another) sees a clean slate. Mirrors the
/// flock `WorkspaceLock` lifetime (held by `Session`, dropped on window close).
pub struct LockManifestGuard {
    lock_json: PathBuf,
}

impl LockManifestGuard {
    pub fn path(&self) -> &Path {
        &self.lock_json
    }
}

impl Drop for LockManifestGuard {
    fn drop(&mut self) {
        if let Err(e) = tombstone(&self.lock_json) {
            tracing::warn!(
                ?e,
                "lock.json tombstone on drop failed — stale live record may cause a foreign conflict warning"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_raw(path: &Path, rec: &LockManifest) {
        std::fs::write(path, serde_json::to_string_pretty(rec).unwrap()).unwrap();
    }

    #[test]
    fn absent_is_available() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("lock.json");
        assert_eq!(acquire(&p, "me").unwrap(), AcquireOutcome::Available);
    }

    #[test]
    fn tombstoned_is_available() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("lock.json");
        let mut rec = LockManifest::current("t".into());
        rec.tombstoned = true;
        write_raw(&p, &rec);
        assert_eq!(
            acquire(&p, &rec.hostname).unwrap(),
            AcquireOutcome::Available
        );
    }

    #[test]
    fn foreign_host_is_conflict() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("lock.json");
        let rec = LockManifest {
            pid: 4321,
            hostname: "other-machine".into(),
            started_at: "2026-06-11T10:04:00Z".into(),
            dat0_version: "0.1.0".into(),
            tombstoned: false,
        };
        write_raw(&p, &rec);
        // We are NOT "other-machine".
        match acquire(&p, "my-machine").unwrap() {
            AcquireOutcome::ConflictForeign(h) => assert_eq!(h.hostname, "other-machine"),
            other => panic!("expected ConflictForeign, got {other:?}"),
        }
    }

    #[test]
    fn same_host_dead_pid_is_reclaimable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("lock.json");
        let rec = LockManifest {
            pid: 999_999, // dead
            hostname: identity::hostname(),
            started_at: "t".into(),
            dat0_version: "0.1.0".into(),
            tombstoned: false,
        };
        write_raw(&p, &rec);
        assert_eq!(
            acquire(&p, &rec.hostname).unwrap(),
            AcquireOutcome::Reclaimable
        );
    }

    #[test]
    fn same_host_live_pid_is_held() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("lock.json");
        let rec = LockManifest {
            pid: std::process::id(), // alive
            hostname: identity::hostname(),
            started_at: "t".into(),
            dat0_version: "0.1.0".into(),
            tombstoned: false,
        };
        write_raw(&p, &rec);
        assert!(matches!(
            acquire(&p, &rec.hostname).unwrap(),
            AcquireOutcome::HeldSameMachine(_)
        ));
    }

    #[test]
    fn tombstone_on_corrupt_file_makes_next_acquire_available() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("lock.json");
        std::fs::write(&p, "{ not valid json").unwrap();
        tombstone(&p).unwrap();
        assert_eq!(acquire(&p, "me").unwrap(), AcquireOutcome::Available);
    }

    #[test]
    fn claim_writes_then_guard_tombstones_on_drop() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("lock.json");
        {
            let _g = claim(&p, "2026-06-11T10:04:00Z".into()).unwrap();
            let rec: LockManifest =
                serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            assert!(!rec.tombstoned);
            assert_eq!(rec.pid, std::process::id());
        } // guard drops here
        let rec: LockManifest =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(rec.tombstoned, "guard drop must tombstone");
    }
}
