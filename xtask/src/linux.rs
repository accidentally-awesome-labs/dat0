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

/// Shared objects the AppImage carries: the ones `dat0` links that a Linux
/// desktop with WebKitGTK installed may lack. muda links libxdo for its menu
/// items, and xdotool, which would bring it, is rarely installed; nor are the
/// two X extensions libxdo links, which WebKitGTK does not bring on Ubuntu
/// 22.04. Only libxdo loads them.
pub const BUNDLED_LIBRARIES: &[&str] = &["libxdo.so.3", "libXtst.so.6", "libXinerama.so.1"];

/// Shared objects `dat0` takes from the host, which a Linux desktop has once
/// WebKitGTK 4.1 is installed (`libwebkit2gtk-4.1-0` on Debian and Ubuntu,
/// `webkit2gtk4.1` on Fedora), with the C runtime and OpenSSL 3.
///
/// WebKitGTK draws a page in helper processes (`WebKitWebProcess`,
/// `WebKitNetworkProcess`) that it starts from a path compiled into the
/// library, so a bundled `libwebkit2gtk` always talks to the host's helpers.
/// The two speak one protocol only when they come from one build: an AppImage
/// that carried Ubuntu 24.04's WebKit 2.52.6 opened a blank window on Ubuntu
/// 25.10, whose WebKit is 2.52.3, because the helpers died on its first
/// message. So WebKit comes from the host, and with it everything WebKit and
/// GTK load, since two copies of GLib or GTK in one process do not work. The
/// host's WebKit, OpenSSL and the rest also get the host's security updates,
/// which a copy frozen into the AppImage would not.
pub const HOST_LIBRARIES: &[&str] = &[
    // WebKitGTK, GTK and GLib.
    "libwebkit2gtk-4.1.so.0",
    "libjavascriptcoregtk-4.1.so.0",
    "libsoup-3.0.so.0",
    "libgtk-3.so.0",
    "libgdk-3.so.0",
    "libgdk_pixbuf-2.0.so.0",
    "libcairo.so.2",
    "libgio-2.0.so.0",
    "libgobject-2.0.so.0",
    "libglib-2.0.so.0",
    // The display and keyboard libraries GTK is built against.
    "libwayland-client.so.0",
    "libfreetype.so.6",
    "libX11.so.6",
    "libXext.so.6",
    "libxkbcommon.so.0",
    // OpenSSL 3, and the C and C++ runtimes.
    "libssl.so.3",
    "libcrypto.so.3",
    "libstdc++.so.6",
    "libgcc_s.so.1",
    "libm.so.6",
    "libc.so.6",
    "ld-linux-x86-64.so.2",
];

/// Where a bundled library is found beside the binary. The binary carries it
/// as DT_RPATH, so the AppImage needs no launcher script and nothing `dat0`
/// starts inherits a changed library path. Not DT_RUNPATH, which the linker
/// writes by default: that serves only the binary's own libraries, and on
/// Ubuntu 22.04 libxdo then could not find the X extensions beside it.
pub const RPATH: &str = "$ORIGIN/../lib";

/// Build, package and GPG-sign the Linux AppImage for `version`; `sign: false`
/// leaves the signature out, for a dry run without the key.
///
/// `version` is not baked into the in-`target/` filename on purpose: four
/// call sites hardcode `target/linux/dat0.AppImage` (`sign.rs`'s `verify`,
/// and `release.yml`'s `APPIMAGE=` in the publish job). The version reaches
/// the release as an ASSET name — `release.yml`'s publish job re-stages the
/// artefact as `dat0-{version}-x86_64.AppImage` before uploading, which is
/// the name `README.md` documents. Here it is the error context, so a
/// failed release names the version it was cutting.
pub fn bundle(version: &str, sign: bool) -> Result<PathBuf> {
    bundle_appimage(sign).with_context(|| format!("bundling v{version}"))
}

/// The shared objects `elf` names as NEEDED, read with `readelf`.
pub fn needed(elf: &Path) -> Result<Vec<String>> {
    let out = Command::new("readelf")
        .arg("-d")
        .arg(elf)
        .output()
        .with_context(|| format!("run readelf -d {}", elf.display()))?;
    anyhow::ensure!(out.status.success(), "readelf -d {} failed", elf.display());
    Ok(parse_needed(&String::from_utf8_lossy(&out.stdout)))
}

/// The DT_RPATH entry of `readelf -d` output:
/// ` 0x000000000000000f (RPATH)  Library rpath: [$ORIGIN/../lib]`. A
/// DT_RUNPATH is not one.
pub fn parse_rpath(readelf: &str) -> Option<&str> {
    readelf
        .lines()
        .find(|l| l.contains("(RPATH)"))
        .and_then(|l| l.split_once('[')?.1.split_once(']'))
        .map(|(path, _)| path)
}

/// The NEEDED entries of `readelf -d` output, in order:
/// ` 0x0000000000000001 (NEEDED)  Shared library: [libxdo.so.3]`.
pub fn parse_needed(readelf: &str) -> Vec<String> {
    readelf
        .lines()
        .filter(|l| l.contains("(NEEDED)"))
        .filter_map(|l| Some(l.split_once('[')?.1.split_once(']')?.0.to_string()))
        .collect()
}

