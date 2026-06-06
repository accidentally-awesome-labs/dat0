//! session.json schema migration. v1 (no version field) → v2 (typed transforms)
//! → v3 (additive `Edit`/`RowDelete` transform variants, P4b) → v4 (projection
//! variants allowlist + active-stack-only persistence, P4c) → v5 (additive SQL
//! console tabs `sql_tabs` + `active_sql_tab`, P5a) → v6 (additive query stores
//! `query_history` + `saved_queries`, P5b) → v7 (additive `attachments`, P5c) →
//! v8 (additive `ui` catalog/inspector dock + tree state, P6a).
//!
//! Migration is load-and-write-back (eager): a successful migration is
//! immediately followed by the caller's `Session::persist` call to land the
//! current-version file atomically. `load` returns the migrated in-memory
//! `SessionState`; the caller (`Session::recover`) unconditionally persists the
//! returned state before returning.
//!
//! Forward-incompat is a hard error so the caller can surface a Banner instead
//! of silently dropping state. There are TWO forward-incompat shapes:
//!   - [`SessionLoadError::UnsupportedVersion`] — `schema_version` is newer than
//!     this build knows.
//!   - [`SessionLoadError::ForwardIncompatTransform`] — the version is known but
//!     a tab's `transform_stack` carries a transform `kind` this build doesn't
//!     recognize (a newer binary wrote a variant we don't have). Detected by an
//!     allowlist pre-check, NOT by deserializing and pattern-matching serde's
//!     error text.
//!
//! Both route to the banner via [`SessionLoadError::is_forward_incompat`].

use std::path::Path;

use serde::Deserialize;

use super::{SESSION_SCHEMA_VERSION, SessionState};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`load`] / [`load_str`].
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
    /// The schema version is known, but a tab's transform stack carries a
    /// transform `kind` this build does not recognize — i.e. a newer dat0 wrote
    /// a `Transformation` variant we don't have. Surfaces the SAME
    /// forward-incompat Banner as [`Self::UnsupportedVersion`] rather than a
    /// confusing "malformed JSON" error.
    #[error(
        "session contains an unknown transform kind '{0}' written by a newer dat0 version; refusing to read"
    )]
    ForwardIncompatTransform(String),
}

