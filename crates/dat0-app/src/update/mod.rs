//! Minimal update *nudge* (not auto-update): compare the running version to the
//! latest GitHub Release. Sparkle-agnostic — the full updater lands in P10a-2.

pub mod check;
pub mod download;
pub mod manifest;

use anyhow::{Context, Result};

pub const MANIFEST_URL: &str =
    "https://github.com/accidentally-awesome-labs/dat0/releases/latest/download/latest.json";
pub const MANIFEST_SIG_URL: &str = "https://github.com/accidentally-awesome-labs/dat0/releases/latest/download/latest.json.minisig";

/// An available update returned by `check::fetch_update`.
#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub artifact: manifest::ArtifactEntry,
}

pub const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/accidentally-awesome-labs/dat0/releases/latest";

fn parse(v: &str) -> (u64, u64, u64) {
    let v = v.trim().trim_start_matches('v');
    let mut it = v.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// True iff `latest` is a strictly newer semver than `current`.
pub fn newer_than(current: &str, latest: &str) -> bool {
    parse(latest) > parse(current)
}

/// GET the GitHub Releases "latest" JSON and return its `tag_name`.
pub fn fetch_latest(api_url: &str) -> Result<String> {
    let body = ureq::get(api_url)
        .set("User-Agent", "dat0-update-check")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .context("GET latest release")?
        .into_string()
        .context("read body")?;
    let json: serde_json::Value = serde_json::from_str(&body).context("parse release json")?;
    json.get("tag_name")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .context("release json missing tag_name")
}
