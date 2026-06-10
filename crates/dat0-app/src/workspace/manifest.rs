//! `.dat0/manifest.json` — workspace identity, separate from `session.json`.
//! Workspace-level + durable (P8a will ratify this schema). Window/view state
//! stays in `session.json`.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Current workspace format version. Bump when the on-disk manifest shape
/// changes. dat0 1.x reads format 1.x (design §forward-compat).
pub const WORKSPACE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format_version: u32,
    pub dat0_version: String,
    pub workspace_id: uuid::Uuid,
    pub created_at: String,  // RFC3339
    pub modified_at: String, // RFC3339
}

impl Manifest {
    /// Build a fresh manifest for a newly promoted workspace.
    pub fn new(now_rfc3339: String) -> Self {
        Self {
            format_version: WORKSPACE_FORMAT_VERSION,
            dat0_version: env!("CARGO_PKG_VERSION").to_string(),
            workspace_id: uuid::Uuid::now_v7(),
            created_at: now_rfc3339.clone(),
            modified_at: now_rfc3339,
        }
    }
}

/// Write `manifest` to `path` atomically (tmp + rename), matching
/// `session::persist`'s durability contract.
pub fn write(path: &Path, manifest: &Manifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest).context("manifest: serialize")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes)
        .with_context(|| format!("manifest: write tmp {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("manifest: rename to {}", path.display()))?;
    Ok(())
}

/// Read + parse a manifest from `path`.
pub fn read(path: &Path) -> Result<Manifest> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("manifest: read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("manifest: parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_through_disk() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("manifest.json");
        let m = Manifest::new("2026-06-10T00:00:00Z".to_string());
        write(&path, &m).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(m, back);
        assert_eq!(back.format_version, 1);
        assert_eq!(back.dat0_version, env!("CARGO_PKG_VERSION"));
        // tmp file must not linger after a successful write.
        assert!(!path.with_extension("json.tmp").exists());
    }
}
