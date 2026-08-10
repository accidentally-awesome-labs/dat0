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

/// The signed DMG. Named once because `sign_and_notarize` writes it and
/// [`verify`] re-reads it; two literals drifted apart is exactly the class of
/// bug this release chain has already shipped.
pub const DMG_PATH: &str = "target/macos/dat0.dmg";

/// Staging directory handed to `create-dmg` as its source folder.
///
/// `create-dmg` copies the ENTIRE source folder into the volume, so it must
/// contain nothing but the bundle. `target/macos` cannot serve: `macos.rs:78-84`
/// has already written `dat0.app.tar.gz` beside the bundle, and this module
/// writes `dat0.dmg` into the same directory — both would end up inside the
/// disk image a user mounts.
pub const DMG_STAGING_DIR: &str = "target/macos/dmg-src";

/// Arguments for `create-dmg`, as a pure function so `xtask/tests/sign_args.rs`
/// can assert the layout on any host. The trailing two positionals are
/// `<output.dmg> <source-folder>`, in that order.
pub fn create_dmg_args(dmg: &str, src_dir: &str) -> Vec<String> {
    vec![
        "--volname".into(),
        "dat0".into(),
        "--window-size".into(),
        "500".into(),
        "300".into(),
        // Place the bundle on the left and an /Applications alias on the right
        // — the drag-to-install layout every macOS user expects. Without
        // `--app-drop-link` the volume is just a folder with an app in it.
        "--icon".into(),
        "dat0.app".into(),
        "125".into(),
        "150".into(),
        "--app-drop-link".into(),
        "375".into(),
        "150".into(),
        dmg.into(),
        src_dir.into(),
    ]
}

pub fn sign_and_notarize(identity: &str) -> Result<PathBuf> {
    let app = crate::macos::APP_PATH;
    let dmg = PathBuf::from(DMG_PATH);

    // 1. Sign the .app under the hardened runtime.
    run(Command::new("codesign").args(codesign_args(identity, app)))?;

    // 2. Stage a clean source folder, then build the DMG from it (create-dmg
    //    installed in CI via brew). `ditto`, not `cp`: it preserves the code
    //    signature applied in step 1 along with symlinks and extended
    //    attributes — so the copy inside the DMG has the same cdhash as
    //    `target/macos/dat0.app`, which is what makes step 5 possible. The
    //    original stays in place because `verify` codesign-checks it there.
    let src_dir = PathBuf::from(DMG_STAGING_DIR);
    let _ = std::fs::remove_dir_all(&src_dir);
    std::fs::create_dir_all(&src_dir).context("create dmg staging dir")?;
    run(Command::new("ditto").arg(app).arg(src_dir.join("dat0.app")))?;

    let _ = std::fs::remove_file(&dmg);
    run(Command::new("create-dmg").args(create_dmg_args(DMG_PATH, DMG_STAGING_DIR)))?;

    // 3. Sign the DMG too.
    run(Command::new("codesign").args(codesign_args(identity, DMG_PATH)))?;

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

    // 5. Staple the ticket to the .app as well. Notarization covers the DMG's
    //    contents, and the enclosed bundle is a `ditto` copy of this one, so
    //    the ticket is issued against this cdhash too. Without a stapled
    //    ticket the auto-update payload needs an online Gatekeeper lookup on
    //    first launch and is refused when the machine is offline.
    run(Command::new("xcrun").args(["stapler", "staple", crate::macos::APP_PATH]))?;

    // 6. Rebuild the auto-update payload from the SIGNED, STAPLED bundle.
    //    `macos::bundle` tars the app at the end of bundling — before any of
    //    the above — so until this step existed the archive the macOS updater
    //    downloads and swaps into place was built from the UNSIGNED app. The
    //    DMG was signed and the update path was not, which bypassed the entire
    //    signature chain for every auto-updated install. `tar_app` deletes the
    //    stale archive and asserts its absence before running `tar`, so the
    //    replacement is a checked fact.
    crate::macos::tar_app().context("re-tar the signed bundle as the update payload")?;

    Ok(dmg)
}

/// Gatekeeper / GPG verification gate. Simulates a downloaded (quarantined) DMG.
pub fn verify(macos: bool, linux: bool) -> Result<()> {
    if macos {
        let dmg = DMG_PATH;
        // Simulate download quarantine, then assert Gatekeeper accepts it.
        run(Command::new("xattr").args(["-w", "com.apple.quarantine", "0081;0;Safari;", dmg]))?;
        run(Command::new("spctl").args(["--assess", "--type", "install", "-vvv", dmg]))?;
        run(Command::new("xcrun").args(["stapler", "validate", dmg]))?;
        run(Command::new("codesign").args([
            "--verify",
            "--deep",
            "--strict",
            "--verbose=2",
            crate::macos::APP_PATH,
        ]))?;
        // The .app carries its own notarization ticket, so the auto-update
        // payload launches offline instead of waiting on a Gatekeeper lookup.
        run(Command::new("xcrun").args(["stapler", "validate", crate::macos::APP_PATH]))?;

        // The update payload must be an archive of the SIGNED bundle. The
        // tarball is written last (sign_and_notarize step 6), after stapling
        // rewrites the DMG, so a tarball older than the DMG means it is the
        // stale bundle-time copy and the whole signature chain is bypassed on
        // the update path.
        let tarball = std::fs::metadata(crate::macos::APP_TARBALL)
            .with_context(|| format!("{} missing", crate::macos::APP_TARBALL))?
            .modified()
            .context("tarball mtime")?;
        let image = std::fs::metadata(dmg)
            .with_context(|| format!("{dmg} missing"))?
            .modified()
            .context("dmg mtime")?;
        anyhow::ensure!(
            tarball >= image,
            "{} predates {dmg}: it is the unsigned bundle-time archive, not the \
             signed one sign_and_notarize should have written",
            crate::macos::APP_TARBALL
        );
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
