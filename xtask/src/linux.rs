use anyhow::Result;
use std::path::PathBuf;

pub fn bundle(_version: &str) -> Result<PathBuf> {
    anyhow::bail!("unimplemented: linux::bundle")
}
