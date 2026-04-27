use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no XDG_CONFIG_HOME")?
        .join("dat0"))
}

pub fn data_dir() -> Result<PathBuf> {
    Ok(dirs::data_dir().context("no XDG_DATA_HOME")?.join("dat0"))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::cache_dir().context("no XDG_CACHE_HOME")?.join("dat0"))
}
