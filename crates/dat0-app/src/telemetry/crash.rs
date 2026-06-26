//! Crash sentinel + staged-crash payload.
//!
//! `running.marker` is created at boot and removed at clean shutdown; a marker
//! that survives into the next launch means the prior run exited abnormally.
//! `last-crash.json` carries a redacted panic payload staged by the panic hook
//! (see `mod.rs`), submitted on relaunch with the user's note.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct StagedCrash {
    pub message: String,
    pub backtrace: String,
    pub version: String,
}

pub fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("running.marker")
}

pub fn staged_path(data_dir: &Path) -> PathBuf {
    data_dir.join("last-crash.json")
}

pub fn mark_running(data_dir: &Path) -> std::io::Result<()> {
    std::fs::write(marker_path(data_dir), b"1")
}

pub fn clear_running(data_dir: &Path) {
    let _ = std::fs::remove_file(marker_path(data_dir));
}

pub fn prior_crash_detected(data_dir: &Path) -> bool {
    marker_path(data_dir).exists()
}

pub fn write_staged(data_dir: &Path, crash: &StagedCrash) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(crash).map_err(std::io::Error::other)?;
    std::fs::write(staged_path(data_dir), json)
}

pub fn read_staged(data_dir: &Path) -> Option<StagedCrash> {
    let bytes = std::fs::read(staged_path(data_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn clear_staged(data_dir: &Path) {
    let _ = std::fs::remove_file(staged_path(data_dir));
}
