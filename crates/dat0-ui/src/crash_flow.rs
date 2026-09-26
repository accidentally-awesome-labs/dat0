//! The crash report offered at relaunch, and Help → Report a Bug (PD-023,
//! step 5.7d).
//!
//! The report panel was built (`components::crash_report`), with its privacy
//! contract and its relaunch gate (`crash_report::on_relaunch`), and nothing
//! called either: a crashed run's staged report was never offered at the next
//! launch, and no menu item or command opened a bug report. The GPUI build
//! offered the report once, in the first window, after launch.

use std::path::PathBuf;

use dioxus::prelude::*;

use dat0_core::telemetry::crash::StagedCrash;

use crate::components::crash_report;
use crate::state::{Modal, Workspace};

/// Offer the prior run's crash report, once per process, in the first
/// window. Only when the user opted in to crash reports: otherwise a staged
/// report is deleted unsent and nothing is asked (`crash_report::on_relaunch`).
pub fn use_relaunch_offer(ws: Workspace) {
    use_hook(move || {
        static OFFERED: std::sync::Once = std::sync::Once::new();
        OFFERED.call_once(|| {
            let Some(dir) = data_dir() else {
                return;
            };
            if let Some(staged) = crash_report::on_relaunch(&dir, opted_in()) {
                offer(ws, dir, staged);
            }
        });
    });
}

/// Put the crash report in the modal slot. Another dialog up keeps its place,
/// and the report stays staged for the next launch.
pub fn offer(ws: Workspace, dir: PathBuf, staged: StagedCrash) {
    let mut modal = ws.modal;
    if modal.peek().is_some() {
        return;
    }
    modal.set(Some(Modal::CrashReport {
        staged: Some(staged),
        data_dir: dir,
    }));
}

/// Help → Report a Bug…: the report panel, with nothing staged.
pub fn report_bug(ws: Workspace) {
    let Some(dir) = data_dir() else {
        return;
    };
    let mut modal = ws.modal;
    modal.set(Some(Modal::CrashReport {
        staged: None,
        data_dir: dir,
    }));
}

/// Where the crash guard stages a crashed run's report: the state root the
/// launcher installs, which is the platform's data directory.
fn data_dir() -> Option<PathBuf> {
    dat0_core::globals::state_root()
        .map(std::path::Path::to_path_buf)
        .or_else(|| dat0_core::platform::data_dir().ok())
}

/// Whether the user opted in to crash reports. Off when the settings cannot
/// be read: the privacy-safe answer.
fn opted_in() -> bool {
    dat0_core::platform::config_dir()
        .ok()
        .and_then(|dir| {
            dat0_core::settings::store::SettingsStore::with_path(dir.join("settings.toml"))
                .load_or_default()
                .ok()
        })
        .is_some_and(|s| s.telemetry.crash_submission_enabled)
}
