//! `MainThreadDispatcher` — post a `FnOnce(&mut gpui::App) + Send` closure
//! onto the GPUI main thread from any thread.
//!
//! P3a PD-010 closure: `AsyncApp::update` calls `RefCell::borrow_mut` on
//! state owned by the Cocoa main thread; calling it from a tokio worker
//! panics. This module replaces that direct path with a
//! `futures::channel::mpsc::unbounded` channel whose receiver lives in a
//! `cx.spawn` future registered during app init. The sender is `Send` +
//! cloneable; tokio tasks (e.g., the UDS forwarder, the import-cancel
//! handler) post closures into it.
//!
//! See `docs/internal/gpui-api-notes.md` § "Globals + cross-thread
//! dispatch" for the verified `cx.update` / `cx.spawn` shape this module
//! relies on (T0 spike).
//!
//! ## Safety
//!
//! No `unsafe` lives in this module. The test seam (`drain_for_test`) takes
//! a real `&mut gpui::App` supplied by `TestAppContext::single()`
//! (see `docs/internal/gpui-api-notes.md` §0.A.9). Production `consume`
//! runs inside `cx.spawn`, so the `cx.update(|app| ...)` call always
//! borrows the foreground-pinned `AppCell` from the main thread.

use futures::StreamExt;
use futures::channel::mpsc;

/// Re-export alias so dependents don't have to import `gpui::App`
/// directly. Same type — see `docs/internal/gpui-api-notes.md` §0.A.2.
pub type AppProxy = gpui::App;

/// Closure shape: takes the current `App` and mutates state on the main
/// thread. Must be `Send` so tokio tasks can dispatch it.
pub type MainClosure = Box<dyn FnOnce(&mut AppProxy) + Send + 'static>;

/// Sender side. Cheap to clone; safe to send across threads.
#[derive(Clone)]
pub struct MainThreadDispatcher {
    tx: mpsc::UnboundedSender<MainClosure>,
}

/// Receiver side. Lives in a `cx.spawn` future during app init; runs
/// until the last `MainThreadDispatcher` is dropped.
pub struct MainLoop {
    rx: mpsc::UnboundedReceiver<MainClosure>,
}

#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("receiver loop is closed")]
    Closed,
}

impl MainThreadDispatcher {
    /// Create a new dispatcher + matching main-loop receiver.
    pub fn new() -> (Self, MainLoop) {
        let (tx, rx) = mpsc::unbounded();
        (Self { tx }, MainLoop { rx })
    }

    /// Post a closure to run on the main thread.
    ///
    /// `unbounded_send` is sync + non-blocking. Fails only if the receiver
    /// has been dropped (see `docs/internal/gpui-api-notes.md` §0.A.8 for
    /// drop semantics).
    pub fn dispatch<F>(&self, f: F) -> Result<(), DispatchError>
    where
        F: FnOnce(&mut AppProxy) + Send + 'static,
    {
        self.tx
            .unbounded_send(Box::new(f) as MainClosure)
            .map_err(|_| DispatchError::Closed)
    }
}

impl MainLoop {
    /// Synchronous, test-oriented drain. Pops every currently-queued
    /// closure (without awaiting more) and invokes it against the
    /// supplied `&mut gpui::App`. Tests obtain that `App` via
    /// `gpui::TestAppContext::single()` (see
    /// `docs/internal/gpui-api-notes.md` §0.A.9) — the safe alternative
    /// to the unsafe dangling-pointer shim originally suggested in the
    /// plan.
    ///
    /// Production paths use [`MainLoop::consume`]; that path always has a
    /// real `App` reachable via `cx.update` on the foreground executor and
    /// is exercised by `tests/single_instance.rs`. This helper stays
    /// available outside `cfg(test)` so integration tests (which compile
    /// the library without `cfg(test)`) can reach it; the doc-name flags
    /// the intent.
    pub fn drain_for_test(&mut self, app: &mut AppProxy) {
        // `try_recv` is the non-deprecated futures-mpsc surface. It
        // returns `Err(Closed)` after all senders drop and the queue is
        // empty, and `Err(Empty)` when the queue is empty but still open.
        // Either error variant stops the drain; only `Ok(f)` invokes.
        while let Ok(f) = self.rx.try_recv() {
            f(app);
        }
    }

    /// Consume the receiver inside a GPUI `cx.spawn` future. Each
    /// received closure runs on the main thread via `cx.update`.
    ///
    /// Call shape (verified at T0 spike — see
    /// `docs/internal/gpui-api-notes.md` §0.A.6):
    ///
    /// ```ignore
    /// cx.spawn(async move |cx| {
    ///     main_loop.consume(cx).await
    /// }).detach();
    /// ```
    pub async fn consume(mut self, cx: &mut gpui::AsyncApp) -> Result<(), anyhow::Error> {
        while let Some(f) = self.rx.next().await {
            cx.update(|app| f(app))?;
        }
        Ok(())
    }
}
