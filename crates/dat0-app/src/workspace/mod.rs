//! Workspace mode (P7a): located `.dat0/` homes, promotion, flock.
pub mod lock;
pub mod manifest;
pub mod networked;
pub mod promote;

use std::path::{Path, PathBuf};

/// Where a window's persistence lives. Both variants are file-backed DuckDB
/// dirs; the difference is *location + guarantees* (design §"scratch vs
/// workspace"), not storage tech.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Home {
    /// `state_root/scratch/{uuid}/` — anonymous, lock-free.
    Scratch { dir: PathBuf },
    /// A user folder (`root`) containing a `.dat0/` subdir (`dat0`).
    Workspace { root: PathBuf, dat0: PathBuf },
}

impl Home {
    /// The `.dat0/` dir for a workspace root.
    pub fn dat0_dir_for(root: &Path) -> PathBuf {
        root.join(".dat0")
    }

    /// The DuckDB file backing this home.
    pub fn db_path(&self) -> PathBuf {
        match self {
            Home::Scratch { dir } => dir.join("scratch.duckdb"),
            Home::Workspace { dat0, .. } => dat0.join("workspace.duckdb"),
        }
    }

    /// The `session.json` path for this home.
    pub fn session_json(&self) -> PathBuf {
        match self {
            Home::Scratch { dir } => dir.join("session.json"),
            Home::Workspace { dat0, .. } => dat0.join("session.json"),
        }
    }

    /// The directory holding `session.json` (its parent — used for fsync of
    /// the parent dir after rename, matching `session::persist`).
    pub fn root_dir(&self) -> &Path {
        match self {
            Home::Scratch { dir } => dir,
            Home::Workspace { dat0, .. } => dat0,
        }
    }

    /// The advisory lock path — `Some` only for workspaces.
    pub fn lock_path(&self) -> Option<PathBuf> {
        match self {
            Home::Scratch { .. } => None,
            Home::Workspace { dat0, .. } => Some(dat0.join("lock")),
        }
    }

    /// The manifest path — `Some` only for workspaces.
    pub fn manifest_path(&self) -> Option<PathBuf> {
        match self {
            Home::Scratch { .. } => None,
            Home::Workspace { dat0, .. } => Some(dat0.join("manifest.json")),
        }
    }

    /// The cross-machine lock manifest path — `Some` only for workspaces.
    pub fn lock_json_path(&self) -> Option<PathBuf> {
        match self {
            Home::Scratch { .. } => None,
            Home::Workspace { dat0, .. } => Some(dat0.join("lock.json")),
        }
    }

    pub fn is_workspace(&self) -> bool {
        matches!(self, Home::Workspace { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_home_paths() {
        let h = Home::Scratch {
            dir: PathBuf::from("/s/scratch/abc"),
        };
        assert_eq!(h.db_path(), PathBuf::from("/s/scratch/abc/scratch.duckdb"));
        assert_eq!(
            h.session_json(),
            PathBuf::from("/s/scratch/abc/session.json")
        );
        assert_eq!(h.lock_path(), None);
        assert_eq!(h.manifest_path(), None);
        assert_eq!(h.lock_json_path(), None);
        assert!(!h.is_workspace());
    }

    #[test]
    fn workspace_home_paths() {
        let root = PathBuf::from("/u/proj");
        let h = Home::Workspace {
            root: root.clone(),
            dat0: Home::dat0_dir_for(&root),
        };
        assert_eq!(h.db_path(), PathBuf::from("/u/proj/.dat0/workspace.duckdb"));
        assert_eq!(
            h.session_json(),
            PathBuf::from("/u/proj/.dat0/session.json")
        );
        assert_eq!(h.lock_path(), Some(PathBuf::from("/u/proj/.dat0/lock")));
        assert_eq!(
            h.manifest_path(),
            Some(PathBuf::from("/u/proj/.dat0/manifest.json"))
        );
        assert_eq!(
            h.lock_json_path(),
            Some(PathBuf::from("/u/proj/.dat0/lock.json"))
        );
        assert!(h.is_workspace());
    }
}
