use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Hand-written Info.plist (no plist-crate dep). `.dat0` is declared both as a
/// handled document type and as an exported UTI (dev.dat0.package).
pub fn info_plist(version: &str, git_sha: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>dat0</string>
  <key>CFBundleDisplayName</key><string>dat0</string>
  <key>CFBundleIdentifier</key><string>dev.dat0.app</string>
  <key>CFBundleExecutable</key><string>dat0</string>
  <key>CFBundleIconFile</key><string>dat0.icns</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}+{git_sha}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array><dict>
    <key>CFBundleTypeName</key><string>dat0 Package</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSItemContentTypes</key><array><string>dev.dat0.package</string></array>
  </dict></array>
  <key>UTExportedTypeDeclarations</key>
  <array><dict>
    <key>UTTypeIdentifier</key><string>dev.dat0.package</string>
    <key>UTTypeDescription</key><string>dat0 Package</string>
    <key>UTTypeConformsTo</key><array><string>public.data</string></array>
    <key>UTTypeTagSpecification</key>
    <dict><key>public.filename-extension</key><array><string>dat0</string></array></dict>
  </dict></array>
</dict>
</plist>
"#
    )
}

/// The assembled application bundle.
pub const APP_PATH: &str = "target/macos/dat0.app";

/// The macOS auto-update payload. `update::manifest`'s macOS `url` points at
/// this exact filename (`xtask/src/manifest.rs`), and `release.yml`'s publish
/// job uploads it under that name, so it is deliberately unversioned.
pub const APP_TARBALL: &str = "target/macos/dat0.app.tar.gz";

/// gzip-tar `dat0.app` into [`APP_TARBALL`], replacing any existing archive.
///
/// Called TWICE by design, and the second call is the one that matters:
/// [`bundle`] produces a tarball so a local `cargo xtask bundle-macos` yields
/// a complete artefact set without signing credentials, and
/// `sign::sign_and_notarize` regenerates it from the signed + stapled bundle.
/// Before that second call existed, the archive the macOS auto-updater
/// downloads and swaps into place was built from the UNSIGNED app — the DMG
/// was signed and the update payload was not, bypassing the whole chain.
///
/// The stale archive is removed and its absence asserted BEFORE `tar` runs, so
/// "the replacement happened" is a checked fact rather than a wall-clock guess.
/// Returns the new archive's size in bytes.
pub fn tar_app() -> Result<u64> {
    let tarball = Path::new(APP_TARBALL);
    let _ = std::fs::remove_file(tarball);
    anyhow::ensure!(
        !tarball.exists(),
        "could not remove stale {APP_TARBALL}; refusing to ship a tarball whose \
         provenance is unknown"
    );
    run(Command::new("tar").args(["-czf", APP_TARBALL, "-C", "target/macos", "dat0.app"]))?;
    let len = std::fs::metadata(tarball)
        .with_context(|| format!("{APP_TARBALL} was not created"))?
        .len();
    anyhow::ensure!(len > 0, "{APP_TARBALL} is empty");
    Ok(len)
}

pub fn bundle(version: &str, git_sha: &str) -> Result<PathBuf> {
    // 1. Build both arches.
    for triple in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        run(Command::new("cargo").args([
            "build",
            "-p",
            "dat0-ui",
            "--release",
            "--target",
            triple,
        ]))?;
    }
    // 2. lipo into a universal binary.
    let out = PathBuf::from("target/macos");
    let app = out.join("dat0.app");
    let macos_dir = app.join("Contents/MacOS");
    let res_dir = app.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&res_dir)?;
    run(Command::new("lipo")
        .args([
            "-create",
            "target/aarch64-apple-darwin/release/dat0",
            "target/x86_64-apple-darwin/release/dat0",
            "-output",
        ])
        .arg(macos_dir.join("dat0")))?;
    // 3. Icon + Info.plist.
    super::icon::generate(Path::new("target/icon"))?;
    std::fs::copy("target/icon/dat0.icns", res_dir.join("dat0.icns")).context("copy icns")?;
    std::fs::write(
        app.join("Contents/Info.plist"),
        info_plist(version, git_sha),
    )?;
    // 4. Provisional tar.gz of the .app bundle, so an unsigned local
    //    `bundle-macos` still yields a full artefact set. `sign_and_notarize`
    //    REPLACES this with an archive of the signed + stapled bundle; see
    //    [`tar_app`].
    tar_app()?;
    Ok(app)
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("spawn {cmd:?}"))?;
    anyhow::ensure!(status.success(), "command failed: {cmd:?}");
    Ok(())
}
