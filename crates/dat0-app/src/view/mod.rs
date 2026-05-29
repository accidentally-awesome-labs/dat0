//! Per-tab view state: active Transformation stack + undo cursor + active view name.

pub mod model;

pub use model::{HISTORY_CAP, ViewChange, ViewModel};
