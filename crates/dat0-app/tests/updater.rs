use dat0_app::updater::Updater;

#[cfg(target_os = "macos")]
#[test]
fn sparkle_reports_crate_version() {
    use dat0_app::updater::SparkleUpdater;
    let u = SparkleUpdater::new().unwrap();
    assert_eq!(u.current_version(), env!("CARGO_PKG_VERSION"));
}

#[cfg(target_os = "linux")]
#[test]
fn appimage_reports_crate_version() {
    use dat0_app::updater::AppImageUpdater;
    let u = AppImageUpdater::new().unwrap();
    assert_eq!(u.current_version(), env!("CARGO_PKG_VERSION"));
}
