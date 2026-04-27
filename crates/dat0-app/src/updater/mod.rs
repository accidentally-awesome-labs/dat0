//! Auto-update bridge.
//!
//! Platform-specific updaters: Sparkle (macOS) and AppImageUpdate (Linux).
//! v1 in P1 is scaffolding only — full UI / subprocess invocation lands in P10.

use anyhow::Result;

#[cfg(target_os = "macos")]
mod sparkle;
#[cfg(target_os = "macos")]
pub use sparkle::*;

#[cfg(target_os = "linux")]
mod appimage;
#[cfg(target_os = "linux")]
pub use appimage::*;

pub trait Updater {
    fn check_for_updates(&self) -> Result<()>;
    fn current_version(&self) -> &str;
}
