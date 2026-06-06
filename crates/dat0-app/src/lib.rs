//! dat0 desktop application library (internal API surface).

pub mod actions;
pub mod app_lock;
pub mod boot;
pub mod catalog;
pub mod command_palette;
pub mod connections;
pub mod empty_state;
pub mod error_ux;
pub mod file_drop;
pub mod grid;
pub mod import_progress;
pub mod import_wizard;
pub mod main_bridge;
pub mod menu_macos;
pub mod platform;
pub mod query;
pub mod recents;
pub mod recovery_panel;
pub mod sample_data;
pub mod session;
pub mod settings;
pub mod settings_ui;
pub mod telemetry;
pub mod theme;
pub mod updater;
pub mod view;
pub mod window;
pub mod window_registry;

pub use window::run_app;
