//! Pure logic for the crash/bug report dialogs — the headless-testable seam
//! under the GPUI modal in `view/crash_report.rs`.

use crate::telemetry::crash::{self, StagedCrash};
use std::path::Path;

pub enum ReportKind {
    Crash(StagedCrash),
    Bug,
}

/// Only prompt to send a prior-run crash when one was detected AND the user has
/// opted in. Opt-out means: silently discard, never prompt.
pub fn should_prompt(prior_crash: bool, opt_in: bool) -> bool {
    prior_crash && opt_in
}

/// What the relaunch path should do with the on-disk crash sentinel state.
///
/// This is the headless-testable seam under the GPUI relaunch closure in
/// `run_app` (window.rs): the closure reads `opt_in` from settings and then
/// defers ALL of the branching to [`resolve_relaunch_action`], so the
/// privacy-critical composition (especially the opt-out → discard path) is unit
/// tested here rather than only reachable by driving a cold start.
#[derive(Debug, PartialEq)]
pub enum RelaunchAction {
    /// Opted in, prior crash, staged payload present → show the crash dialog.
    ShowCrash(StagedCrash),
    /// Opted in, prior crash, but no staged payload (SIGKILL / native crash) →
    /// discard the bare marker; the minimal-report path is v1.x / UAT-only.
    DiscardMarkerOnly,
    /// Opted out → discard any staged data unconditionally, never prompt/transmit.
    DiscardOptOut,
    /// Opted in with no prior crash (a clean launch) → do nothing. Unreachable in
    /// production because `CrashGuard::arm` sets the marker before `run_app`, but
    /// modeled explicitly so the match is total.
    Nothing,
}

/// Decide the relaunch action from the on-disk sentinel state in `dir` and the
/// persisted `opt_in` flag. Pure of GPUI (reads only the sentinel files), so the
/// whole gate — including the opt-out discard guarantee — is unit-testable.
///
/// Preserves the exact side-effect contract of the former inline closure: both
/// discard arms leave the caller to `clear_staged(dir)`; only `ShowCrash` opens a
/// dialog.
pub fn resolve_relaunch_action(dir: &Path, opt_in: bool) -> RelaunchAction {
    let prior = crash::prior_crash_detected(dir);
    if should_prompt(prior, opt_in) {
        match crash::read_staged(dir) {
            Some(staged) => RelaunchAction::ShowCrash(staged),
            None => RelaunchAction::DiscardMarkerOnly,
        }
    } else if !opt_in {
        RelaunchAction::DiscardOptOut
    } else {
        RelaunchAction::Nothing
    }
}

pub fn dialog_title_key(kind: &ReportKind) -> &'static str {
    match kind {
        ReportKind::Crash(_) => "crash.dialog.title",
        ReportKind::Bug => "report.dialog.title",
    }
}

pub fn dialog_body_key(kind: &ReportKind) -> &'static str {
    match kind {
        ReportKind::Crash(_) => "crash.dialog.body",
        ReportKind::Bug => "report.dialog.body",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::crash::StagedCrash;

    #[test]
    fn prompts_only_when_crash_and_opt_in() {
        assert!(should_prompt(true, true));
        assert!(!should_prompt(true, false));
        assert!(!should_prompt(false, true));
        assert!(!should_prompt(false, false));
    }

    #[test]
    fn kind_selects_distinct_keys() {
        let crash = ReportKind::Crash(StagedCrash {
            message: "m".into(),
            backtrace: "b".into(),
            version: "0".into(),
        });
        assert_ne!(dialog_title_key(&crash), dialog_title_key(&ReportKind::Bug));
    }

    fn sample() -> StagedCrash {
        StagedCrash {
            message: "boom".into(),
            backtrace: "bt".into(),
            version: "9.9.9".into(),
        }
    }

    /// Privacy-critical (§3): opted OUT with a fully-staged crash (marker + payload)
    /// must NEVER resolve to `ShowCrash` — it discards.
    #[test]
    fn opt_out_never_shows_even_with_staged_payload() {
        let dir = tempfile::tempdir().unwrap();
        crash::mark_running(dir.path()).unwrap();
        crash::write_staged(dir.path(), &sample()).unwrap();
        assert_eq!(
            resolve_relaunch_action(dir.path(), false),
            RelaunchAction::DiscardOptOut
        );
    }

    /// Opted in, prior crash, staged payload present → show the dialog with the
    /// exact staged payload.
    #[test]
    fn opt_in_with_marker_and_staged_shows_crash() {
        let dir = tempfile::tempdir().unwrap();
        crash::mark_running(dir.path()).unwrap();
        crash::write_staged(dir.path(), &sample()).unwrap();
        assert_eq!(
            resolve_relaunch_action(dir.path(), true),
            RelaunchAction::ShowCrash(sample())
        );
    }

    /// Opted in, marker present but no staged JSON (SIGKILL / native) → discard
    /// the bare marker, do not prompt.
    #[test]
    fn opt_in_with_marker_no_staged_discards_marker_only() {
        let dir = tempfile::tempdir().unwrap();
        crash::mark_running(dir.path()).unwrap();
        assert_eq!(
            resolve_relaunch_action(dir.path(), true),
            RelaunchAction::DiscardMarkerOnly
        );
    }

    /// Opted in, no prior crash (clean dir) → nothing to do.
    #[test]
    fn opt_in_no_marker_is_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_relaunch_action(dir.path(), true),
            RelaunchAction::Nothing
        );
    }
}
