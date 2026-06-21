use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn desktop_entry() -> String {
    "\
[Desktop Entry]
Type=Application
Name=dat0
Comment=Native data workbench
Exec=dat0 %F
Icon=dat0
Categories=Utility;Development;Database;
Terminal=false
MimeType=application/x-dat0;
"
    .to_string()
}

pub fn mime_xml() -> String {
    "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">
  <mime-type type=\"application/x-dat0\">
    <comment>dat0 Package</comment>
    <glob pattern=\"*.dat0\"/>
  </mime-type>
</mime-info>
"
    .to_string()
}

pub fn bundle(version: &str) -> Result<PathBuf> {
    let triple = "x86_64-unknown-linux-gnu";
    run(Command::new("cargo").args(["build", "-p", "dat0-app", "--release", "--target", triple]))?;

    let out = PathBuf::from("target/linux");
    let appdir = out.join("AppDir");
    std::fs::create_dir_all(appdir.join("usr/bin"))?;
    std::fs::create_dir_all(appdir.join("usr/share/applications"))?;
    std::fs::create_dir_all(appdir.join("usr/share/mime/packages"))?;
    std::fs::create_dir_all(appdir.join("usr/share/icons/hicolor/512x512/apps"))?;

    // Correction 1: binary is named `dat0` (not `dat0-app`) per [[bin]] name in Cargo.toml
    std::fs::copy(
        format!("target/{triple}/release/dat0"),
        appdir.join("usr/bin/dat0"),
    )
    .context("copy binary")?;
    super::icon::generate(Path::new("target/icon"))?;
    std::fs::copy(
        "target/icon/dat0-512.png",
        appdir.join("usr/share/icons/hicolor/512x512/apps/dat0.png"),
    )?;
    std::fs::write(
        appdir.join("usr/share/applications/dat0.desktop"),
        desktop_entry(),
    )?;
    std::fs::write(appdir.join("dat0.desktop"), desktop_entry())?; // linuxdeploy top-level
    std::fs::write(appdir.join("usr/share/mime/packages/dat0.xml"), mime_xml())?;

    // appimagetool packs AppDir → dat0.AppImage.
    let img = out.join("dat0.AppImage");
    run(Command::new("appimagetool").arg(&appdir).arg(&img))?;

    // Correction 2: no --armor so gpg emits binary .sig (not .asc); T7/T9 expect .sig
    run(Command::new("gpg")
        .args(["--batch", "--yes", "--detach-sign"])
        .arg(&img))?;

    // suppress unused-variable warning for version (used by caller for tagging)
    let _ = version;
    Ok(img)
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("spawn {cmd:?}"))?;
    anyhow::ensure!(status.success(), "command failed: {cmd:?}");
    Ok(())
}
