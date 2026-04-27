//! dat0 desktop application library (internal API surface).

pub mod boot;
pub mod error_ux;
pub mod menu_macos;
pub mod platform;
pub mod recents;
pub mod settings;
pub mod settings_ui;
pub mod telemetry;
pub mod theme;
pub mod updater;
pub mod window;

pub use window::run_app;
