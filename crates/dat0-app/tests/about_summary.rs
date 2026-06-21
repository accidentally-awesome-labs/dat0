use dat0_app::about::build_info::BuildInfo;

#[test]
fn build_info_reports_crate_version_and_sha() {
    let b = BuildInfo::current();
    assert_eq!(b.version, env!("CARGO_PKG_VERSION"));
    assert!(!b.git_sha.is_empty()); // real sha or "unknown"
}
