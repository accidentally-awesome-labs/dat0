use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("no home")?
        .join("Library/Application Support/dat0"))
}

pub fn data_dir() -> Result<PathBuf> {
    config_dir()
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("no home")?
        .join("Library/Caches/dat0"))
}

/// Open `url` in the user's default browser via the macOS `open` shell tool.
pub fn open_url(url: &str) -> Result<()> {
    let status = std::process::Command::new("open").arg(url).status()?;
    anyhow::ensure!(status.success(), "open_url failed for {url}");
    Ok(())
}
