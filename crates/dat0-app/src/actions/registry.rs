//! Central registry of dispatchable actions.
//!
//! [`ActionRegistry`] is an `Arc<RwLock<HashMap<ActionId, ActionDescriptor>>>`
//! cloneable handle: the boot path constructs one, hands it to
//! [`super::builtin::register_all`], then publishes it through
//! [`crate::window_registry::install_action_registry`] so every window /
//! subsystem can look up an action by stable id.
//!
//! Dispatch closures are `Fn(&mut gpui::App) + Send + Sync + 'static`.
//! They are invoked on the GPUI main thread. Closures that capture
//! non-`Send` state must route through
//! [`crate::main_bridge::MainThreadDispatcher`] (T1) — `&mut gpui::App`
//! is only ever observed on the main thread, so the bound is honored
//! by construction.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use thiserror::Error;

/// Stable id for an action. Used by the palette + Banner action lookups.
///
/// Constructed via [`ActionId::from`] (free function on the type, not the
/// `From` trait — keeps the call-site terse without forcing a generic
/// type parameter on every use).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionId(String);

impl ActionId {
    pub fn from(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Grouping bucket displayed in the command palette (T6) and used by
/// settings / menu wiring to order related actions together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionGroup {
    Navigation,
    Theme,
    File,
    Settings,
    Recovery,
    Import,
    Edit,
}

/// Closure type for action dispatch. Must be `Send + Sync` because the
/// registry is `Arc<RwLock<...>>`; actions that capture non-`Send` state
/// route through [`crate::main_bridge::MainThreadDispatcher`] (T1) inside
/// the closure.
pub type DispatchFn = Arc<dyn Fn(&mut gpui::App) + Send + Sync + 'static>;

#[derive(Clone)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub title: String,
    pub group: ActionGroup,
    pub keybinding: Option<gpui::Keystroke>,
    pub dispatch: DispatchFn,
}

impl std::fmt::Debug for ActionDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionDescriptor")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("group", &self.group)
            .field("keybinding", &self.keybinding)
            .field("dispatch", &"<DispatchFn>")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum RegisterError {
    #[error("duplicate action id: {0}")]
    DuplicateId(String),
}

#[derive(Clone, Default)]
pub struct ActionRegistry {
    inner: Arc<RwLock<HashMap<ActionId, ActionDescriptor>>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, desc: ActionDescriptor) -> Result<(), RegisterError> {
        let mut w = self.inner.write();
        if w.contains_key(&desc.id) {
            return Err(RegisterError::DuplicateId(desc.id.to_string()));
        }
        w.insert(desc.id.clone(), desc);
        Ok(())
    }

    pub fn get(&self, id: &ActionId) -> Option<ActionDescriptor> {
        self.inner.read().get(id).cloned()
    }

    /// Snapshot iterator over all registered descriptors. Returns an owned
    /// `Vec`-backed iterator so the registry lock isn't held for the
    /// duration of the consumer (which may be the palette renderer doing
    /// fuzzy match + UI work).
    pub fn iter(&self) -> impl Iterator<Item = ActionDescriptor> {
        let snapshot: Vec<ActionDescriptor> = self.inner.read().values().cloned().collect();
        snapshot.into_iter()
    }

    pub fn count(&self) -> usize {
        self.inner.read().len()
    }

    /// Return `true` if an action with the given string id has been registered.
    pub fn contains(&self, id: &str) -> bool {
        self.inner.read().contains_key(&ActionId(id.to_string()))
    }
}
