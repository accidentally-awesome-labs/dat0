//! Smoke test: dat0_app must expose a `run_app(lock, paths, main_loop)` entry point.

#[test]
fn window_module_exposes_run_app() {
    // Verify the function exists with the P3b T1 signature:
    //   fn(AppLock, Vec<PathBuf>, MainLoop) -> Result<()>
    let _: fn(
        dat0_app::app_lock::AppLock,
        Vec<std::path::PathBuf>,
        dat0_app::main_bridge::MainLoop,
    ) -> anyhow::Result<()> = dat0_app::run_app;
}
