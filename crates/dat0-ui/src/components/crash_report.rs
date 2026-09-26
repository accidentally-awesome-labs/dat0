//! The crash / bug-report panel.
//!
//! # The side-effect contract
//!
//! This is a privacy surface, so its two exits are exact and neither is
//! allowed to drift:
//!
//! * **Send**    → submit (`submit_staged` for a prior-run crash,
//!   `submit_report` for a user-initiated bug report) → `clear_staged` → a
//!   banner saying whether it went → close.
//! * **Dismiss** → `clear_staged` → close. **Nothing is transmitted.**
//!
//! Both exits clear the staged payload, so a report the user declined can
//! never be picked up and sent by a later launch.
//!
//! Submitting sends only while crash reports are on in the settings, read
//! when Send is pressed. Report a Bug can be chosen while they are off, and
//! Send used to close on a report that went nowhere, saying nothing: the
//! panel now says they are off, and offers no Send (step 5.11a).
//!
//! # Why the panel may not even appear
//!
//! Whether a prior-run crash gets a prompt at all is a *gate*, not a UI
//! decision — see [`on_relaunch`]. Opt-out means discard silently; it does not
//! mean "prompt and then don't send".

use std::path::{Path, PathBuf};

use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::telemetry::crash::{self, StagedCrash};
use dat0_core::telemetry::report_logic::{
    RelaunchAction, ReportKind, dialog_body_key, dialog_title_key, resolve_relaunch_action,
};
use dat0_core::telemetry::{Submission, submission_allowed, submit_report, submit_staged};

use crate::a11y::AccessRole;
use crate::state::Workspace;

/// Dismissable: closing the scrim is the Dismiss path, which is safe — it
/// clears staging and sends nothing. The host must route a scrim dismiss
/// through [`dismiss`] rather than merely unmounting, or the staged payload
/// survives to prompt again next launch.
pub const SCRIM_DISMISSABLE: bool = true;

/// Decide what the relaunch path should do about a prior-run crash, and carry
/// out the discard side effects it implies.
///
/// Returns the payload to prompt with, or `None` when nothing should be shown.
/// The opt-out arm is the important one: a staged crash is deleted from disk
/// and never transmitted, and the user is not asked about it. "Opted out" is
/// answered once, at the settings screen, not again at every relaunch.
pub fn on_relaunch(data_dir: &Path, opt_in: bool) -> Option<StagedCrash> {
    match resolve_relaunch_action(data_dir, opt_in) {
        RelaunchAction::ShowCrash(staged) => Some(staged),
        // A bare marker with no payload (SIGKILL, a native crash) has nothing
        // worth sending, so it is swept rather than surfaced.
        RelaunchAction::DiscardMarkerOnly | RelaunchAction::DiscardOptOut => {
            crash::clear_staged(data_dir);
            None
        }
        RelaunchAction::Nothing => None,
    }
}

/// The header title the modal host should render above [`CrashReport`].
pub fn title(staged: Option<&StagedCrash>) -> String {
    dat0_i18n::t(dialog_title_key(&kind_of(staged)))
}

fn kind_of(staged: Option<&StagedCrash>) -> ReportKind {
    match staged {
        Some(s) => ReportKind::Crash(s.clone()),
        None => ReportKind::Bug,
    }
}

#[derive(Clone, PartialEq, Props)]
pub struct CrashReportProps {
    /// `Some` = a prior run crashed and this payload is staged on disk.
    /// `None` = the user chose "Report a Bug" from the menu.
    #[props(default)]
    pub staged: Option<StagedCrash>,
    /// Where the crash sentinel lives, so both exits can clear staging.
    pub data_dir: PathBuf,
    pub on_close: EventHandler<()>,
}

