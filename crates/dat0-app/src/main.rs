use anyhow::Result;
use dat0_app::app_lock::{AppLock, OpenWindowMessage};

fn main() -> Result<()> {
    dat0_app::boot::init_logging()?;
    let _ctx = dat0_app::boot::AppContext::boot()?;
    let state_dir = dat0_app::platform::data_dir()?;
    let cli_paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
    let lock = match AppLock::try_acquire(&state_dir)? {
        Some(l) => l,
        None => {
            // Another instance is running — forward the open-window request and exit.
            AppLock::forward_open_window(&state_dir, OpenWindowMessage { paths: cli_paths })?;
            return Ok(());
        }
    };
    tracing::info!("dat0 starting");
    dat0_app::run_app(lock, cli_paths)
}
