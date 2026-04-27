fn main() -> anyhow::Result<()> {
    dat0_app::boot::init_logging()?;
    tracing::info!("dat0 starting");
    dat0_app::run_app()
}
