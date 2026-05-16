//! Per-window scratch session: a UUID-named directory under `state_root/scratch/`,
//! a DuckDB engine bound to that directory, and a tab list that is persisted
//! atomically to `session.json` on every mutation. Recovery reads an existing
//! scratch dir back into a live `Session` struct, using default state when
//! `session.json` is absent (e.g. after a crash or mid-write interruption).
//! Atomic-rename persistence is the contract: `.tmp` files must never be present
//! after a successful `persist()`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// A single tab within a scratch session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    /// The DuckDB table name this tab is viewing.
    pub table_name: String,
    /// The source file path, if this tab was created by registering a file.
    pub source_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Private persistence shape
// ---------------------------------------------------------------------------

/// Serialized form of mutable session state written to `session.json`.
#[derive(Debug, Serialize, Deserialize, Default)]
struct SessionState {
    tabs: Vec<Tab>,
    active_tab: Option<usize>,
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
}

impl Session {
    /// Create a brand-new session under `state_root/scratch/{uuid}/`.
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

        let state: SessionState = {
            let json_path = scratch_dir.join("session.json");
            if json_path.exists() {
                let bytes = std::fs::read(&json_path).with_context(|| {
                    format!("session::recover: could not read {}", json_path.display())
                })?;
                serde_json::from_slice(&bytes).with_context(|| {
                    format!(
                        "session::recover: malformed session.json at {}",
                        json_path.display()
                    )
                })?
            } else {
                tracing::warn!(
                    window_id = %window_id,
                    path = %json_path.display(),
                    "session.json missing; using empty default state"
                );
                SessionState::default()
            }
        };

        let engine = build_engine(&scratch_dir, engine_budget_bytes).await?;

        let sess = Self {
            window_id,
            scratch_dir,
            engine: Arc::new(engine),
            tabs: state.tabs,
            active_tab: state.active_tab,
        };

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

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Atomically persist the current tab state to `session.json`.
    ///
    /// Writes to `session.json.tmp` first, then renames to `session.json`.
    /// The `.tmp` file is never visible after a successful call.
    pub fn persist(&self) -> Result<()> {
        let state = SessionState {
            tabs: self.tabs.clone(),
            active_tab: self.active_tab,
        };

        let bytes =
            serde_json::to_vec_pretty(&state).context("session::persist: serialisation failed")?;

        let json_path = self.scratch_dir.join("session.json");
        let tmp_path = self.scratch_dir.join("session.json.tmp");

        std::fs::write(&tmp_path, &bytes).with_context(|| {
            format!(
                "session::persist: write to tmp file {} failed",
                tmp_path.display()
            )
        })?;

        std::fs::rename(&tmp_path, &json_path).with_context(|| {
            format!(
                "session::persist: rename {} -> {} failed",
                tmp_path.display(),
                json_path.display()
            )
        })?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Engine construction helper
// ---------------------------------------------------------------------------

/// Build and initialise a `DuckDBEngine` bound to `scratch_dir`.
///
/// The DB file is `scratch_dir/session.duckdb`. Only `init()` is async;
/// `DuckDBEngine::new` is synchronous.
async fn build_engine(scratch_dir: &Path, budget_bytes: u64) -> Result<DuckDBEngine> {
    let db_path = scratch_dir.join("session.duckdb");
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BUDGET: u64 = 256 * 1024 * 1024;

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
        };
        sess.add_tab(tab).expect("add_tab");

        // Read back raw JSON and verify contents.
        let json_path = sess.scratch_dir.join("session.json");
        let raw = std::fs::read_to_string(&json_path).expect("read session.json");
        let state: SessionState = serde_json::from_str(&raw).expect("parse session.json");

        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.tabs[0].table_name, "my_table");
        assert_eq!(state.active_tab, Some(0));
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
            };
            sess.add_tab(tab).expect("add_tab");

            sess.scratch_dir.clone()
            // sess drops here, releasing the engine + DB lock
        };

        // Give the engine a moment to release the lock before recovering.
        tokio::task::yield_now().await;

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
    async fn persist_is_atomic_via_rename() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut sess = Session::new(root.path(), TEST_BUDGET)
            .await
            .expect("Session::new");

        let tab = Tab {
            table_name: "atomic_test".to_string(),
            source_path: None,
        };
        sess.add_tab(tab).expect("add_tab");

        // After add_tab (which calls persist), the .tmp file must not exist.
        let tmp_path = sess.scratch_dir.join("session.json.tmp");
        assert!(
            !tmp_path.exists(),
            "session.json.tmp should not exist after successful persist"
        );
    }
}
