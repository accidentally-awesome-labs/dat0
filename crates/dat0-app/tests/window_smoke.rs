//! Smoke test: dat0_app must expose a `run_app()` entry point.

#[test]
fn window_module_exposes_run_app() {
    let _: fn() -> anyhow::Result<()> = dat0_app::run_app;
}
