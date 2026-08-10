//! Central registry of dispatchable actions.
//!
//! [`ActionRegistry`] is an `Arc<RwLock<HashMap<ActionId, ActionDescriptor>>>`
//! cloneable handle: the boot path constructs one, hands it to
//! [`super::builtin::register_all`], then publishes it so every window and
//! subsystem can look up an action by stable id.
//!
//! Dispatch closures take [`AppEvents`] — the toolkit-free event bus — not a
//! renderer context. An action therefore *names* what should happen; the shell
//! decides how, and in which window. That is what lets the registry, the
//! command palette and the native menu bar all be renderer-free.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use thiserror::Error;

use crate::events::AppEvents;

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

/// Closure type for action dispatch. `Send + Sync` because the registry is an
/// `Arc<RwLock<...>>` read from any thread.
pub type DispatchFn = Arc<dyn Fn(&AppEvents) + Send + Sync + 'static>;

/// A registry-visible command.
///
/// There is deliberately no `keybinding` field: the one that used to live here
/// was hand-typed next to the `cx.bind_keys` calls rather than read from them,
/// so a palette hint could silently disagree with the live chord.
/// The keymap table the bindings are installed from is the single source for
/// chord hints, and `tests/keymap.rs` fails when an id is neither bound nor
/// listed as deliberately chord-less.
#[derive(Clone)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub title: String,
    pub group: ActionGroup,
    pub dispatch: DispatchFn,
}

impl std::fmt::Debug for ActionDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionDescriptor")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("group", &self.group)
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

    /// Fire the action with this id, if it is registered.
    ///
    /// Returns `false` for an unknown id rather than panicking: ids arrive from
    /// banner payloads and native menu events, neither of which is checked at
    /// compile time.
    pub fn dispatch(&self, id: &str, events: &AppEvents) -> bool {
        match self.get(&ActionId::from(id)) {
            Some(desc) => {
                (desc.dispatch)(events);
                true
            }
            None => {
                tracing::warn!(action_id = %id, "dispatch: action not registered");
                false
            }
        }
    }
}
