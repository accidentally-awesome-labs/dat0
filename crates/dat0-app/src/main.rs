fn main() -> anyhow::Result<()> {
    dat0_app::boot::init_logging()?;
    let _ctx = dat0_app::boot::AppContext::boot()?;
    tracing::info!("dat0 starting");
    dat0_app::run_app()
}
