//! Scratch → workspace promotion: move `scratch.duckdb` + `session.json`
//! into `<root>/.dat0/`, write a manifest, and hand the caller a held flock.
//! Failure-safe ordering: the scratch dir is NOT touched until the move
//! succeeds; the caller removes it LAST.

use super::Home;
use super::lock::WorkspaceLock;
use super::manifest::{self, Manifest};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Result of a successful file-move promotion. The caller adopts `root` + `lock`,
/// then removes `old_scratch_dir` last.
pub struct Promoted {
    pub root: PathBuf,
    pub lock: WorkspaceLock,
    pub old_scratch_dir: PathBuf,
}

impl std::fmt::Debug for Promoted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Promoted")
            .field("root", &self.root)
            .field("old_scratch_dir", &self.old_scratch_dir)
            .finish_non_exhaustive()
    }
}

/// Move a scratch home's files into a new `.dat0/` workspace under `target`.
/// The caller MUST `close()` the DuckDB engine before calling this. The engine
/// need NOT be dropped first: on POSIX the open handle follows the moved inode,
/// and `Session::adopt_workspace` releases it by DROPPING the old engine after
/// the move (which also flushes its WAL into the new location). This
/// close → move → drop sequence is what the T6 integration test proves lossless.
pub fn promote_files(target: &Path, scratch_dir: &Path, now_rfc3339: String) -> Result<Promoted> {
    let dat0 = Home::dat0_dir_for(target);
    if dat0.exists() {
        bail!(
            "target folder is already a dat0 workspace: {}",
            dat0.display()
        );
    }
    std::fs::create_dir_all(dat0.join("lineage"))
        .with_context(|| format!("promote: create {}", dat0.join("lineage").display()))?;
    // Acquire the lock first — fail fast, scratch untouched.
    let lock = WorkspaceLock::try_acquire(&dat0.join("lock"))
        .context("promote: open lock")?
        .ok_or_else(|| anyhow::anyhow!("promote: new workspace lock unexpectedly held"))?;
    // Move the DB (+ WAL sibling) and session.json.
    move_file(
        &scratch_dir.join("scratch.duckdb"),
        &dat0.join("workspace.duckdb"),
    )?;
    let wal = scratch_dir.join("scratch.duckdb.wal");
    if wal.exists() {
        move_file(&wal, &dat0.join("workspace.duckdb.wal"))?;
    }
    move_file(
        &scratch_dir.join("session.json"),
        &dat0.join("session.json"),
    )?;
    // Write the manifest LAST (its presence = "promotion completed").
    manifest::write(&dat0.join("manifest.json"), &Manifest::new(now_rfc3339))
        .context("promote: write manifest")?;
    Ok(Promoted {
        root: target.to_path_buf(),
        lock,
        old_scratch_dir: scratch_dir.to_path_buf(),
    })
}

/// `fs::rename`, falling back to copy+remove on EXDEV (cross-volume).
fn move_file(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc_exdev()) => {
            std::fs::copy(src, dst)
                .with_context(|| format!("promote: copy {} -> {}", src.display(), dst.display()))?;
            std::fs::remove_file(src)
                .with_context(|| format!("promote: remove {}", src.display()))?;
            Ok(())
        }
        Err(e) => Err(e)
            .with_context(|| format!("promote: rename {} -> {}", src.display(), dst.display())),
    }
}

/// EXDEV errno (18 on Linux + macOS).
fn libc_exdev() -> i32 {
    18
}

/// A `.dat0/` dir that exists but lacks `manifest.json` or `workspace.duckdb`
/// is an interrupted promotion.
pub fn detect_incomplete(dat0_dir: &Path) -> bool {
    dat0_dir.exists()
        && (!dat0_dir.join("manifest.json").exists() || !dat0_dir.join("workspace.duckdb").exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seed_scratch(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("scratch.duckdb"), b"FAKEDB").unwrap();
        std::fs::write(dir.join("session.json"), b"{\"schema_version\":8}").unwrap();
    }
    #[test]
    fn promote_moves_files_and_writes_manifest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let scratch = tmp.path().join("scratch/abc");
        seed_scratch(&scratch);
        let target = tmp.path().join("proj");
        std::fs::create_dir_all(&target).unwrap();
        let p = promote_files(&target, &scratch, "2026-06-10T00:00:00Z".into()).unwrap();
        let dat0 = Home::dat0_dir_for(&target);
        assert!(dat0.join("workspace.duckdb").exists());
        assert!(dat0.join("session.json").exists());
        assert!(dat0.join("manifest.json").exists());
        assert!(dat0.join("lineage").is_dir());
        assert!(!scratch.join("scratch.duckdb").exists());
        assert!(
            WorkspaceLock::try_acquire(&dat0.join("lock"))
                .unwrap()
                .is_none()
        );
        drop(p);
    }
    #[test]
    fn promote_refuses_existing_workspace() {
        let tmp = tempfile::TempDir::new().unwrap();
        let scratch = tmp.path().join("scratch/abc");
        seed_scratch(&scratch);
        let target = tmp.path().join("proj");
        std::fs::create_dir_all(Home::dat0_dir_for(&target)).unwrap();
        let err = promote_files(&target, &scratch, "t".into()).unwrap_err();
        assert!(err.to_string().contains("already a dat0 workspace"));
        assert!(scratch.join("scratch.duckdb").exists());
    }
    #[test]
    fn detect_incomplete_flags_missing_manifest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dat0 = tmp.path().join(".dat0");
        std::fs::create_dir_all(&dat0).unwrap();
        std::fs::write(dat0.join("workspace.duckdb"), b"x").unwrap();
        assert!(detect_incomplete(&dat0));
    }
}
