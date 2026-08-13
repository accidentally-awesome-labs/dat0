//! The window's session, as a state rather than a guarantee.
//!
//! `Session::new` opens DuckDB, runs its `PRAGMA`s and applies the migrations,
//! all of it synchronous. Doing that before the first frame put a
//! `block_on` on the UI thread — which `boot.rs` flagged in its own SAFETY
//! comment as a latent nested-runtime abort, because `Handle::block_on` from
//! inside a polled tokio task aborts the process and only convention kept the
//! toolkit from dispatching actions that way.
//!
//! The fix is to open the window first and post the session in when it is
//! built, which means the shell needs a third state the type system used to
//! deny it.
//!
//! Toolkit-free: the two shells project it onto their own chrome — the GPUI
//! build through `view::title_bar::SessionPhase`, the Dioxus build through the
//! titlebar pill and the status bar's engine dot.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::session::Session;

/// The window's session.
///
/// `Failed` carries a rendered message because `Session::new` returns
/// `anyhow::Error`, which has no variant to match on — it is deliberately NOT
/// routed through `error_ux::engine::banner_for`, whose whole contract is an
/// exhaustive match over `EngineError`'s sixteen variants.
pub enum SessionSlot {
    /// DuckDB is opening on the tokio runtime. Terminal only in the sense that
    /// exactly one of the other two states follows it.
    Booting,
    Ready(Arc<Mutex<Session>>),
    /// `format!("{e:#}")` of the `anyhow::Error`. Terminal until the user
    /// retries — deliberately no automatic retry, because a failing
    /// `Session::new` is usually a full disk or a locked state root and a
    /// retry loop would hammer both.
    Failed(String),
}

impl SessionSlot {
    /// The live session, or `None` in either non-`Ready` state.
    pub fn ready(&self) -> Option<&Arc<Mutex<Session>>> {
        match self {
            Self::Ready(s) => Some(s),
            Self::Booting | Self::Failed(_) => None,
        }
    }

    /// The rendered failure message, or `None` when the slot has not failed.
    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(msg) => Some(msg.as_str()),
            Self::Booting | Self::Ready(_) => None,
        }
    }

    /// Whether the session is still opening.
    pub fn is_booting(&self) -> bool {
        matches!(self, Self::Booting)
    }
}
