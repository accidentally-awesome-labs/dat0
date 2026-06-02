//! SQL console query model + statement utilities (P5a).
pub mod statement;

use std::sync::Weak;
use uuid::Uuid;

use dat0_engine::DuckDBEngine;

/// Where a run's result renders (P5a §0 decision 2). Not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultTarget {
    #[default]
    MainGrid,
    Pane,
}

/// Non-view, persistable metadata for one SQL console tab. The editor buffer
/// lives in a gpui-component `InputState` entity held alongside this in the
/// `SqlConsole` view; this struct is what `QueryModel`/session reason about.
#[derive(Debug, Clone)]
pub struct SqlTabMeta {
    pub id: Uuid,
    pub title: String,
    pub result_target: ResultTarget,
    /// The TEMP VIEW name bound for this tab's last result, if any.
    pub last_result_view: Option<String>,
}

impl SqlTabMeta {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            title: title.into(),
            result_target: ResultTarget::MainGrid,
            last_result_view: None,
        }
    }
}

/// Stable per-tab TEMP VIEW name. `win` is a short window discriminator; `tab`
/// is the tab index. Reused across runs (create-or-replace overwrites).
pub fn result_view_name(win: &str, tab: usize) -> String {
    format!("__dat0_qr_{win}_{tab}")
}

/// Execution lifecycle for the active run.
pub enum ExecState {
    Idle,
    Running {
        started_at: std::time::Instant,
        cancel: QueryCancel,
    },
    Cancelled,
    Error(String),
}

/// Token-free cancellation (P5a §0 decision 3 / §3). Dropping an armed guard,
/// or calling `cancel()`, fires the engine's connection-wide `interrupt()`.
/// Normal completion calls `disarm()` first. Safe because the per-window engine
/// serializes on one Mutex'd connection — `interrupt()` hits the running query.
pub struct QueryCancel {
    engine: Weak<DuckDBEngine>,
    armed: bool,
}

impl QueryCancel {
    pub fn new(engine: &std::sync::Arc<DuckDBEngine>) -> Self {
        Self {
            engine: std::sync::Arc::downgrade(engine),
            armed: true,
        }
    }
    /// Disarm so a subsequent drop does NOT interrupt (call on normal completion).
    pub fn disarm(&mut self) {
        self.armed = false;
    }
    /// Explicitly interrupt now (Cmd+. / Cancel button) and disarm.
    pub fn cancel(&mut self) {
        if self.armed {
            if let Some(e) = self.engine.upgrade() {
                e.interrupt();
            }
            self.armed = false;
        }
    }
}

impl Drop for QueryCancel {
    fn drop(&mut self) {
        if self.armed {
            if let Some(e) = self.engine.upgrade() {
                e.interrupt();
            }
        }
    }
}
