//! Smoke test: dat0_app must expose a `run_app(lock, paths)` entry point.

#[test]
fn window_module_exposes_run_app() {
    // Verify the function exists with the T12 signature:
    //   fn(AppLock, Vec<PathBuf>) -> Result<()>
    let _: fn(dat0_app::app_lock::AppLock, Vec<std::path::PathBuf>) -> anyhow::Result<()> =
        dat0_app::run_app;
}
