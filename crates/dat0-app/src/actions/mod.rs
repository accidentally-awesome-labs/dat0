//! Action discovery + dispatch — central `ActionRegistry` consumed by
//! the command palette (T6) and resolved by Banner action ids (T2).
//!
//! [`builtin::register_all`] is invoked once at app boot (see
//! `crates/dat0-app/src/main.rs`); the resulting registry is parked in
//! [`crate::window_registry::action_registry`] so any subsystem
//! (palette, banner, menu) can look up an action by stable id.

pub mod builtin;
pub mod edit_actions;
pub mod registry;
pub mod view_actions;

pub use registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry, RegisterError};

/// Construct a fully-populated [`ActionRegistry`] (all built-ins registered)
/// for use in integration tests. Boot calls `builtin::register_all` directly;
/// this wrapper adds the ergonomic `.expect` so test boilerplate is minimal.
pub fn test_registry() -> ActionRegistry {
    let reg = ActionRegistry::new();
    builtin::register_all(&reg).expect("built-in registration must not fail in tests");
    reg
}
