//! Build `latest.json` manifest for auto-update.

/// Generate a JSON manifest string for the update system.
///
/// The JSON has the shape expected by `dat0-app`'s `update::manifest::UpdateManifest`:
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
    let macos_url = format!(
        "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{}/dat0.app.tar.gz",
        version
    );
    let linux_url = format!(
        "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{}/dat0.AppImage",
        version
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
