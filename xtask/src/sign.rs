use anyhow::Result;
use std::path::PathBuf;

pub fn sign_and_notarize(_identity: &str) -> Result<PathBuf> {
    anyhow::bail!("unimplemented: sign::sign_and_notarize")
}

pub fn verify(_macos: bool, _linux: bool) -> Result<()> {
    anyhow::bail!("unimplemented: sign::verify")
}
