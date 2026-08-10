use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// Build, package and GPG-sign the Linux AppImage for `version`.
///
/// `version` is not baked into the in-`target/` filename on purpose: four
/// call sites hardcode `target/linux/dat0.AppImage` (`sign.rs`'s `verify`,
/// and `release.yml`'s `APPIMAGE=` in the publish job). The version reaches
/// the release as an ASSET name — `release.yml`'s publish job re-stages the
/// artefact as `dat0-{version}-x86_64.AppImage` before uploading, which is
/// the name `README.md:37` documents. Here it is the error context, so a
/// failed release names the version it was cutting.
pub fn bundle(version: &str) -> Result<PathBuf> {
    bundle_appimage().with_context(|| format!("bundling v{version}"))
}

fn bundle_appimage() -> Result<PathBuf> {
    let triple = "x86_64-unknown-linux-gnu";
    run(Command::new("cargo").args(["build", "-p", "dat0-ui", "--release", "--target", triple]))?;

    let out = PathBuf::from("target/linux");
    let appdir = out.join("AppDir");
    // A stale AppDir would let a library deployed by a previous run mask a
    // dependency the current binary no longer resolves — the failure would
    // then only appear on a user's machine.
    let _ = std::fs::remove_dir_all(&appdir);

    // Desktop file and icon are staged OUTSIDE the AppDir: linuxdeploy installs
    // them itself (into usr/share/{applications,icons} plus the top-level
    // symlinks it needs), and handing it a path already inside the AppDir makes
    // it copy a file onto itself.
    let stage = out.join("stage");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage)?;
    let desktop = stage.join("dat0.desktop");
    // The icon's BASENAME must equal the desktop entry's `Icon=` value (`dat0`)
    // or linuxdeploy rejects it — which is why `dat0-512.png` is re-staged
    // rather than passed through under its build name.
    let icon = stage.join("dat0.png");
    std::fs::write(&desktop, desktop_entry()).context("stage desktop entry")?;
    super::icon::generate(Path::new("target/icon"))?;
    std::fs::copy("target/icon/dat0-512.png", &icon).context("stage icon")?;

    // Correction 1: binary is named `dat0` (not `dat0-ui`) per [[bin]] name in Cargo.toml
    let exe = format!("target/{triple}/release/dat0");

    // linuxdeploy walks the executable's DT_NEEDED graph and copies every
    // non-excluded shared object into AppDir/usr/lib, then writes the AppRun
    // that puts that directory on the loader path.
    //
    // This pass was missing. The AppDir held the bare binary and NO AppRun at
    // all, so the image could only ever run on a host that already had
    // libssl/libpango/libxkbcommon/libsecret installed — the opposite of what
    // an AppImage is for, and precisely what `release.yml`'s clean-container
    // smoke test exists to catch.
    //
    // Libraries on the AppImage excludelist (libc, libstdc++, libGL, libX11,
    // libfontconfig, glib …) are deliberately NOT bundled: the format declares
    // them the host desktop's responsibility. That contract is why the smoke
    // container installs that baseline before running AppRun.
    run(Command::new("linuxdeploy")
        .arg("--appdir")
        .arg(&appdir)
        .arg("--executable")
        .arg(&exe)
        .arg("--desktop-file")
        .arg(&desktop)
        .arg("--icon-file")
        .arg(&icon))?;

    // MIME registration is not a linuxdeploy concern, so it is added after the
    // deploy pass rather than before it.
    let mime_dir = appdir.join("usr/share/mime/packages");
    std::fs::create_dir_all(&mime_dir)?;
    std::fs::write(mime_dir.join("dat0.xml"), mime_xml())?;

    // Prove the deploy pass did something. `linuxdeploy` exits 0 on an AppDir
    // it declined to touch, and an empty usr/lib is exactly the state that
    // shipped before this pass existed.
    anyhow::ensure!(
        appdir.join("AppRun").exists(),
        "linuxdeploy produced no AppRun in {}; appimagetool would fail and the \
         AppImage would have no entry point",
        appdir.display()
    );
    let deployed = std::fs::read_dir(appdir.join("usr/lib"))
        .map(|d| d.count())
        .unwrap_or(0);
    anyhow::ensure!(
        deployed > 0,
        "linuxdeploy bundled no shared libraries into {}/usr/lib; the AppImage \
         would only run on this build host",
        appdir.display()
    );

    // appimagetool packs AppDir → dat0.AppImage.
    let img = out.join("dat0.AppImage");
    let _ = std::fs::remove_file(&img);
    run(Command::new("appimagetool").arg(&appdir).arg(&img))?;

    gpg_sign(&img)?;
    Ok(img)
}

/// GPG-sign the AppImage into a detached BINARY `.sig` (no `--armor`: T7/T9 and
/// `sign::verify` both expect `.sig`, not `.asc`).
///
/// `DAT0_GPG_PASSPHRASE` is OPTIONAL by design, and the two branches are not
/// interchangeable:
///
/// * Unset → plain `gpg --batch --yes --detach-sign`. This is the posture
///   `docs/security-runbook.md:120-123` recommends (a dedicated passphraseless
///   CI subkey). Passing `--passphrase-fd 0` with an empty passphrase makes gpg
///   FAIL on such a key rather than skip the prompt, so the flags must be
///   absent, not merely empty.
/// * Set → the protected-key branch of `docs/release-runbook.md:72-74`:
///   loopback pinentry with the passphrase on stdin. Never on argv — argv is
///   world-readable through `ps` on the runner.
fn gpg_sign(img: &Path) -> Result<()> {
    let passphrase = std::env::var("DAT0_GPG_PASSPHRASE")
        .ok()
        .filter(|p| !p.is_empty());

    let mut cmd = Command::new("gpg");
    cmd.args(["--batch", "--yes"]);
    if passphrase.is_some() {
        cmd.args(["--pinentry-mode", "loopback", "--passphrase-fd", "0"]);
    }
    cmd.arg("--detach-sign").arg(img);

    let Some(passphrase) = passphrase else {
        return run(&mut cmd);
    };

    cmd.stdin(Stdio::piped());
    let mut child = cmd.spawn().with_context(|| format!("spawn {cmd:?}"))?;
    {
        // Dropping the handle closes the pipe, which is the EOF gpg waits for
        // after reading the passphrase line from fd 0.
        let mut stdin = child.stdin.take().context("gpg stdin pipe")?;
        stdin
            .write_all(passphrase.as_bytes())
            .context("write gpg passphrase")?;
    }
    let status = child.wait().context("wait for gpg")?;
    anyhow::ensure!(status.success(), "gpg --detach-sign failed: {status}");
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("spawn {cmd:?}"))?;
    anyhow::ensure!(status.success(), "command failed: {cmd:?}");
    Ok(())
}
