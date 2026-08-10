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

/// Resident set size of this process, in bytes. `None` on any failure.
///
/// Field 2 of `/proc/self/statm` is the resident page count; the kernel reports
/// pages, not bytes, so it is scaled by the runtime page size rather than an
/// assumed 4 KiB — arm64 hosts commonly run 16 KiB pages and the constant would
/// under-report by 4×.
///
/// Best-effort by contract: a caller renders an em-dash on `None` and never
/// treats the absence as an error.
pub fn rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: `sysconf` is a pure lookup of a static system parameter; it takes
    // no pointer and mutates nothing.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return None;
    }
    resident_pages.checked_mul(page_size as u64)
}
