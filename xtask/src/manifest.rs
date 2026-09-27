//! Build `latest.json` manifest for auto-update, and check its signature as
//! the app will before it is published.

use anyhow::{Context, Result, anyhow};

/// Generate a JSON manifest string for the update system.
///
/// The JSON has the shape expected by `dat0-ui`'s `update::manifest::UpdateManifest`:
/// ```json
/// {
///   "version": "0.2.0",
///   "macos": { "url": "...", "sha256": "...", "size": ... },
///   "linux": { "url": "...", "sha256": "...", "size": ... }
/// }
/// ```
pub fn build_manifest(
    version: &str,
    macos_sha: &str,
    macos_size: u64,
    linux_sha: &str,
    linux_size: u64,
) -> String {
    // The macOS updater artifact is the `.app` TARBALL, not the DMG, and it is
    // uploaded to the release under its plain name by `release.yml`'s publish
    // job (`gh release create … upload/dat0.app.tar.gz`) — the asset name is
    // the basename, so this URL must stay unversioned. Only the
    // DMG and the AppImage are re-staged under versioned names before upload.
    let macos_url = format!(
        "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{}/dat0.app.tar.gz",
        version
    );
    // The AppImage IS re-staged: `release.yml`'s publish job copies
    // `dat0.AppImage` to `dat0-{version}-x86_64.AppImage` before uploading, so
    // the release asset — and therefore this URL — carries the version. This
    // is the name `README.md:37` documents to users.
    let linux_asset = format!("dat0-{version}-x86_64.AppImage");
    let linux_url = format!(
        "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{}/{}",
        version, linux_asset
    );

    format!(
        r#"{{
  "version": "{}",
  "macos": {{ "url": "{}", "sha256": "{}", "size": {} }},
  "linux": {{ "url": "{}", "sha256": "{}", "size": {} }}
}}"#,
        version, macos_url, macos_sha, macos_size, linux_url, linux_sha, linux_size
    )
}

/// Check a signed manifest as the app will: its signature verifies against
/// `public_key`, the one line of `crates/dat0-core/assets/minisign-public-key.txt`
/// the app compiles in, and is not a legacy signature, which the app refuses.
/// The app's own check is `dat0_core::update::manifest::verify_manifest`; xtask
/// does not depend on dat0-core, so this makes the same `minisign-verify` call.
///
/// `release.yml`'s publish job runs it after signing and before uploading
/// anything: a `MINISIGN_SECRET_KEY` that is not the pair of the committed key
/// signs without complaint, and every client would then refuse the update.
pub fn verify(manifest: &[u8], minisig: &str, public_key: &str) -> Result<()> {
    let key = minisign_verify::PublicKey::from_base64(public_key.trim())
        .context("decode the update public key")?;
    let signature =
        minisign_verify::Signature::decode(minisig).context("decode the manifest signature")?;
    key.verify(manifest, &signature, false).map_err(|e| {
        anyhow!("the manifest's signature does not verify against the key dat0 trusts: {e}")
    })
}
