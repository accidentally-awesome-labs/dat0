//! Pure logic for the crash/bug report dialogs — the headless-testable seam
//! under the GPUI modal in `view/crash_report.rs`.

use crate::telemetry::crash::StagedCrash;

pub enum ReportKind {
    Crash(StagedCrash),
    Bug,
}

/// Only prompt to send a prior-run crash when one was detected AND the user has
/// opted in. Opt-out means: silently discard, never prompt.
pub fn should_prompt(prior_crash: bool, opt_in: bool) -> bool {
    prior_crash && opt_in
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
}
