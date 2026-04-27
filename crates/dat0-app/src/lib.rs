//! dat0 desktop application library (internal API surface).

pub mod boot;
pub mod platform;
pub mod settings;
pub mod theme;
pub mod window;

pub use window::run_app;
