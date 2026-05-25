//! Recovery panel + supporting non-UI helpers (P3b T5).
//!
//! The non-UI helpers ([`load_for_open`], [`discard`]) are unit-testable
//! without GPUI; the [`open`] entry point is invoked by the
//! `recovery.review` action descriptor on the GPUI main thread.
//!
//! # On-disk shape
//!
//! P3a's `crate::session::Session` serialises tab state as
//! `{ "tabs": [ { "table_name": "...", "source_path": "..." } ],
//!    "active_tab": <usize|null> }` (see `session.rs:23-29` —
//! `SessionState` + `Tab`). The recovery panel does NOT reuse those
//! types directly because:
//!
//! 1. `Session::Tab` owns a `String` for `table_name` and an
//!    `Option<PathBuf>` for `source_path`. The recovery panel only
//!    needs the surface fields a user sees in the list ("which file?"
//!    / "which table?") and never round-trips back to `Session`.
//! 2. Using `serde(rename)` lets the in-memory field names match the
//!    UX vocabulary (`path`, `table`) without touching the on-disk
//!    schema — the recovery flow stays decoupled from future Session
//!    field renames.
//!
//! # GPUI view
//!
//! [`open`] is currently a tracing stub. The T0 spike doc
//! (`docs/internal/gpui-api-notes.md` §0.5b) verifies that
//! `gpui_component::Sheet` is the drawer primitive, but it requires a
//! `&mut Window` context — not the `&mut App` available here. Mounting
//! the Sheet from the action-dispatch path needs a window-handle hop
//! (resolve a target window from `WindowRegistry`, then `update`
//! into it). That plumbing is left as a follow-up; the load-bearing
//! helpers above are covered by `tests/recovery_panel.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// A single restored tab as surfaced to the recovery panel UI.
///
/// JSON keys (`table_name` / `source_path`) match the on-disk shape
/// owned by `session::SessionState`; the in-memory field names
/// (`table` / `path`) match the UX vocabulary.
#[derive(Debug, Clone, Deserialize)]
pub struct RestoredTab {
    #[serde(rename = "source_path")]
    pub path: PathBuf,
    #[serde(rename = "table_name")]
    pub table: String,
}

/// Restored session-level state surfaced to the recovery panel UI.
#[derive(Debug, Clone, Deserialize)]
pub struct RestoredSession {
    pub tabs: Vec<RestoredTab>,
    pub active_tab: Option<usize>,
}

/// Load `session.json` from an orphan scratch dir into a
/// [`RestoredSession`]. Used by the "Open" row action to populate the
/// new window's tab list.
pub fn load_for_open(orphan_dir: &Path) -> Result<RestoredSession> {
    let session_json = orphan_dir.join("session.json");
    let raw = fs::read_to_string(&session_json)
        .with_context(|| format!("read {}", session_json.display()))?;
    let parsed: RestoredSession =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", session_json.display()))?;
    Ok(parsed)
}

/// Permanently remove an orphan scratch dir and everything under it.
/// Used by the "Discard" row action.
pub fn discard(orphan_dir: &Path) -> Result<()> {
    fs::remove_dir_all(orphan_dir).with_context(|| format!("remove {}", orphan_dir.display()))
}

/// Entry point invoked by the `recovery.review` action descriptor
/// (see `actions::builtin::ids::RECOVERY_REVIEW`).
///
/// **Current behaviour:** logs the request. The Sheet-based panel
/// requires `&mut Window` context (see module-level docs) and lands in
/// a follow-up that hops through `WindowRegistry` to resolve a target
/// window. The unit-tested non-UI helpers above are the load-bearing
/// part and are covered by `tests/recovery_panel.rs`.
pub fn open(_app: &mut gpui::App) {
    tracing::info!("recovery_panel::open invoked — Sheet view lands in T5 follow-up");
}
