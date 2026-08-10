use dat0_core::about::build_info::BuildInfo;

#[test]
fn build_info_reports_crate_version_and_sha() {
    let b = BuildInfo::current();
    assert_eq!(b.version, env!("CARGO_PKG_VERSION"));
    assert!(!b.git_sha.is_empty()); // real sha or "unknown"
}

fn fixture() -> BuildInfo {
    BuildInfo {
        version: "0.1.0",
        git_sha: "abc1234",
        built: Some("1718600000"),
    }
}

#[test]
fn summary_contains_version_sha_and_license() {
    let lines = dat0_core::about::summary_lines(&fixture(), None);
    let joined = lines.join("\n");
    assert!(joined.contains("0.1.0"));
    assert!(joined.contains("abc1234"));
    assert!(joined.contains("Apache-2.0"));
    // No update available → no "available" nudge line.
    assert!(!joined.to_lowercase().contains("available"));
}

#[test]
fn summary_shows_update_nudge_when_newer() {
    let lines = dat0_core::about::summary_lines(&fixture(), Some("0.2.0"));
    let joined = lines.join("\n");
    assert!(joined.contains("0.2.0"));
    assert!(joined.to_lowercase().contains("available"));
}
