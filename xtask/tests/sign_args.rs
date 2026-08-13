use xtask::sign::{DMG_PATH, DMG_STAGING_DIR, codesign_args, create_dmg_args};

#[test]
fn codesign_uses_hardened_runtime_and_timestamp() {
    let args = codesign_args("Developer ID Application: Acme", "target/macos/dat0.app");
    assert!(args.contains(&"--options".to_string()));
    assert!(args.contains(&"runtime".to_string())); // hardened runtime
    assert!(args.contains(&"--timestamp".to_string())); // secure timestamp
    assert!(args.contains(&"--force".to_string()));
    assert!(args.iter().any(|a| a.contains("dat0.app")));
    assert!(args.iter().any(|a| a.contains("Developer ID Application")));
}

/// RL4: `create-dmg`'s last two positionals are `<output.dmg> <source-folder>`.
/// The original invocation passed the `.app` BUNDLE as the source folder, which
/// create-dmg treats as a directory to copy wholesale — and `target/macos`, the
/// obvious alternative, already holds `dat0.app.tar.gz` (`xtask/src/macos.rs:78-84`)
/// and the output `dat0.dmg` itself. Both would be copied into the volume.
#[test]
fn create_dmg_source_is_the_clean_staging_dir() {
    let args = create_dmg_args(DMG_PATH, DMG_STAGING_DIR);

    // Drag-to-install layout: without this the volume has no /Applications alias.
    assert!(
        args.contains(&"--app-drop-link".to_string()),
        "create-dmg must place an /Applications drop link: {args:?}"
    );

    // The trailing positionals, in create-dmg's required order.
    let n = args.len();
    assert_eq!(args[n - 2], "target/macos/dat0.dmg", "output positional");
    assert_eq!(
        args[n - 1],
        "target/macos/dmg-src",
        "source positional must be the staging dir, never the .app bundle or \
         target/macos (which holds dat0.app.tar.gz and the output dmg): {args:?}"
    );

    // The staging dir must not be the directory the output lands in, or the
    // in-progress dmg would be copied into itself.
    assert_ne!(
        std::path::Path::new(DMG_PATH).parent(),
        Some(std::path::Path::new(DMG_STAGING_DIR)),
        "dmg output must not live inside the staging dir"
    );
}

/// RL4: the macOS auto-update payload must be tarred from the SIGNED, STAPLED
/// bundle — not from the bundle `macos::bundle` assembled minutes earlier.
///
/// `macos::bundle` writes `dat0.app.tar.gz` at the end of bundling, i.e. before
/// `sign-macos` runs at all. That archive is what `update::apply` downloads and
/// swaps into `/Applications`, so shipping the bundle-time copy means the DMG
/// is signed and notarized while every auto-updated install is neither. The
/// ordering is not expressible in the type system — both calls are plain
/// statements — so it is asserted on the source, the same way
/// `crates/dat0-app/tests/window_module_ratchet.rs` asserts its invariants.
#[test]
fn update_payload_is_retarred_after_notarization() {
    let src = include_str!("../src/sign.rs");

    let staple_dmg = src
        .find(r#"["stapler", "staple"]"#)
        .expect("sign_and_notarize must staple the DMG");
    let staple_app = src
        .find(r#"["stapler", "staple", crate::macos::APP_PATH]"#)
        .expect(
            "sign_and_notarize must staple the .app too, or the update payload \
             needs an online Gatekeeper lookup on first launch",
        );
    let retar = src
        .find("crate::macos::tar_app()")
        .expect("sign_and_notarize must rebuild dat0.app.tar.gz from the signed bundle");

    assert!(
        staple_app > staple_dmg,
        "the .app is stapled from the DMG's notarization ticket, so it must \
         come after the DMG staple"
    );
    assert!(
        retar > staple_app,
        "tar_app must run AFTER signing + stapling; re-tarring earlier ships an \
         unsigned update payload, which is the defect this test exists to lock out"
    );
}
