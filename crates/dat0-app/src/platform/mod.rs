use anyhow::Result;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

pub fn ensure_dir(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}
