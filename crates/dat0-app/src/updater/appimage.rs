//! AppImageUpdate scaffolding for Linux self-updating AppImages.
//!
//! v1: stub — real subprocess invocation to `appimageupdatetool` lands in P10.

use super::Updater;
use anyhow::Result;

pub struct AppImageUpdater {
    version: String,
}

impl AppImageUpdater {
    pub fn new() -> Result<Self> {
        Ok(Self {
            version: env!("CARGO_PKG_VERSION").into(),
        })
    }
}

impl Updater for AppImageUpdater {
    fn check_for_updates(&self) -> Result<()> {
        tracing::info!(version = %self.version, "appimage: check_for_updates (stub)");
        Ok(())
    }

    fn current_version(&self) -> &str {
        &self.version
    }
}
