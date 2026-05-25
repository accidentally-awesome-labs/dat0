//! In-process registry of live windows + scaffolded WorkspaceMutex API.
//!
//! The registry tracks `WindowHandle`s so the AppLock UDS callback knows
//! how many windows are open (used by Cmd-N spawn, last-window-closes
//! shutdown, and orphan-recovery banner dispatch). The WorkspaceMutex
//! map is scaffolded here per P2 retro #1 but not exercised until P4
//! SQL Console / Workspace mode lands.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::main_bridge::MainThreadDispatcher;

/// Process-wide dispatcher slot. Set exactly once, before `Application::run`
/// (see `main.rs`). Used by the UDS handler and any tokio task that needs to
/// post closures onto the GPUI main thread. Closes PD-010.
static DISPATCHER: OnceCell<MainThreadDispatcher> = OnceCell::new();

/// Install the dispatcher for process-wide access. Idempotent: a second
/// call is a no-op (subsequent attempts simply drop the new dispatcher).
pub fn install_dispatcher(d: MainThreadDispatcher) {
    let _ = DISPATCHER.set(d);
}

/// Access the installed dispatcher. Returns `None` only if
/// [`install_dispatcher`] has not yet been called (e.g., very early during
/// boot, or in tests that exercise sub-modules in isolation).
pub fn dispatcher() -> Option<&'static MainThreadDispatcher> {
    DISPATCHER.get()
}

#[derive(Debug, Clone)]
pub struct WindowHandle {
    pub window_id: Uuid,
}

pub struct WindowRegistry {
    windows: Vec<WindowHandle>,
    workspace_mutex: HashMap<PathBuf, Arc<Mutex<()>>>,
}

impl WindowRegistry {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            workspace_mutex: HashMap::new(),
        }
    }

    pub fn register(&mut self, handle: WindowHandle) {
        self.windows.push(handle);
    }

    pub fn unregister(&mut self, window_id: Uuid) {
        self.windows.retain(|w| w.window_id != window_id);
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub fn live_windows(&self) -> impl Iterator<Item = &WindowHandle> {
        self.windows.iter()
    }

    /// P4 SCAFFOLD — returns a per-workspace-path mutex. Same path returns
    /// the same `Arc<Mutex<()>>`; concurrent workspace opens serialize on it.
    /// P3a does not call this; tests prove correctness in advance.
    pub fn workspace_mutex(&mut self, canonical_path: &std::path::Path) -> Arc<Mutex<()>> {
        self.workspace_mutex
            .entry(canonical_path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

impl Default for WindowRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_unregister_round_trip() {
        let mut reg = WindowRegistry::new();
        assert!(reg.is_empty());
        let h = WindowHandle {
            window_id: Uuid::now_v7(),
        };
        let id = h.window_id;
        reg.register(h);
        assert_eq!(reg.len(), 1);
        reg.unregister(id);
        assert!(reg.is_empty());
    }

    #[test]
    fn workspace_mutex_same_path_returns_same_arc() {
        let mut reg = WindowRegistry::new();
        let p = std::path::Path::new("/tmp/dat0/workspace-a");
        let a = reg.workspace_mutex(p);
        let b = reg.workspace_mutex(p);
        assert!(Arc::ptr_eq(&a, &b), "same path must reuse the mutex");
    }

    #[test]
    fn workspace_mutex_distinct_paths_are_independent() {
        let mut reg = WindowRegistry::new();
        let a = reg.workspace_mutex(std::path::Path::new("/tmp/dat0/ws-a"));
        let b = reg.workspace_mutex(std::path::Path::new("/tmp/dat0/ws-b"));
        assert!(!Arc::ptr_eq(&a, &b));
    }
}
