//! SQL console query model + statement utilities.
//!
//! Splitting a buffer into statements, naming a result view, and the per-tab
//! metadata the console keeps are all editor-independent. Only the completion
//! *provider* and syntax highlighting were toolkit-bound, and from Phase 4
//! CodeMirror owns both.

pub mod completion;
pub mod statement;

use std::sync::Weak;
use uuid::Uuid;

use dat0_engine::{DuckDBEngine, QueryToken};

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

/// Cancellation drop-guard for one console run (P5a §3, scoped by EN2).
///
/// Dropping an armed guard, or calling `cancel()`, fires
/// [`DuckDBEngine::interrupt_scoped`] for the token this run was armed with —
/// so a Cmd+. lands on *this* query and nothing else. Before EN2 the guard
/// fired the connection-wide `interrupt()`, which meant a console cancel also
/// aborted whatever grid prefetch happened to be in flight on the same window.
///
/// Normal completion calls `disarm()`, which retires the token instead of
/// interrupting it. A token whose query already finished is stale, and a stale
/// scoped interrupt is a silent no-op — so a late cancel cannot hit the next
/// query to start.
pub struct QueryCancel {
    engine: Weak<DuckDBEngine>,
    token: QueryToken,
    armed: bool,
}

impl QueryCancel {
    /// Arm a guard for `token`, which the caller minted with
    /// `engine.begin_query(QueryLane::Console)`.
    pub fn new(engine: &std::sync::Arc<DuckDBEngine>, token: QueryToken) -> Self {
        Self {
            engine: std::sync::Arc::downgrade(engine),
            token,
            armed: true,
        }
    }
    /// Disarm so a subsequent drop does NOT interrupt (call on normal
    /// completion). Also retires the token from the engine's in-flight slot:
    /// leaving it there would let a later lane-scoped supersede fire an
    /// interrupt at a query that has already returned.
    pub fn disarm(&mut self) {
        self.armed = false;
        if let Some(e) = self.engine.upgrade() {
            e.end_query(self.token);
        }
    }
    /// Explicitly interrupt now (Cmd+. / Cancel button) and disarm. The token
    /// stays in flight: the interrupted run still routes through
    /// `finish_sql_run`, which calls `disarm()` and retires it there.
    pub fn cancel(&mut self) {
        if self.armed {
            if let Some(e) = self.engine.upgrade() {
                e.interrupt_scoped(self.token);
            }
            self.armed = false;
        }
    }
}

impl Drop for QueryCancel {
    fn drop(&mut self) {
        if self.armed {
            if let Some(e) = self.engine.upgrade() {
                e.interrupt_scoped(self.token);
                e.end_query(self.token);
            }
        }
    }
}
