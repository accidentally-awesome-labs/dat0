//! dat0's user interface, on Dioxus.
//!
//! Everything that is not a widget lives in [`dat0_core`]; this crate is the
//! shell, the components, and the platform glue that gives them a window.
//!
//! ## Shape
//!
//! - [`protocol`] serves every web asset out of the binary over the `dat0`
//!   custom scheme, so a bundled `.app`/`.AppImage` needs no files beside it.
//! - [`theme`] holds the active token set as a signal; `app.css` reads it
//!   through `var(--d0-…)`.
//! - [`a11y`] is the ARIA vocabulary and the stable `data-a11y-id` handles the
//!   tests query by. Unlike the GPUI build, these ship in release.

pub mod a11y;
pub mod clipboard;
pub mod components;
pub mod files;
#[cfg(feature = "gallery")]
pub mod gallery;
pub mod keys;
pub mod launch;
pub mod menu;
#[cfg(feature = "perf-harness")]
pub mod perf;
pub mod protocol;
pub mod router;
pub mod session_boot;
pub mod state;
pub mod theme;
#[cfg(feature = "visual")]
pub mod visual;
