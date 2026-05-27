//! Import progress + cancel state. One active import per app (P3b
//! simplification); cancel sets the flag + calls engine.interrupt
//! (deferred to D-008 — placeholder logged today).
//!
//! Replaces the T3 inline stub. `actions::builtin::ids::IMPORT_CANCEL`
//! dispatches into [`cancel_active`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

#[derive(Clone)]
pub struct ImportProgress {
    pub cancel: Arc<AtomicBool>,
    pub bytes_done: Arc<AtomicU64>,
    pub total_bytes: u64,
    /// Placeholder DuckDB statement handle (D-008). Boxed via `u64` so
    /// this module doesn't depend on engine internals.
    pub handle: Arc<Mutex<Option<u64>>>,
}

impl ImportProgress {
    pub fn new(total_bytes: u64) -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            bytes_done: Arc::new(AtomicU64::new(0)),
            total_bytes,
            handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn update(&self, done: u64) {
        self.bytes_done.store(done, Ordering::SeqCst);
    }

    pub fn bytes_done(&self) -> u64 {
        self.bytes_done.load(Ordering::SeqCst)
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

static ACTIVE: Lazy<Mutex<Option<ImportProgress>>> = Lazy::new(|| Mutex::new(None));

pub fn set_active(p: ImportProgress) {
    *ACTIVE.lock() = Some(p);
}

pub fn clear_active() {
    *ACTIVE.lock() = None;
}

pub fn active() -> Option<ImportProgress> {
    ACTIVE.lock().clone()
}

/// Sets cancel flag + (when D-008 lands) calls engine.interrupt(handle).
pub fn request_cancel(cancel: &Arc<AtomicBool>) {
    cancel.store(true, Ordering::SeqCst);
}

/// Cancel the currently active import (called via `IMPORT_CANCEL` action).
pub fn cancel_active(_app: &mut gpui::App) {
    if let Some(p) = active() {
        request_cancel(&p.cancel);
        if let Some(_h) = *p.handle.lock() {
            tracing::info!("import_progress::cancel_active — engine.interrupt requested (D-008)");
        }
        crate::error_ux::push(crate::error_ux::Banner::warning("Import cancelled"));
        clear_active();
    }
}

/// Test seam: cancel the currently active import without needing a
/// `&mut gpui::App`. Follows the T1 `MainLoop::drain_for_test` precedent
/// (unconditional `pub` with docstring marker). Integration tests in
/// `tests/` build the library without the `test` cfg flag and so cannot
/// see `#[cfg(test)]`-gated items — exposing this unconditionally is the
/// only way to keep the seam callable from `tests/import_cancel.rs`.
pub fn cancel_active_for_test() {
    if let Some(p) = active() {
        request_cancel(&p.cancel);
        clear_active();
    }
}
