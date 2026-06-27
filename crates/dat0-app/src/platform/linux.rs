use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    // Test / portable-install relocation seam: a non-empty `DAT0_CONFIG_DIR`
    // overrides the default location verbatim. When unset/empty the body below
    // is reached and behaves byte-identically to before this seam existed.
    if let Some(p) = std::env::var_os("DAT0_CONFIG_DIR").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
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

/// Open `url` in the user's default browser via the freedesktop `xdg-open` tool.
pub fn open_url(url: &str) -> Result<()> {
    let status = std::process::Command::new("xdg-open").arg(url).status()?;
    anyhow::ensure!(status.success(), "open_url failed for {url}");
    Ok(())
}
