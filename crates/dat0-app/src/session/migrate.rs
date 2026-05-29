//! session.json schema migration. v1 (no version field) → v2 (typed transforms).
//!
//! Migration is one-shot + write-back. `load` returns the migrated in-memory
//! `SessionState`; the caller (`Session::recover`) is responsible for persisting
//! the v2 state via the existing atomic-write path on the first dirty save.
//!
//! Forward-incompat (v > current) is a hard error so the caller can surface
//! a Banner instead of silently dropping state.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SESSION_SCHEMA_VERSION, SessionState};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`load`].
#[derive(Debug, thiserror::Error)]
pub enum SessionLoadError {
    /// I/O error reading the file (includes `NotFound` for absent files).
    #[error("session.json io: {0}")]
    Io(#[from] std::io::Error),
    /// The file content is not valid JSON or doesn't match the expected shape.
    #[error("session.json malformed json: {0}")]
    Json(#[from] serde_json::Error),
    /// The file was written by a newer dat0 version; refusing to read.
    ///
    /// The upper layer should surface a Banner: "Session from a newer dat0
    /// version (schema vN). Open with that version or discard."
    #[error("session was written by a newer dat0 version (schema v{0}); refusing to read")]
    UnsupportedVersion(u32),
}

// ---------------------------------------------------------------------------
// Version probe — minimal deserialize to peek at schema_version
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct VersionProbe {
    /// Absent in v1 files; defaults to 1.
    #[serde(default = "version_one")]
    schema_version: u32,
}

fn version_one() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load `path` and migrate forward to the current schema if necessary.
///
/// Returns the live [`SessionState`]. Does NOT write back — the caller
/// (`Session::recover`) decides when to persist. By design, the first
/// `persist()` call after a successful v1→v2 migration writes the v2 file
/// via the existing atomic-write path.
///
/// # Errors
///
/// - [`SessionLoadError::Io`] — file not found or unreadable (NotFound is
///   the caller's signal to fall back to `SessionState::default()`).
/// - [`SessionLoadError::Json`] — malformed JSON.
/// - [`SessionLoadError::UnsupportedVersion`] — schema_version > current.
pub fn load(path: &Path) -> Result<SessionState, SessionLoadError> {
    let raw = std::fs::read_to_string(path)?;
    let probe: VersionProbe = serde_json::from_str(&raw)?;

    match probe.schema_version {
        1 => migrate_v1_to_v2(&raw),
        n if n == SESSION_SCHEMA_VERSION => {
            let state: SessionState = serde_json::from_str(&raw)?;
            Ok(state)
        }
        n if n > SESSION_SCHEMA_VERSION => Err(SessionLoadError::UnsupportedVersion(n)),
        // n == 0 or any gap below 1: treat as unsupported (impossible in
        // practice because the default is 1, but be explicit).
        other => Err(SessionLoadError::UnsupportedVersion(other)),
    }
}

// ---------------------------------------------------------------------------
// Private migration helpers
// ---------------------------------------------------------------------------

/// Migrate a raw v1 JSON string into a v2 `SessionState`.
///
/// v1 had no `schema_version` + no `transform_stack` + no `undo_cursor` on
/// `Tab`. The `#[serde(default)]` attrs on those fields handle the gaps; we
/// just re-parse the whole document (which now has the serde defaults applied)
/// and stamp `schema_version = SESSION_SCHEMA_VERSION`.
fn migrate_v1_to_v2(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    // serde(default) on Tab fields ensures:
    //   transform_stack = Vec::new()
    //   undo_cursor     = 0
    //   extra           = serde_json::Map::new()   (via flatten)
    // No further field-level work is needed.
    Ok(state)
}
