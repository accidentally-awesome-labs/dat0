use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn codesign_args(identity: &str, path: &str) -> Vec<String> {
    vec![
        "--force".into(),
        "--deep".into(),
        "--options".into(),
        "runtime".into(),
        "--timestamp".into(),
        "--sign".into(),
        identity.into(),
        path.into(),
    ]
}

pub fn sign_and_notarize(identity: &str) -> Result<PathBuf> {
    let app = "target/macos/dat0.app";
    let dmg = PathBuf::from("target/macos/dat0.dmg");

    // 1. Sign the .app under the hardened runtime.
    run(Command::new("codesign").args(codesign_args(identity, app)))?;

    // 2. Build the DMG (create-dmg installed in CI via brew).
    let _ = std::fs::remove_file(&dmg);
    run(Command::new("create-dmg")
        .args(["--volname", "dat0", "--window-size", "500", "300"])
        .arg(&dmg)
        .arg(app))?;

    // 3. Sign the DMG too.
    run(Command::new("codesign").args(codesign_args(identity, dmg.to_str().unwrap())))?;

    // 4. Notarize (App Store Connect API key from env) + staple.
    let key_id = std::env::var("AC_KEY_ID").context("AC_KEY_ID")?;
    let issuer = std::env::var("AC_ISSUER_ID").context("AC_ISSUER_ID")?;
    let key_path = std::env::var("AC_API_KEY_PATH").context("AC_API_KEY_PATH")?;
    run(Command::new("xcrun")
        .args(["notarytool", "submit"])
        .arg(&dmg)
        .args([
            "--key", &key_path, "--key-id", &key_id, "--issuer", &issuer, "--wait",
        ]))?;
    run(Command::new("xcrun").args(["stapler", "staple"]).arg(&dmg))?;
    Ok(dmg)
}

/// Gatekeeper / GPG verification gate. Simulates a downloaded (quarantined) DMG.
pub fn verify(macos: bool, linux: bool) -> Result<()> {
    if macos {
        let dmg = "target/macos/dat0.dmg";
        // Simulate download quarantine, then assert Gatekeeper accepts it.
        run(Command::new("xattr").args([
            "-w",
            "com.apple.quarantine",
            "0081;0;Safari;",
            dmg,
        ]))?;
        run(Command::new("spctl").args(["--assess", "--type", "install", "-vvv", dmg]))?;
        run(Command::new("xcrun").args(["stapler", "validate", dmg]))?;
        run(Command::new("codesign").args([
            "--verify",
            "--deep",
            "--strict",
            "--verbose=2",
            "target/macos/dat0.app",
        ]))?;
    }
    if linux {
        let img = "target/linux/dat0.AppImage";
        run(Command::new("gpg").args(["--verify", &format!("{img}.sig"), img]))?;
        // Functional smoke is run under a clean Docker Ubuntu in release.yml.
    }
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("spawn {cmd:?}"))?;
    anyhow::ensure!(status.success(), "command failed: {cmd:?}");
    Ok(())
}
