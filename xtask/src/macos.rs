use anyhow::Result;
use std::path::PathBuf;

pub fn bundle(_version: &str, _git_sha: &str) -> Result<PathBuf> {
    anyhow::bail!("unimplemented: macos::bundle")
}
