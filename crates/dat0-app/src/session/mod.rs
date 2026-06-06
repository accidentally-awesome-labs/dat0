//! Per-window scratch session: a UUID-named directory under `state_root/scratch/`,
//! a DuckDB engine bound to that directory, and a tab list that is persisted
//! atomically to `session.json` on every mutation. Recovery reads an existing
//! scratch dir back into a live `Session` struct, using default state when
//! `session.json` is absent (e.g. after a crash or mid-write interruption).
//! Atomic-rename persistence is the contract: `.tmp` files must never be present
//! after a successful `persist()`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, Transformation};

pub mod migrate;
pub mod queries;
pub use migrate::SessionLoadError;
use queries::{HistoryEntry, SavedQuery};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// Current schema version. Bump whenever fields are added/removed.
///
/// v2 → v3 (P4b) is an IDENTITY migration: the `Edit` / `RowDelete`
/// `Transformation` variants are purely additive tagged-enum cases, so no
/// field reshape is needed — a v2 file (filter/sort only) loads byte-identically
/// except for the bumped version.
///
/// v3 → v4 (P4c) adds the display-only projection variants (Reorder/Rename/
/// DeleteColumn) and changes persistence to the ACTIVE stack only (the P4c
/// history zipper drops the in-stack redo tail). Migration truncates each tab's
/// `transform_stack` to `undo_cursor` (its active slice).
///
/// v4 → v5 (P5a) adds SQL console tabs (`sql_tabs` + `active_sql_tab`) alongside
/// the existing table `tabs`. Purely additive: v4 files default both to empty.
///
/// v5 → v6 (P5b) adds `query_history` + `saved_queries`. Purely additive: v5
/// files default both to empty.
///
/// v6 → v7 (P5c) adds persisted `attachments` (MotherDuck + sqlite). Purely
/// additive: v6 files default to an empty vec.
///
/// v7 → v8 (P6a) adds `ui` (catalog/inspector dock + tree state). Purely
/// additive: v7 files default `ui` to all-collapsed / both-docks-hidden.
pub const SESSION_SCHEMA_VERSION: u32 = 8;

/// A single tab within a scratch session.
///
/// v2 additions: `transform_stack` + `undo_cursor`. Unknown fields from
/// the on-disk file flow through `extra` so forward-incompat is graceful.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    /// The DuckDB table name this tab is viewing.
    pub table_name: String,
    /// The source file path, if this tab was created by registering a file.
    pub source_path: Option<PathBuf>,
    /// Active transformation chain for this tab (v2+). Default is empty.
    #[serde(default)]
    pub transform_stack: Vec<Transformation>,
    /// Undo cursor position into `transform_stack` (v2+). Default is 0.
    #[serde(default)]
    pub undo_cursor: usize,
    /// Catch-all for unknown fields written by future versions; preserved
    /// verbatim through migration round-trips.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single SQL console tab persisted in `session.json` (v5+). The editor
/// buffer text only — the live `InputState` is reconstructed on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlTabState {
    pub id: uuid::Uuid,
    pub title: String,
    pub sql: String,
}

/// Persisted catalog/inspector UI state (v8+, P6a). Additive: a v7 file lacks
/// the enclosing `ui` field, so the whole struct serde-defaults to
/// all-collapsed / both-docks-hidden.
///
/// `catalog_expanded` / `catalog_selection` are forward-looking — the T7
/// catalog dock currently renders flat (no expand/collapse/selection UI), so
/// they persist as empty / `None` for now.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionUiState {
    #[serde(default)]
    pub catalog_panel_visible: bool,
    #[serde(default)]
    pub inspector_panel_visible: bool,
    #[serde(default)]
    pub catalog_expanded: Vec<String>, // expanded node names
    #[serde(default)]
    pub catalog_selection: Option<String>,
}

// ---------------------------------------------------------------------------
// Private persistence shape
// ---------------------------------------------------------------------------

