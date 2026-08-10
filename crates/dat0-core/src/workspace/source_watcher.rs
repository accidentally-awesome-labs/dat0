//! Per-window file watcher over a single imported table's source file (P7c).
//!
//! Mirrors `settings/watcher.rs`: owns a `notify::RecommendedWatcher` whose
//! `Drop` stops watching. Adds a debounce so a burst of editor saves coalesces
//! into one `on_change` call. The callback must NOT touch GPUI state directly —
//! it runs on a `notify` background thread; the caller bridges to the main
//! thread via `window_registry::dispatcher()` (see window.rs, T5).

use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

pub struct SourceWatcher {
    _watcher: RecommendedWatcher,
}

impl SourceWatcher {
    /// Watch `path`; call `on_change(path)` at most once per `debounce` quiet
    /// window after the last `Modify`/`Create` event.
    pub fn start<F>(path: PathBuf, debounce: Duration, on_change: F) -> Result<Self>
    where
        F: Fn(PathBuf) + Send + 'static,
    {
        let (evt_tx, evt_rx) = mpsc::channel::<()>();
        let watch_path = path.clone();

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                Ok(event) => {
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        // Err only if the debounce thread already exited; nothing to signal.
                        let _ = evt_tx.send(());
                    }
                }
                Err(e) => tracing::warn!(?e, "source watcher error"),
            })?;
        watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

        // Debounce thread: collect events, fire `on_change` after a quiet window.
        // Dropping `SourceWatcher` drops `_watcher`, which drops `evt_tx`; the
        // `recv()`/`recv_timeout()` below then return `Err` and the thread
        // exits — so there is no leaked thread (single stop mechanism: the
        // channel disconnect on Drop).
        std::thread::Builder::new()
            .name("dat0-source-debounce".into())
            .spawn(move || {
                loop {
                    // Block for the first event (or watcher drop).
                    match evt_rx.recv() {
                        Ok(()) => {}
                        Err(_) => return, // watcher dropped
                    }
                    // Drain the quiet window: keep resetting while events arrive.
                    loop {
                        match evt_rx.recv_timeout(debounce) {
                            Ok(()) => continue,                            // more activity → keep waiting
                            Err(mpsc::RecvTimeoutError::Timeout) => break, // quiet → fire
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    on_change(path.clone());
                }
            })?;

        Ok(Self { _watcher: watcher })
    }
}
