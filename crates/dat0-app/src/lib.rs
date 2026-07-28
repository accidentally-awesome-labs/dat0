//! dat0 desktop application library (internal API surface).

// UAT Gap 2: re-export the i18n crate so the behavioral test harness can name
// the exact rendered label (`dat0_app::dat0_i18n::t("hero.take_tour")`) it asks
// the a11y tree for — keeping test assertions and render code on one source.
pub use dat0_i18n;

pub mod a11y;
pub mod about;
pub mod actions;
pub mod ai;
pub mod app_lock;
pub mod assets;
pub mod boot;
pub mod catalog;
pub mod charts;
pub mod cli;
pub mod command_palette;
pub mod connections;
pub mod empty_state;
pub mod error_ux;
pub mod file_drop;
/// Dev-only token gallery (UI redesign A4). Never compiled into the shipped
/// binary — the `gallery` feature is only enabled by the self-dev-dependency.
#[cfg(feature = "gallery")]
pub mod gallery;
pub mod grid;
pub mod import_progress;
pub mod import_wizard;
pub mod inspector;
pub mod live_refresh_dialog;
pub mod main_bridge;
pub mod menu_macos;
pub mod onboarding;
pub mod package;
pub mod platform;
pub mod query;
pub mod recents;
pub mod recovery_panel;
pub mod recovery_scan;
pub mod sample_data;
pub mod session;
pub mod settings;
pub mod settings_ui;
pub mod telemetry;
pub mod theme;
pub mod update;
pub mod view;
pub mod window;
pub mod window_registry;
pub mod workspace;
pub mod workspace_in_use_modal;

pub use window::run_app;
