//! dat0 core: everything in the application that is not a widget.
//!
//! This crate is toolkit-free by construction — no `gpui`, no `dioxus` — and a
//! CI gate (`cargo tree -p dat0-core -e normal`) keeps it that way. It is what
//! made the GPUI -> Dioxus migration a UI rewrite rather than an application
//! rewrite.
//!
//! The rough shape:
//!
//! - **Data & engine** — `grid`, `view`, `query`, `charts`, `inspector`,
//!   `catalog`, `connections`, `ai`.
//! - **Persistence** — `session`, `settings`, `workspace`, `package`,
//!   `recents`.
//! - **Process & platform** — `boot`, `cli`, `app_lock`, `platform`,
//!   `globals`, `time`, `update`, `telemetry`, `recovery_scan`.
//! - **UI-facing contracts a renderer implements** — `actions`,
//!   `command_palette`, `events`, `error_ux`, `theme::contrast`, `perf`.

// Re-exported so behavioural tests can name the exact rendered label
// (`dat0_core::dat0_i18n::t("hero.take_tour")`) the UI asked for, keeping
// assertions and render code on one source.
pub use dat0_i18n;

pub mod about;
pub mod actions;
pub mod ai;
pub mod app_lock;
pub mod boot;
pub mod catalog;
pub mod charts;
pub mod cli;
pub mod command_palette;
pub mod connections;
pub mod error_ux;
pub mod events;
pub mod file_drop;
pub mod globals;
pub mod grid;
pub mod import_progress;
pub mod import_wizard;
pub mod inspector;
pub mod keymap;
pub mod onboarding;
pub mod package;
pub mod perf;
pub mod platform;
pub mod query;
pub mod recents;
pub mod recovery_scan;
pub mod sample_data;
pub mod session;
pub mod settings;
pub mod telemetry;
pub mod theme;
pub mod time;
pub mod update;
pub mod view;
pub mod workspace;
