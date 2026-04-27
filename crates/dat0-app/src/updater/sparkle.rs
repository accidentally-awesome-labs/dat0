//! Sparkle bridge for macOS auto-update.
//!
//! v1 scaffolding only: configuration object + check_for_updates() trigger.
//! Full UI (release notes, restart prompt) lands in P10, along with the
//! Objective-C bridge (objc / cocoa) that wraps `SUUpdater`.

use super::Updater;
use anyhow::Result;

pub struct SparkleUpdater {
    appcast_url: String,
    /// EdDSA public key used by Sparkle to verify update signatures.
    /// Stored now (read from `assets/sparkle-public-key.txt` at compile time);
    /// consumed by the Objective-C bridge in P10.
    #[allow(dead_code)]
    public_key: String,
    version: String,
}

impl SparkleUpdater {
    pub fn new() -> Result<Self> {
        Ok(Self {
            appcast_url: env!("DAT0_SPARKLE_APPCAST_URL").into(),
            public_key: include_str!("../../assets/sparkle-public-key.txt")
                .trim()
                .into(),
            version: env!("CARGO_PKG_VERSION").into(),
        })
    }
}

impl Updater for SparkleUpdater {
    fn check_for_updates(&self) -> Result<()> {
        tracing::info!(
            appcast = %self.appcast_url,
            version = %self.version,
            "sparkle: smoke GET"
        );
        let response = ureq::get(&self.appcast_url)
            .timeout(std::time::Duration::from_secs(10))
            .call();
        match response {
            Ok(r) => tracing::info!(status = r.status(), "appcast reachable"),
            Err(e) => tracing::warn!(?e, "appcast smoke failed"),
        }
        Ok(())
    }

    fn current_version(&self) -> &str {
        &self.version
    }
}
