use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    // Test / portable-install relocation seam: a non-empty `DAT0_CONFIG_DIR`
    // overrides the default location verbatim. When unset/empty the body below
    // is reached and behaves byte-identically to before this seam existed.
    if let Some(p) = std::env::var_os("DAT0_CONFIG_DIR").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
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