/// Kind of a persisted attachment (v7+, P5c). `#[serde(rename_all = "snake_case")]`
/// makes the on-disk enum tags `md` / `sqlite`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersistedAttachmentKind {
    Md,
    Sqlite { path: String },
}

/// A single persisted attachment (v7+, P5c): an alias bound to an attachment kind
/// (MotherDuck or a sqlite file), re-attached on session recover.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedAttachment {
    pub alias: String,
    pub kind: PersistedAttachmentKind,
}

/// Serialized form of mutable session state written to `session.json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionState {
    /// Schema version. Absent in v1 files (treated as 1 by serde default).
    #[serde(default = "default_schema_version_v1")]
    pub schema_version: u32,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub active_tab: Option<usize>,
    #[serde(default)]
    pub sql_tabs: Vec<SqlTabState>,
    #[serde(default)]
    pub active_sql_tab: Option<usize>,
    #[serde(default)]
    pub query_history: Vec<HistoryEntry>,
    #[serde(default)]
    pub saved_queries: Vec<SavedQuery>,
    #[serde(default)]
    pub attachments: Vec<PersistedAttachment>,
    #[serde(default)]
    pub ui: SessionUiState,
}

fn default_schema_version_v1() -> u32 {
    1
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            tabs: Vec::new(),
            active_tab: None,
            sql_tabs: Vec::new(),
            active_sql_tab: None,
            query_history: Vec::new(),
            saved_queries: Vec::new(),
            attachments: Vec::new(),
            ui: SessionUiState::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// A per-window scratch session. Owns a UUID-named directory, a live DuckDB
/// engine, and a tab list that is persisted to disk on every mutation.
pub struct Session {
    /// Unique ID for this window session (parsed from the scratch directory name).
    pub window_id: Uuid,
    /// Absolute path to the scratch directory: `state_root/scratch/{window_id}/`.
    pub scratch_dir: PathBuf,
    /// Live DuckDB engine bound to the scratch directory.
    pub engine: Arc<DuckDBEngine>,
    tabs: Vec<Tab>,
    active_tab: Option<usize>,
    sql_tabs: Vec<SqlTabState>,
    active_sql_tab: Option<usize>,
    query_history: Vec<HistoryEntry>,
    saved_queries: Vec<SavedQuery>,
    attachments: Vec<PersistedAttachment>,
    ui: SessionUiState,
}

impl Session {
    /// Create a brand-new session under `state_root/scratch/{uuid}/`.
    ///
    /// Caller passes `state_root` (e.g. `$STATE`) by reference; the function
    /// generates the UUID and joins the path internally. Asymmetric with
    /// [`Session::recover`] which receives the full `scratch_dir` because
    /// the caller has already scanned the directory tree.
    ///
    /// Generates a UUID v7, creates the directory, builds + initialises the
    /// DuckDB engine, persists an empty `session.json`, and returns the live
    /// session.
    pub async fn new(state_root: &Path, engine_budget_bytes: u64) -> Result<Self> {
        let window_id = Uuid::now_v7();
        let scratch_dir = state_root.join("scratch").join(window_id.to_string());

        std::fs::create_dir_all(&scratch_dir).with_context(|| {
            format!(
                "session::new: could not create scratch dir {}",
                scratch_dir.display()
            )
        })?;

        let engine = build_engine(&scratch_dir, engine_budget_bytes).await?;

        let sess = Self {
            window_id,
            scratch_dir,
            engine: Arc::new(engine),
            tabs: Vec::new(),
            active_tab: None,
            sql_tabs: Vec::new(),
            active_sql_tab: None,
            query_history: Vec::new(),
            saved_queries: Vec::new(),
            attachments: Vec::new(),
            ui: SessionUiState::default(),
        };
        sess.persist()
            .context("session::new: initial persist failed")?;

        tracing::debug!(window_id = %sess.window_id, "session created");
        Ok(sess)
    }

    /// Recover an existing session from `scratch_dir`.
    ///
    /// Parses the UUID from the directory name, reads `session.json` (falls
    /// back to empty state if the file is missing), builds + initialises the
    /// DuckDB engine, and returns the session.
    pub async fn recover(scratch_dir: PathBuf, engine_budget_bytes: u64) -> Result<Self> {
        let dir_name = scratch_dir
            .file_name()
            .and_then(|n| n.to_str())
            .with_context(|| {
                format!(
                    "session::recover: scratch_dir has no file-name component: {}",
                    scratch_dir.display()
                )
            })?;

        let window_id = Uuid::parse_str(dir_name).with_context(|| {
            format!("session::recover: directory name is not a valid UUID: {dir_name}")
        })?;

        let json_path = scratch_dir.join("session.json");
        let state: SessionState = match migrate::load(&json_path) {
            Ok(state) => state,
            Err(SessionLoadError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    window_id = %window_id,
                    path = %json_path.display(),
                    "session.json missing; using empty default state"
                );
                SessionState::default()
            }
            Err(e) if e.is_forward_incompat() => {
                // Forward-incompat: the session.json was written by a NEWER dat0
                // (a newer top-level schema_version, or a known version carrying
                // an unrecognized transform `kind`). Surface the dedicated
                // forward-incompat Banner (PD-018 / T13 review Important 2) rather
                // than treating it as corruption or silently dropping state, then
                // propagate the error so the caller does NOT proceed to eagerly
                // persist OUR (older) schema over the user's newer file —
                // destroying data the newer binary can still read.
                crate::error_ux::push(forward_incompat_banner(&e));
                return Err(anyhow::Error::from(e))
                    .context("recover: session.json is from a newer dat0 version");
            }
            Err(e) => {
                // Malformed JSON / other I/O errors surface here; caller decides
                // UX. We propagate (no forward-incompat banner — this is genuine
                // corruption, not a version skew).
                return Err(anyhow::Error::from(e))
                    .context("recover: session.json migration failed");
            }
        };

        let engine = build_engine(&scratch_dir, engine_budget_bytes).await?;

        let sess = Self {
            window_id,
            scratch_dir,
            engine: Arc::new(engine),
            tabs: state.tabs,
            active_tab: state.active_tab,
            sql_tabs: state.sql_tabs,
            active_sql_tab: state.active_sql_tab,
            query_history: state.query_history,
            saved_queries: state.saved_queries,
            attachments: state.attachments,
            ui: state.ui,
        };

        // Eagerly persist after recovery. This matches Session::new's pattern and
        // guarantees that a v1 → v2 migration lands on disk on the first open,
        // rather than waiting for the first user-initiated mutation (plan §T8:
        // "one-shot + write-back"). A read-only session that never mutates state
        // would otherwise leave v1 on disk indefinitely, re-running migration on
        // every subsequent open.
        sess.persist()
            .context("recover: post-migration persist failed")?;

        tracing::debug!(window_id = %sess.window_id, tab_count = sess.tabs.len(), "session recovered");
        Ok(sess)
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Return the ordered slice of open tabs.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Return the currently active tab, if any.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab.and_then(|i| self.tabs.get(i))
    }

    /// All persisted SQL console tabs (buffer text + title).
    pub fn sql_tabs(&self) -> &[SqlTabState] {
        &self.sql_tabs
    }

    /// Index of the currently active SQL console tab, if any.
    pub fn active_sql_tab(&self) -> Option<usize> {
        self.active_sql_tab
    }

    // -----------------------------------------------------------------------
    // Mutators
    // -----------------------------------------------------------------------

    /// Append a new tab and make it the active tab, then persist.
    pub fn add_tab(&mut self, tab: Tab) -> Result<()> {
        self.tabs.push(tab);
        self.active_tab = Some(self.tabs.len() - 1);
        self.persist().context("session::add_tab: persist failed")
    }

    /// Set the active tab by index. Returns an error if `index` is out of bounds.
    pub fn set_active(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            bail!(
                "session::set_active: index {index} out of bounds (len={})",
                self.tabs.len()
            );
        }
        self.active_tab = Some(index);
        self.persist()
            .context("session::set_active: persist failed")
    }

    /// Replace the entire SQL-tab set + active index and persist. Called by the
    /// SqlConsole on run / tab add-remove-switch / blur / window close (P5a §4).
    pub fn set_sql_tabs(&mut self, tabs: Vec<SqlTabState>, active: Option<usize>) -> Result<()> {
        self.sql_tabs = tabs;
        self.active_sql_tab = active;
        self.persist()
            .context("session::set_sql_tabs: persist failed")
    }

    /// Replace the query history and persist (P5b).
    pub fn set_query_history(&mut self, history: Vec<HistoryEntry>) -> Result<()> {
        self.query_history = history;
        self.persist()
            .context("session::set_query_history: persist failed")
    }

    /// Read-only access to the persisted history (newest last).
    pub fn query_history(&self) -> &[HistoryEntry] {
        &self.query_history
    }

    /// Replace the saved queries and persist (P5b).
    pub fn set_saved_queries(&mut self, saved: Vec<SavedQuery>) -> Result<()> {
        self.saved_queries = saved;
        self.persist()
            .context("session::set_saved_queries: persist failed")
    }

    /// Read-only access to the saved queries.
    pub fn saved_queries(&self) -> &[SavedQuery] {
        &self.saved_queries
    }

    /// Replace the persisted attachment set and persist (P5c).
    pub fn set_attachments(&mut self, attachments: Vec<PersistedAttachment>) -> Result<()> {
        self.attachments = attachments;
        self.persist()
            .context("session::set_attachments: persist failed")
    }

    /// Read-only access to the persisted attachments.
    pub fn attachments(&self) -> &[PersistedAttachment] {
        &self.attachments
    }

    /// Read-only access to the persisted catalog/inspector UI state (P6a).
    pub fn ui(&self) -> &SessionUiState {
        &self.ui
    }

    /// Replace the persisted catalog/inspector UI state and persist (P6a).
    pub fn set_ui(&mut self, ui: SessionUiState) -> Result<()> {
        self.ui = ui;
        self.persist().context("session::set_ui: persist failed")
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Atomically persist the current tab state to `session.json`.
    ///
    /// Writes to `session.json.tmp` first, syncs the file, then renames to
    /// `session.json`, then syncs the parent directory so the rename metadata
    /// also reaches stable storage (PD-002 sibling fix — same pattern as
    /// `settings.toml` atomic-write). The `.tmp` file is never visible after a
    /// successful call.
    pub fn persist(&self) -> Result<()> {
        let state = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            tabs: self.tabs.clone(),
            active_tab: self.active_tab,
            sql_tabs: self.sql_tabs.clone(),
            active_sql_tab: self.active_sql_tab,
            query_history: self.query_history.clone(),
            saved_queries: self.saved_queries.clone(),
            attachments: self.attachments.clone(),
            ui: self.ui.clone(),
        };

        let bytes =
            serde_json::to_vec_pretty(&state).context("session::persist: serialisation failed")?;

        let json_path = self.scratch_dir.join("session.json");
        let tmp_path = self.scratch_dir.join("session.json.tmp");

        {
            let f = std::fs::File::create(&tmp_path).with_context(|| {
                format!(
                    "session::persist: create tmp file {} failed",
                    tmp_path.display()
                )
            })?;
            let mut bw = std::io::BufWriter::new(f);
            bw.write_all(&bytes)
                .context("session::persist: write to tmp failed")?;
            let f = bw
                .into_inner()
                .context("session::persist: flush BufWriter failed")?;
            f.sync_all()
                .context("session::persist: fsync tmp file failed")?;
        }

        std::fs::rename(&tmp_path, &json_path).with_context(|| {
            format!(
                "session::persist: rename {} -> {} failed",
                tmp_path.display(),
                json_path.display()
            )
        })?;

        // fsync the parent directory so the rename metadata hits disk.
        // PD-002 sibling concern: without this, a power-loss between the rename
        // and any future OS-triggered directory sync could lose the new file.
        let parent_dir = std::fs::File::open(&self.scratch_dir).with_context(|| {
            format!(
                "session::persist: open parent dir {} failed",
                self.scratch_dir.display()
            )
        })?;
        parent_dir
            .sync_all()
            .context("session::persist: fsync parent dir failed")?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Engine construction helper
// ---------------------------------------------------------------------------

/// Build and initialise a `DuckDBEngine` bound to `scratch_dir`.
///
/// The DB file is `scratch_dir/scratch.duckdb` (per spec §3 scratch layout).
/// Only `init()` is async; `DuckDBEngine::new` is synchronous.
async fn build_engine(scratch_dir: &Path, budget_bytes: u64) -> Result<DuckDBEngine> {
    let db_path = scratch_dir.join("scratch.duckdb");
    let budget = MemoryBudget {
        bytes: budget_bytes,
    };

    let engine = DuckDBEngine::new(db_path, budget)
        .context("session::build_engine: DuckDBEngine::new failed")?;

    engine
        .init()
        .await
        .context("session::build_engine: engine.init() failed")?;

    Ok(engine)
}

// ---------------------------------------------------------------------------
// Orphan scratch-dir scan (relaunch recovery)
// ---------------------------------------------------------------------------

/// Scan `$state_root/scratch/*` for dirs not represented in the live set.
/// Returns the orphaned dirs; caller decides whether to recover or discard.
///
/// Non-UUID directory names are ignored (defensive against manual `mkdir`).
/// `.wal` siblings of `scratch.duckdb` are NOT separate orphan dirs because
/// the scan only enumerates directories under `scratch/`.
pub fn scan_orphans(state_root: &Path, live: &[Uuid]) -> Result<Vec<PathBuf>> {
    let scratch_root = state_root.join("scratch");
    if !scratch_root.exists() {
        return Ok(Vec::new());
    }
    let live: std::collections::HashSet<Uuid> = live.iter().copied().collect();
    let mut orphans = Vec::new();
    for entry in std::fs::read_dir(&scratch_root)
        .with_context(|| format!("read_dir {}", scratch_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        let id = match Uuid::parse_str(name_str) {
            Ok(id) => id,
            Err(_) => continue,
        };
        if !live.contains(&id) {
            orphans.push(entry.path());
        }
    }
    Ok(orphans)
}

// ---------------------------------------------------------------------------
// Forward-incompat banner
// ---------------------------------------------------------------------------

/// Build the persistent forward-incompat [`crate::error_ux::Banner`] for a
/// [`SessionLoadError`] that [`SessionLoadError::is_forward_incompat`] flags —
/// i.e. the session.json was written by a newer dat0 version.
///
/// Routed from [`Session::recover`] instead of generic error propagation
/// (T13 review Important 2). The banner is an Error-kind notice with a body
/// distinguishing the two forward-incompat shapes (newer top-level version vs.
/// an unrecognized transform `kind`), so the user understands their session is
/// intact but needs the newer dat0 to open.
fn forward_incompat_banner(err: &SessionLoadError) -> crate::error_ux::Banner {
    let body = match err {
        SessionLoadError::UnsupportedVersion(v) => format!(
            "This session was saved by a newer version of dat0 (schema v{v}). \
             Open it with that version, or discard it to start fresh."
        ),
        SessionLoadError::ForwardIncompatTransform(kind) => format!(
            "This session uses a feature ('{kind}') from a newer version of dat0. \
             Open it with that version, or discard it to start fresh."
        ),
        // Non-forward-incompat variants never reach this helper (the caller
        // gates on `is_forward_incompat`), but keep the match total.
        other => other.to_string(),
    };
    crate::error_ux::Banner::error("Session from a newer dat0 version", body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BUDGET: u64 = 256 * 1024 * 1024;

    #[test]
    fn v6_session_loads_as_v7_with_empty_attachments() {
        // A v6 JSON (no `attachments`) must deserialize with an empty vec.
        let json = r#"{"schema_version":6,"tabs":[],"active_tab":null}"#;
        let state: SessionState = serde_json::from_str(json).unwrap();
        assert!(state.attachments.is_empty());
    }

    #[test]
    fn v7_file_loads_as_v8_with_default_ui() {
        // a v7 SessionState JSON (no `ui` field) must load with default UI state.
        let v7 = serde_json::json!({
            "schema_version": 7, "tabs": [], "attachments": []
        });
        let state: super::SessionState = serde_json::from_value(v7).unwrap();
        assert!(!state.ui.catalog_panel_visible, "default dock hidden");
        assert!(state.ui.catalog_expanded.is_empty());
    }

    #[test]
    fn attachments_round_trip() {
        let state = SessionState {
            attachments: vec![PersistedAttachment {
                alias: "md".into(),
                kind: PersistedAttachmentKind::Md,
            }],
            ..Default::default()
        };
        let s = serde_json::to_string(&state).unwrap();
        let back: SessionState = serde_json::from_str(&s).unwrap();
        assert_eq!(back.attachments.len(), 1);
        assert_eq!(back.schema_version, SESSION_SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn new_session_creates_dir_and_persists_empty_state() {
        let root = tempfile::tempdir().expect("tempdir");
        let sess = Session::new(root.path(), TEST_BUDGET)
            .await
            .expect("Session::new");

        // Directory must exist.
        assert!(sess.scratch_dir.exists(), "scratch dir should exist");

        // session.json must exist.
        let json_path = sess.scratch_dir.join("session.json");
        assert!(json_path.exists(), "session.json should exist");

        // Tabs must be empty; no active tab.
        assert!(sess.tabs().is_empty(), "tabs should be empty");
        assert!(sess.active_tab().is_none(), "active_tab should be None");
    }

    #[tokio::test]
    async fn add_tab_persists_to_session_json() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut sess = Session::new(root.path(), TEST_BUDGET)
            .await
            .expect("Session::new");

        let tab = Tab {
            table_name: "my_table".to_string(),
            source_path: None,
            transform_stack: Vec::new(),
            undo_cursor: 0,
            extra: Default::default(),
        };
        sess.add_tab(tab).expect("add_tab");

        // Read back raw JSON and verify contents.
        let json_path = sess.scratch_dir.join("session.json");
        let raw = std::fs::read_to_string(&json_path).expect("read session.json");
        let state: SessionState = serde_json::from_str(&raw).expect("parse session.json");

        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.tabs[0].table_name, "my_table");
        assert_eq!(state.active_tab, Some(0));
        assert_eq!(state.schema_version, SESSION_SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn recover_reads_tab_state_back() {
        let root = tempfile::tempdir().expect("tempdir");

        let scratch_dir = {
            let mut sess = Session::new(root.path(), TEST_BUDGET)
                .await
                .expect("Session::new");

            let tab = Tab {
                table_name: "recovered_table".to_string(),
                source_path: Some(PathBuf::from("/tmp/data.csv")),
                transform_stack: Vec::new(),
                undo_cursor: 0,
                extra: Default::default(),
            };
            sess.add_tab(tab).expect("add_tab");

            sess.scratch_dir.clone()
            // sess drops here, releasing the engine + DB lock
        };

        let recovered = Session::recover(scratch_dir, TEST_BUDGET)
            .await
            .expect("Session::recover");

        assert_eq!(recovered.tabs().len(), 1);
        assert_eq!(recovered.tabs()[0].table_name, "recovered_table");
        assert_eq!(
            recovered.tabs()[0].source_path,
            Some(PathBuf::from("/tmp/data.csv"))
        );
        assert_eq!(recovered.active_tab, Some(0));
    }

    #[tokio::test]
    async fn query_stores_round_trip_through_persist_and_recover() {
        use queries::{HistoryEntry, SavedQuery};

        let root = tempfile::tempdir().expect("tempdir");

        let scratch_dir = {
            let mut sess = Session::new(root.path(), TEST_BUDGET)
                .await
                .expect("Session::new");

            // Fresh session has empty stores.
            assert!(sess.query_history().is_empty());
            assert!(sess.saved_queries().is_empty());

            sess.set_query_history(vec![HistoryEntry {
                sql: "select 1".into(),
                ran_at: 42,
                ok: true,
                elapsed_ms: 7,
            }])
            .expect("set_query_history");

            let id = Uuid::now_v7();
            sess.set_saved_queries(vec![SavedQuery {
                id,
                name: "Daily".into(),
                sql: "select count(*) from t".into(),
                saved_at: 99,
            }])
            .expect("set_saved_queries");

            sess.scratch_dir.clone()
            // sess drops here, releasing the engine + DB lock
        };

        let recovered = Session::recover(scratch_dir, TEST_BUDGET)
            .await
            .expect("Session::recover");

        assert_eq!(recovered.query_history().len(), 1);
        assert_eq!(recovered.query_history()[0].sql, "select 1");
        assert_eq!(recovered.query_history()[0].ran_at, 42);
        assert_eq!(recovered.saved_queries().len(), 1);
        assert_eq!(recovered.saved_queries()[0].name, "Daily");
        assert_eq!(recovered.saved_queries()[0].sql, "select count(*) from t");
    }

    #[tokio::test]
    async fn ui_round_trips_through_persist_and_recover() {
        // Dock/tree UI state (P6a v8) must survive a restart — set_ui persists it,
        // recover reads it back verbatim (all four fields, incl. the forward-looking
        // catalog_expanded/catalog_selection).
        let root = tempfile::tempdir().expect("tempdir");

        let scratch_dir = {
            let mut sess = Session::new(root.path(), TEST_BUDGET)
                .await
                .expect("Session::new");

            // Fresh session has default (all-hidden / empty) UI state.
            assert_eq!(sess.ui(), &SessionUiState::default());

            sess.set_ui(SessionUiState {
                catalog_panel_visible: true,
                inspector_panel_visible: true,
                catalog_expanded: vec!["orders".into(), "sales".into()],
                catalog_selection: Some("orders".into()),
            })
            .expect("set_ui");

            sess.scratch_dir.clone()
            // sess drops here, releasing the engine + DB lock
        };

        let recovered = Session::recover(scratch_dir, TEST_BUDGET)
            .await
            .expect("Session::recover");

        let ui = recovered.ui();
        assert!(ui.catalog_panel_visible, "catalog dock visibility survived");
        assert!(ui.inspector_panel_visible, "inspector dock visibility survived");
        assert_eq!(ui.catalog_expanded, vec!["orders".to_string(), "sales".to_string()]);
        assert_eq!(ui.catalog_selection.as_deref(), Some("orders"));
    }

    #[tokio::test]
    async fn persist_is_atomic_via_rename() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut sess = Session::new(root.path(), TEST_BUDGET)
            .await
            .expect("Session::new");

        let tab = Tab {
            table_name: "atomic_test".to_string(),
            source_path: None,
            transform_stack: Vec::new(),
            undo_cursor: 0,
            extra: Default::default(),
        };
        sess.add_tab(tab).expect("add_tab");

        // After add_tab (which calls persist), the .tmp file must not exist.
        let tmp_path = sess.scratch_dir.join("session.json.tmp");
        assert!(
            !tmp_path.exists(),
            "session.json.tmp should not exist after successful persist"
        );
    }

    #[tokio::test]
    async fn scan_orphans_finds_dir_not_in_live_set() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let orphan_id = Uuid::now_v7();
        let orphan_dir = tmp.path().join("scratch").join(orphan_id.to_string());
        std::fs::create_dir_all(&orphan_dir).unwrap();
        std::fs::write(orphan_dir.join("session.json"), "{}").unwrap();

        let found = scan_orphans(tmp.path(), &[]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], orphan_dir);

        let found = scan_orphans(tmp.path(), &[orphan_id]).unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn recover_v1_session_json_migrates_to_v2() {
        // Write a raw v1 session.json (no schema_version, no transform fields).
        let root = tempfile::tempdir().expect("tempdir");
        let window_id = Uuid::now_v7();
        let scratch_dir = root.path().join("scratch").join(window_id.to_string());
        std::fs::create_dir_all(&scratch_dir).unwrap();

        let v1_json = r#"{
  "tabs": [{ "table_name": "orders", "source_path": null }],
  "active_tab": 0
}"#;
        std::fs::write(scratch_dir.join("session.json"), v1_json).unwrap();

        let sess = Session::recover(scratch_dir, TEST_BUDGET)
            .await
            .expect("Session::recover with v1 json");

        assert_eq!(sess.tabs().len(), 1);
        assert_eq!(sess.tabs()[0].table_name, "orders");
        assert!(
            sess.tabs()[0].transform_stack.is_empty(),
            "v1 migration should default transform_stack to empty"
        );
        assert_eq!(sess.tabs()[0].undo_cursor, 0);
    }

    /// `forward_incompat_banner` produces an Error-kind banner whose body
    /// distinguishes the two forward-incompat shapes (pure function — no global
    /// state, no parallel-test interference).
    #[test]
    fn forward_incompat_banner_describes_both_shapes() {
        let v = forward_incompat_banner(&SessionLoadError::UnsupportedVersion(999));
        assert_eq!(v.kind, crate::error_ux::BannerKind::Error);
        assert!(
            v.body.contains("999"),
            "version banner body must mention the version: {}",
            v.body
        );

        let k = forward_incompat_banner(&SessionLoadError::ForwardIncompatTransform(
            "frobnicate".into(),
        ));
        assert_eq!(k.kind, crate::error_ux::BannerKind::Error);
        assert!(
            k.body.contains("frobnicate"),
            "transform banner body must mention the kind: {}",
            k.body
        );
    }

    /// `Session::recover` on a forward-incompat (newer-version) session.json
    /// pushes the forward-incompat Banner AND returns an error — it must NOT
    /// silently fall back to default state (which would overwrite the user's
    /// newer file via the eager post-recover persist).
    #[tokio::test]
    async fn recover_forward_incompat_pushes_banner_and_errors() {
        // Drain any banners other tests left pending so our assertion is clean.
        let _ = crate::error_ux::drain_pending();

        let root = tempfile::tempdir().expect("tempdir");
        let window_id = Uuid::now_v7();
        let scratch_dir = root.path().join("scratch").join(window_id.to_string());
        std::fs::create_dir_all(&scratch_dir).unwrap();

        // A session.json written by a hypothetical future dat0 (newer schema).
        let v999_json = r#"{ "schema_version": 999, "tabs": [], "active_tab": null }"#;
        std::fs::write(scratch_dir.join("session.json"), v999_json).unwrap();

        let result = Session::recover(scratch_dir, TEST_BUDGET).await;
        assert!(
            result.is_err(),
            "forward-incompat recover must error (not fall back + overwrite the newer file)"
        );

        // The forward-incompat banner must have been pushed to the pending queue.
        let drained = crate::error_ux::drain_pending();
        assert!(
            drained
                .iter()
                .any(|b| b.kind == crate::error_ux::BannerKind::Error
                    && b.title.contains("newer dat0 version")),
            "a forward-incompat Error banner must be pushed, got: {drained:?}"
        );
    }
}
