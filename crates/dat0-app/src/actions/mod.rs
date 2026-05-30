//! Action discovery + dispatch — central `ActionRegistry` consumed by
//! the command palette (T6) and resolved by Banner action ids (T2).
//!
//! [`builtin::register_all`] is invoked once at app boot (see
//! `crates/dat0-app/src/main.rs`); the resulting registry is parked in
//! [`crate::window_registry::action_registry`] so any subsystem
//! (palette, banner, menu) can look up an action by stable id.

pub mod builtin;
pub mod registry;
pub mod view_actions;

pub use registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry, RegisterError};