/// Sort the libraries an object needs into the ones to bundle, and fail on
/// any library neither list names: whether the host provides a new
/// dependency, or the AppImage carries it, is a decision, not a default.
pub fn classify(needed: &[String]) -> Result<Vec<String>> {
    let mut bundle = Vec::new();
    for lib in needed {
        if BUNDLED_LIBRARIES.contains(&lib.as_str()) {
            bundle.push(lib.clone());
        } else if !HOST_LIBRARIES.contains(&lib.as_str()) {
            anyhow::bail!(
                "dat0 now needs {lib}, which neither xtask::linux::BUNDLED_LIBRARIES nor \
                 HOST_LIBRARIES names. Add it to HOST_LIBRARIES if every desktop with \
                 WebKitGTK has it, and to BUNDLED_LIBRARIES if not."
            );
        }
    }
    Ok(bundle)
}

/// Where the build host's loader finds `lib`, from `ldd`'s `name => path` lines.
fn resolve(elf: &Path, lib: &str) -> Result<PathBuf> {
    let out = Command::new("ldd")
        .arg(elf)
        .output()
        .with_context(|| format!("run ldd {}", elf.display()))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| {
            let (name, rest) = l.trim().split_once(" => ")?;
            let path = rest.split(" (").next()?.trim();
            (name == lib).then(|| PathBuf::from(path))
        })
        .with_context(|| format!("ldd cannot find {lib} for {}", elf.display()))
}

fn bundle_appimage(sign: bool) -> Result<PathBuf> {
    let triple = "x86_64-unknown-linux-gnu";
    // The library path goes in at link time. Appended to what RUSTFLAGS
    // already holds; with `--target` given, it reaches the binary and not the
    // build scripts.
    let rustflags = format!(
        "{} -C link-arg=-Wl,--disable-new-dtags,-rpath,{RPATH}",
        std::env::var("RUSTFLAGS").unwrap_or_default()
    );
    run(Command::new("cargo")
        .args(["build", "-p", "dat0-ui", "--release", "--target", triple])
        .env("RUSTFLAGS", rustflags.trim()))?;
    let exe = PathBuf::from(format!("target/{triple}/release/dat0"));

    let out = PathBuf::from("target/linux");
    let appdir = out.join("AppDir");
    // A stale AppDir would carry a library a previous run bundled and this
    // binary no longer needs.
    let _ = std::fs::remove_dir_all(&appdir);
    let bin = appdir.join("usr/bin");
    let lib = appdir.join("usr/lib");
    let apps = appdir.join("usr/share/applications");
    let icons = appdir.join("usr/share/icons/hicolor/512x512/apps");
    let mime = appdir.join("usr/share/mime/packages");
    for dir in [&bin, &lib, &apps, &icons, &mime] {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::copy(&exe, bin.join("dat0")).context("copy dat0 into the AppDir")?;

    let dynamic = Command::new("readelf").arg("-d").arg(&exe).output()?;
    anyhow::ensure!(
        parse_rpath(&String::from_utf8_lossy(&dynamic.stdout)) == Some(RPATH),
        "{} carries no DT_RPATH {RPATH}, so the libraries the AppImage bundles \
         would not be found",
        exe.display()
    );

    // What the binary links, and in turn what each bundled library links:
    // every name is sorted into the host's or the AppImage's.
    let mut pending = classify(&needed(&exe)?)?;
    let mut bundled = Vec::new();
    while let Some(name) = pending.pop() {
        if bundled.contains(&name) {
            continue;
        }
        let path = resolve(&exe, &name)?;
        std::fs::copy(&path, lib.join(&name))
            .with_context(|| format!("copy {} into the AppDir", path.display()))?;
        pending.extend(classify(&needed(&path)?)?);
        bundled.push(name);
    }

    std::fs::write(apps.join("dat0.desktop"), desktop_entry()).context("write desktop entry")?;
    super::icon::generate(Path::new("target/icon"))?;
    // The icon's name is the desktop entry's `Icon=` value.
    std::fs::copy("target/icon/dat0-512.png", icons.join("dat0.png")).context("copy icon")?;
    std::fs::write(mime.join("dat0.xml"), mime_xml())?;
    // The notices the licences ask to travel with the binary.
    super::legal::copy_into(Path::new("."), &appdir.join("usr/share/doc/dat0"))?;
    // appimagetool wants the desktop entry, the icon and an AppRun at the
    // AppDir's root; the binary is the AppRun.
    symlink("usr/bin/dat0", &appdir.join("AppRun"))?;
    symlink(
        "usr/share/applications/dat0.desktop",
        &appdir.join("dat0.desktop"),
    )?;
    symlink(
        "usr/share/icons/hicolor/512x512/apps/dat0.png",
        &appdir.join("dat0.png"),
    )?;
    symlink("dat0.png", &appdir.join(".DirIcon"))?;

    // appimagetool packs AppDir → dat0.AppImage. Without a runtime file it
    // downloads the newest runtime; release.yml hands it a pinned one.
    let img = out.join("dat0.AppImage");
    let _ = std::fs::remove_file(&img);
    let mut pack = Command::new("appimagetool");
    if let Some(runtime) = std::env::var_os("DAT0_APPIMAGE_RUNTIME") {
        pack.arg("--runtime-file").arg(runtime);
    }
    run(pack.arg(&appdir).arg(&img))?;

    if sign {
        gpg_sign(&img)?;
    }
    Ok(img)
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("link {} -> {target}", link.display()))
}

#[cfg(not(unix))]
fn symlink(_target: &str, _link: &Path) -> Result<()> {
    anyhow::bail!("the AppImage is built on Linux")
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
