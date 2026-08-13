//! Parse + cryptographically verify the update manifest before trusting it.
//!
//! `verify_manifest` verifies the minisign (Ed25519) signature over the raw
//! `latest.json` bytes FIRST, and ONLY THEN deserialises the JSON.  Nothing in
//! the manifest is trusted before the signature is confirmed.
use anyhow::{Context, Result};
use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ArtifactEntry {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UpdateManifest {
    pub version: String,
    pub macos: ArtifactEntry,
    pub linux: ArtifactEntry,
}

/// Embedded minisign public key (PRODUCTION) used to verify `latest.json`.
/// Currently a TEST-key placeholder; replaced by the real production key in T8 ops.
/// NOTE: `include_str!` keeps the file's trailing newline — `verify_manifest` trims
/// its `pubkey_b64` argument internally, so callers may pass this constant directly.
pub const EMBEDDED_PUBKEY: &str = include_str!("../../assets/minisign-public-key.txt");

/// Verify the minisign signature over `manifest_bytes` using `pubkey_b64`,
/// THEN parse the JSON.  Verification happens before any trust in the content.
pub fn verify_manifest(
    manifest_bytes: &[u8],
    minisig: &str,
    pubkey_b64: &str,
) -> Result<UpdateManifest> {
    let pk = PublicKey::from_base64(pubkey_b64.trim()).context("decode minisign pubkey")?;
    let sig = Signature::decode(minisig).context("decode minisign signature")?;
    pk.verify(manifest_bytes, &sig, false)
        .map_err(|e| anyhow::anyhow!("manifest signature verification failed: {e}"))?;
    serde_json::from_slice(manifest_bytes).context("parse verified manifest json")
}

#[cfg(target_os = "macos")]
pub fn for_current_platform(m: &UpdateManifest) -> &ArtifactEntry {
    &m.macos
}

#[cfg(target_os = "linux")]
pub fn for_current_platform(m: &UpdateManifest) -> &ArtifactEntry {
    &m.linux
}