impl SessionLoadError {
    /// Whether this error means "written by a newer dat0 version" — the signal
    /// for the upper layer to surface the forward-incompat Banner (rather than
    /// treating it as corruption / falling back to default state).
    ///
    /// Covers both a newer top-level `schema_version`
    /// ([`Self::UnsupportedVersion`]) and a known version carrying an unknown
    /// transform variant ([`Self::ForwardIncompatTransform`]).
    pub fn is_forward_incompat(&self) -> bool {
        matches!(
            self,
            SessionLoadError::UnsupportedVersion(_) | SessionLoadError::ForwardIncompatTransform(_)
        )
    }
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
// Known transform `kind` discriminators (forward-incompat allowlist)
// ---------------------------------------------------------------------------

/// The set of `Transformation` `kind` discriminators this build understands.
///
/// `Transformation` is `#[serde(tag = "kind", rename_all = "snake_case")]`, so
/// each stack element is a JSON object with a `"kind"` string. A value outside
/// this set means a newer dat0 wrote a variant we don't have — a
/// forward-incompat condition, not corruption.
///
/// MUST stay in sync with `dat0_engine::Transformation`. When a variant is
/// added there, add its snake_case tag here (a stale allowlist would reject a
/// transform this build CAN handle).
const KNOWN_TRANSFORM_KINDS: &[&str] = &[
    "filter",
    "sort",
    "edit",
    "row_delete",
    "reorder",
    "rename",
    "delete_column",
];

/// Scan a parsed session document for any tab whose `transform_stack` contains
/// a transform with an unrecognized top-level `kind`. Returns the offending
/// kind string if found.
///
/// This is the allowlist PRE-CHECK: it runs before strict deserialization so an
/// unknown variant maps to [`SessionLoadError::ForwardIncompatTransform`]
/// instead of a generic serde "unknown variant" `Json` error. Pre-checking is
/// more robust than matching serde's error text (which is not part of serde's
/// stable API and varies by representation).
///
/// Only the TOP-LEVEL transform `kind` is checked (the variant a newer binary
/// would add). Malformed inner shapes still surface as `Json` errors downstream.
fn find_unknown_transform_kind(doc: &serde_json::Value) -> Option<String> {
    let tabs = doc.get("tabs")?.as_array()?;
    for tab in tabs {
        let Some(stack) = tab.get("transform_stack").and_then(|s| s.as_array()) else {
            continue;
        };
        for op in stack {
            if let Some(kind) = op.get("kind").and_then(|k| k.as_str()) {
                if !KNOWN_TRANSFORM_KINDS.contains(&kind) {
                    return Some(kind.to_string());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load `path` and migrate forward to the current schema if necessary.
///
/// Returns the live [`SessionState`]. The caller (`Session::recover`)
/// unconditionally persists the returned state via the existing atomic-write
/// path, landing the current-version file on disk on first open (eager
/// write-back).
///
/// # Errors
///
/// - [`SessionLoadError::Io`] — file not found or unreadable (NotFound is
///   the caller's signal to fall back to `SessionState::default()`).
/// - [`SessionLoadError::Json`] — malformed JSON.
/// - [`SessionLoadError::UnsupportedVersion`] — schema_version is newer than
///   this build handles.
/// - [`SessionLoadError::ForwardIncompatTransform`] — a known version carrying
///   an unknown transform `kind` written by a newer dat0.
pub fn load(path: &Path) -> Result<SessionState, SessionLoadError> {
    let raw = std::fs::read_to_string(path)?;
    load_str(&raw)
}

/// Parse + migrate a session.json **string** (no I/O). Used by [`load`] and by
/// tests that drive migration directly from in-memory fixtures.
///
/// Runs the forward-incompat transform-`kind` allowlist pre-check before any
/// strict deserialization so an unrecognized variant becomes
/// [`SessionLoadError::ForwardIncompatTransform`] rather than a bare serde
/// error.
///
/// # Errors
/// Same as [`load`], minus the I/O cases.
pub fn load_str(raw: &str) -> Result<SessionState, SessionLoadError> {
    let probe: VersionProbe = serde_json::from_str(raw)?;

    // IMPORTANT: use literal version arms, NOT `n if n == SESSION_SCHEMA_VERSION`.
    // The guard form breaks whenever SESSION_SCHEMA_VERSION is bumped: a valid
    // current-version file would fall through to UnsupportedVersion once the
    // const advances. Literal arms also force the future implementer to add a
    // migration path (e.g. `3 => migrate_v3_to_v4(raw)`) or get an
    // inexhaustive-match error instead of a silent runtime failure.
    match probe.schema_version {
        1 => migrate_v1_to_v7(raw),
        2 => migrate_v2_to_v7(raw),
        3 => migrate_v3_to_v7(raw),
        4 => migrate_v4_to_v7(raw),
        5 => migrate_v5_to_v7(raw),
        6 => migrate_v6_to_v7(raw),
        7 => migrate_v7_to_v8(raw),
        8 => {
            // Forward-incompat guard: a NEWER dat0 (writing the same v8 schema)
            // may have introduced a transform variant this build doesn't know.
            // Scan the current-version document's transform stacks and map any
            // unknown TOP-LEVEL `kind` to the forward-incompat banner path BEFORE
            // strict deserialization (which would otherwise fail with a generic
            // "unknown variant" Json error). Only meaningful for the current
            // version: past-version files predate every future variant, so an
            // unknown `kind` there is genuine corruption and is correctly left to
            // surface as a plain parse error in the migration helper. Future
            // versions are rejected by the catch-all arm below before we pay for
            // the extra `Value` parse.
            let doc: serde_json::Value = serde_json::from_str(raw)?;
            if let Some(kind) = find_unknown_transform_kind(&doc) {
                return Err(SessionLoadError::ForwardIncompatTransform(kind));
            }
            let state: SessionState = serde_json::from_str(raw)?;
            Ok(state)
        }
        // When SESSION_SCHEMA_VERSION advances, add: N => migrate_vN_to_v(N+1)(raw)
        // and make the new current-version (now 8) arm above the "load as-is"
        // target.
        n => Err(SessionLoadError::UnsupportedVersion(n)),
    }
}

// ---------------------------------------------------------------------------
// Private migration helpers
// ---------------------------------------------------------------------------

/// Migrate a raw v1 JSON string straight to the current (v7) `SessionState`.
///
/// v1 had no `schema_version` + no `transform_stack` + no `undo_cursor` on
/// `Tab`. The `#[serde(default)]` attrs on those fields handle the gaps; we
/// just re-parse the whole document (which now has the serde defaults applied)
/// and stamp `schema_version = SESSION_SCHEMA_VERSION`.
///
/// v2 → v3 → v4 → v5 → v6 → v7 are identity / additive reshapes (see
/// [`migrate_v2_to_v7`], [`migrate_v3_to_v7`], [`migrate_v4_to_v7`],
/// [`migrate_v5_to_v7`], [`migrate_v6_to_v7`]), so the v1 → v7 path is the same
/// single re-parse + version stamp — no intermediate hops are needed (v1 stacks
/// are always empty, so the v4 redo-truncation is a no-op, and the v5 SQL-tab +
/// v6 query-store + v7 attachments fields default via serde).
fn migrate_v1_to_v7(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    // serde(default) on Tab fields ensures:
    //   transform_stack = Vec::new()
    //   undo_cursor     = 0
    //   extra           = serde_json::Map::new()   (via flatten)
    // serde(default) on SessionState fields ensures:
    //   sql_tabs        = Vec::new()
    //   active_sql_tab  = None
    //   query_history   = Vec::new()
    //   saved_queries   = Vec::new()
    //   attachments     = Vec::new()
    // No further field-level work is needed.
    Ok(state)
}

/// Migrate a raw v2 JSON string to a v7 `SessionState` — IDENTITY.
///
/// v3 adds the `Edit` / `RowDelete` `Transformation` variants, which are purely
/// additive tagged-enum cases. v4 adds the projection variants (Reorder/Rename/
/// DeleteColumn) and truncates to the active slice. v5 adds SQL console tabs,
/// v6 adds the query stores (`query_history` + `saved_queries`), and v7 adds
/// `attachments` — all additive, serde-defaulted. A v2 file (filter/sort only)
/// parses into the exact same in-memory shape; stacks parsed from v2 are by
/// definition "active only" (no redo tail in v2 format). The only change is the
/// version stamp. Re-parse and bump `schema_version`.
fn migrate_v2_to_v7(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    Ok(state)
}

/// Migrate a raw v3 JSON string to a v7 `SessionState`.
///
/// v4 adds the display-only projection variants (Reorder/Rename/DeleteColumn) —
/// additive tagged-enum cases — AND changes persistence to the ACTIVE stack only
/// (the P4c history zipper drops the in-stack redo tail). So we truncate each
/// tab's `transform_stack` to `undo_cursor` (its active slice). v5 then adds SQL
/// console tabs, v6 adds the query stores, and v7 adds `attachments` — all
/// additive (serde-defaulted). Truncate, then stamp v7.
fn migrate_v3_to_v7(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    for tab in &mut state.tabs {
        let keep = tab.undo_cursor.min(tab.transform_stack.len());
        tab.transform_stack.truncate(keep);
        tab.undo_cursor = keep;
    }
    state.schema_version = SESSION_SCHEMA_VERSION;
    Ok(state)
}

/// Migrate a raw v4 JSON string to a v7 `SessionState`.
///
/// v5 adds SQL console tabs (`sql_tabs` + `active_sql_tab`), v6 adds the query
/// stores (`query_history` + `saved_queries`), and v7 adds `attachments`. Purely
/// additive: a v4 file lacks all of these fields, so serde `#[serde(default)]`
/// fills them with empty vecs / `None`. No table-tab reshaping. Just stamp the
/// version.
fn migrate_v4_to_v7(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    Ok(state)
}

/// Migrate a raw v5 JSON string to a v7 `SessionState`.
///
/// v6 adds `query_history` + `saved_queries` and v7 adds `attachments`. Purely
/// additive: a v5 file lacks all three, so `#[serde(default)]` fills them with
/// empty vecs. Just stamp the version.
fn migrate_v5_to_v7(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    Ok(state)
}

/// Migrate a raw v6 JSON string to a v7 `SessionState`.
///
/// v7 adds `attachments` (MotherDuck + sqlite). Purely additive: a v6 file lacks
/// the field, so `#[serde(default)]` fills it with an empty vec. Just stamp the
/// version.
fn migrate_v6_to_v7(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    Ok(state)
}

/// Migrate a raw v7 JSON string to a v8 `SessionState` — IDENTITY.
///
/// v8 adds `ui` (catalog/inspector dock + tree state); additive,
/// serde-defaulted: a v7 file lacks the `ui` field, so `#[serde(default)]`
/// fills it with `SessionUiState::default()` (both docks hidden, no expanded
/// nodes, no selection). Re-parse + stamp the version.
fn migrate_v7_to_v8(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    Ok(state)
}
