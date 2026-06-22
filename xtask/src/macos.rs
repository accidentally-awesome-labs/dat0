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

pub fn bundle(version: &str, git_sha: &str) -> Result<PathBuf> {
    // 1. Build both arches.
    for triple in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        run(Command::new("cargo").args([
            "build",
            "-p",
            "dat0-app",
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
    // 4. Create tar.gz of the .app bundle.
    run(Command::new("tar").args(["-czf", "target/macos/dat0.app.tar.gz", "-C", "target/macos", "dat0.app"]))?;
    Ok(app)
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("spawn {cmd:?}"))?;
    anyhow::ensure!(status.success(), "command failed: {cmd:?}");
    Ok(())
}
