use xtask::macos::info_plist;

#[test]
fn plist_has_required_keys_and_dat0_uti() {
    let p = info_plist("0.1.0", "abc1234");
    for needle in [
        "<key>CFBundleIdentifier</key>", "dev.dat0.app",
        "<key>CFBundleShortVersionString</key>", "0.1.0",
        "<key>CFBundleExecutable</key>", "dat0",
        "<key>CFBundleIconFile</key>", "dat0.icns",
        "<key>NSHighResolutionCapable</key>",
        "<key>CFBundleDocumentTypes</key>",
        "<key>UTExportedTypeDeclarations</key>",
        "dev.dat0.package", "dat0",
    ] {
        assert!(p.contains(needle), "Info.plist missing: {needle}");
    }
}
