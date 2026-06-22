//! Fetch + cryptographically verify the update manifest, then compare
//! against the currently-running version.  Returns `Some(AvailableUpdate)`
//! iff the remote version is strictly newer than `current_version`.

use super::manifest::{self, ArtifactEntry, UpdateManifest};
use anyhow::{Context, Result};

fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    use std::io::Read;
    ureq::get(url)
        .set("User-Agent", "dat0-updater")
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .context("GET")?
        .into_reader()
        .read_to_end(&mut buf)
        .context("read body")?;
    Ok(buf)
}

pub fn fetch_update(
    manifest_url: &str,
    sig_url: &str,
    pubkey_b64: &str,
    current_version: &str,
) -> Result<Option<super::AvailableUpdate>> {
    let manifest_bytes = get_bytes(manifest_url)?;
    let sig = String::from_utf8(get_bytes(sig_url)?).context("sig utf8")?;
    let m: UpdateManifest = manifest::verify_manifest(&manifest_bytes, &sig, pubkey_b64)?;
    if super::newer_than(current_version, &m.version) {
        let artifact: ArtifactEntry = manifest::for_current_platform(&m).clone();
        Ok(Some(super::AvailableUpdate {
            version: m.version,
            artifact,
        }))
    } else {
        Ok(None)
    }
}
