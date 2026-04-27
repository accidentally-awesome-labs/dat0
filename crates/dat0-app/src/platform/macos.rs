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
