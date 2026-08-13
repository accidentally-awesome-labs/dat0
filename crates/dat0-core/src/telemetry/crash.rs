//! Crash sentinel + staged-crash payload.
//!
//! `running.marker` is created at boot and removed at clean shutdown; a marker
//! that survives into the next launch means the prior run exited abnormally.
//! `last-crash.json` carries a redacted panic payload staged by the panic hook
//! (see `mod.rs`), submitted on relaunch with the user's note.

use crate::telemetry::redaction::redact_text_pub;
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

/// Build a redacted [`StagedCrash`] from a live panic. Pure (no I/O), so the
/// panic hook stays trivial and this is unit-testable.
pub fn payload_from_panic(info: &std::panic::PanicHookInfo<'_>, version: &str) -> StagedCrash {
    let raw_msg = info.to_string();
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    StagedCrash {
        message: redact_text_pub(&raw_msg),
        backtrace: redact_text_pub(&backtrace),
        version: version.to_string(),
    }
}

/// Install a process-wide panic hook that stages a redacted crash payload, then
/// chains to the previously-installed hook (preserving default abort/print).
pub fn install_panic_hook(data_dir: PathBuf) {
    let prev = std::panic::take_hook();
    let version = env!("CARGO_PKG_VERSION").to_string();
    std::panic::set_hook(Box::new(move |info| {
        let payload = payload_from_panic(info, &version);
        let _ = write_staged(&data_dir, &payload);
        prev(info);
    }));
}