#[component]
pub fn CrashReport(props: CrashReportProps) -> Element {
    let mut note = use_signal(String::new);
    // A crash is offered only while reports are on (`on_relaunch`); a bug
    // report can be asked for while they are off, and then there is nothing
    // to send. Send asks again either way: Settings is a window of its own,
    // and can turn them off while this is up.
    let off = use_hook(|| props.staged.is_none() && !submission_allowed());
    // The window's banners, to say whether a report went.
    let ws = try_use_context::<Workspace>();

    let staged = props.staged.clone();
    let body = if off {
        dat0_i18n::t("report.dialog.off")
    } else {
        dat0_i18n::t(dialog_body_key(&kind_of(staged.as_ref())))
    };
    let dismiss_label = dat0_i18n::t(if off { "common.close" } else { "common.cancel" });

    let send = {
        let staged = staged.clone();
        let dir = props.data_dir.clone();
        move |_| {
            let text = note.peek().clone();
            let note_opt = (!text.trim().is_empty()).then_some(text);
            let went = submit(staged.as_ref(), note_opt.as_deref());
            crash::clear_staged(&dir);
            if let Some(ws) = ws {
                ws.push_banner(outcome(went));
            }
            props.on_close.call(());
        }
    };

    let dismiss = {
        let dir = props.data_dir.clone();
        move |_| {
            dismiss(&dir);
            props.on_close.call(());
        }
    };

    rsx! {
        div { class: "d0-report", "data-a11y-id": "report",

            p {
                class: "d0-body",
                "data-a11y-id": "report-body",
                role: AccessRole::Label.aria(),
                "aria-label": "{body}",
                "{body}"
            }

            if !off {
                textarea {
                    class: "d0-field d0-report-note",
                    "data-a11y-id": "report-note",
                    "aria-label": dat0_i18n::t("report.dialog.note_placeholder"),
                    placeholder: dat0_i18n::t("report.dialog.note_placeholder"),
                    rows: "4",
                    value: "{note}",
                    oninput: move |e| note.set(e.value()),
                }
            }

            div { class: "d0-report-actions",
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "report-dismiss",
                    role: AccessRole::Button.aria(),
                    "aria-label": "{dismiss_label}",
                    onclick: dismiss,
                    "{dismiss_label}"
                }
                if !off {
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "report-send",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("report.dialog.send"),
                        onclick: send,
                        {dat0_i18n::t("report.dialog.send")}
                    }
                }
            }
        }
    }
}

/// Discard the staged payload without transmitting anything.
///
/// Public because the host's scrim dismiss and Escape are the same exit as the
/// Dismiss button, and an exit that forgets to clear staging re-prompts on the
/// next launch for a report the user already declined.
pub fn dismiss(data_dir: &Path) {
    crash::clear_staged(data_dir);
}

/// Transmit the report, if crash reports are on now.
///
/// Deliberately synchronous, matching the GPUI build: `submit_*` ends in
/// `sentry::flush(5s)`, and moving it to a background thread would let the
/// process exit mid-flush — losing exactly the report the user just chose to
/// send. The accepted cost is that Send can block for up to five seconds.
fn submit(staged: Option<&StagedCrash>, note: Option<&str>) -> Submission {
    match staged {
        Some(s) => submit_staged(s, note),
        None => submit_report(note.unwrap_or("")),
    }
}

/// The banner that says what became of a report the user chose to send.
fn outcome(went: Submission) -> Banner {
    match went {
        Submission::Sent => Banner::info(dat0_i18n::t("report.sent")),
        Submission::Off => Banner::warning_with_body(
            dat0_i18n::t("report.not_sent"),
            dat0_i18n::t("report.not_sent.body"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn staged_crash() -> StagedCrash {
        StagedCrash {
            message: "boom".into(),
            backtrace: "frame".into(),
            version: "0.1.0".into(),
        }
    }

    #[test]
    fn opting_out_discards_the_staged_crash_and_never_prompts() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        crash::mark_running(dir).unwrap();
        crash::write_staged(dir, &staged_crash()).unwrap();

        assert!(on_relaunch(dir, false).is_none(), "opt-out must not prompt");
        assert!(
            !crash::staged_path(dir).exists(),
            "opt-out must delete the payload, not keep it for a later launch"
        );
    }

    #[test]
    fn opting_in_prompts_with_the_staged_payload_and_keeps_it_until_the_user_answers() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        crash::mark_running(dir).unwrap();
        crash::write_staged(dir, &staged_crash()).unwrap();

        assert_eq!(on_relaunch(dir, true), Some(staged_crash()));
        assert!(
            crash::staged_path(dir).exists(),
            "the payload is the dialog's content; clearing happens on the user's answer"
        );
    }

    #[test]
    fn a_marker_with_no_payload_is_swept_rather_than_surfaced() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        crash::mark_running(dir).unwrap();

        assert!(on_relaunch(dir, true).is_none());
    }

    #[test]
    fn a_clean_launch_does_nothing_at_all() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir.join("sub")).unwrap();

        assert!(on_relaunch(dir, true).is_none());
        assert!(on_relaunch(dir, false).is_none());
    }

    #[test]
    fn the_banner_says_whether_the_report_went() {
        assert_eq!(outcome(Submission::Sent).title, dat0_i18n::t("report.sent"));
        let off = outcome(Submission::Off);
        assert_eq!(off.title, dat0_i18n::t("report.not_sent"));
        assert_eq!(off.body, dat0_i18n::t("report.not_sent.body"));
    }

    #[test]
    fn the_two_kinds_use_different_copy() {
        assert_ne!(title(None), title(Some(&staged_crash())));
    }
}
