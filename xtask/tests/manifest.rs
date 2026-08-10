use xtask::manifest::build_manifest;

#[test]
fn manifest_has_version_and_both_platform_artifacts() {
    let j = build_manifest("0.2.0", "aa", 10, "bb", 20);
    for needle in [
        "\"version\": \"0.2.0\"",
        "dat0.app.tar.gz",
        // RL4: the AppImage release asset is re-staged under a versioned name
        // before upload, so the manifest URL must carry the version too.
        "dat0-0.2.0-x86_64.AppImage",
        "\"sha256\": \"aa\"",
        "\"size\": 20",
    ] {
        assert!(j.contains(needle), "manifest missing: {needle}\n{j}");
    }
}
