//! session.json schema migration. v1 (no version field) → v2 (typed transforms).
//!
//! Migration is load-and-write-back (eager): a successful v1 → v2 migration is
//! immediately followed by the caller's `Session::persist` call to land the v2
//! file atomically. `load` returns the migrated in-memory `SessionState`; the
//! caller (`Session::recover`) unconditionally persists the returned state before
//! returning.
//!
//! Forward-incompat (v > current) is a hard error so the caller can surface
//! a Banner instead of silently dropping state.

use std::path::Path;

use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
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
/// Returns the live [`SessionState`]. The caller (`Session::recover`)
/// unconditionally persists the returned state via the existing atomic-write
/// path, landing the v2 file on disk on first open (eager write-back).
///
/// # Errors
///
/// - [`SessionLoadError::Io`] — file not found or unreadable (NotFound is
///   the caller's signal to fall back to `SessionState::default()`).
/// - [`SessionLoadError::Json`] — malformed JSON.
/// - [`SessionLoadError::UnsupportedVersion`] — schema_version is not a known
///   version handled by this function.
pub fn load(path: &Path) -> Result<SessionState, SessionLoadError> {
    let raw = std::fs::read_to_string(path)?;
    let probe: VersionProbe = serde_json::from_str(&raw)?;

    // IMPORTANT: use literal version arms, NOT `n if n == SESSION_SCHEMA_VERSION`.
    // The guard form breaks whenever SESSION_SCHEMA_VERSION is bumped: a valid
    // v2 file would fall through to UnsupportedVersion(2) once the const is 3.
    // Literal arms also force the future implementer to add a migration path
    // (e.g. `2 => migrate_v2_to_v3(&raw)`) or get a compile-time inexhaustive
    // match error instead of a silent runtime failure.
    match probe.schema_version {
        1 => migrate_v1_to_v2(&raw),
        2 => {
            let state: SessionState = serde_json::from_str(&raw)?;
            Ok(state)
        }
        // When SESSION_SCHEMA_VERSION advances, add: N => migrate_vN_to_v(N+1)(&raw)
        // The current-version arm becomes the new "load as-is" target.
        n => Err(SessionLoadError::UnsupportedVersion(n)),
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
