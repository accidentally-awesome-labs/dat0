use xtask::macos::info_plist;

#[test]
fn plist_has_required_keys_and_dat0_uti() {
    let p = info_plist("0.1.0", "abc1234");
    for needle in [
        "<key>CFBundleShortVersionString</key>",
        "0.1.0",
        "<key>CFBundleExecutable</key>",
        "dat0",
        "<key>CFBundleIconFile</key>",
        "dat0.icns",
        "<key>NSHighResolutionCapable</key>",
        "<key>CFBundleDocumentTypes</key>",
        "<key>UTExportedTypeDeclarations</key>",
        "dat0",
    ] {
        assert!(p.contains(needle), "Info.plist missing: {needle}");
    }
}

/// The identifiers are reverse-DNS of dat0.app, the project's domain, matched
/// whole: `app.dat0` alone is also the start of the file type's name.
#[test]
fn the_identifiers_name_the_projects_domain() {
    let p = info_plist("0.1.0", "abc1234");
    for pair in [
        "<key>CFBundleIdentifier</key><string>app.dat0</string>",
        "<key>UTTypeIdentifier</key><string>app.dat0.package</string>",
        // The type dat0 opens is the one it exports, so a double-clicked
        // `.dat0` comes to it.
        "<key>LSItemContentTypes</key><array><string>app.dat0.package</string></array>",
    ] {
        assert!(p.contains(pair), "Info.plist missing: {pair}");
    }
    assert!(!p.contains("dev.dat0"), "Info.plist still names dat0.dev");
}
