//! The "workspace is already open" gate.
//!
//! Two different problems wear this surface, and conflating them would be a
//! correctness bug, not a copy one:
//!
//! * **Same machine** — another window of *this* process already has the
//!   folder. There is nothing to warn about; the right move is to focus the
//!   window that has it.
//! * **Foreign machine** — a live, non-tombstoned `lock.json` written by some
//!   other host, usually reached through a sync drive. Editing the same
//!   workspace from two machines can corrupt it. dat0 **warns and does not
//!   auto-resolve**: there is no force-unlock, and "Open anyway" is the user
//!   accepting the risk explicitly.
//!
//! # This one does not close by itself
//!
//! Unlike every other modal here, dismissing is not a safe default. Both
//! outcomes — proceed, or don't — are decisions with consequences, so the
//! scrim is inert and there is no ✕. See [`SCRIM_DISMISSABLE`].

use dioxus::prelude::*;

use dat0_core::workspace::lock_manifest::{AcquireOutcome, LockManifest};

use crate::a11y::AccessRole;

/// **False, and load-bearing.** A click on the scrim, an Escape, or a header ✕
/// would all have to mean one of Cancel or Open-anyway, and neither is a safe
/// thing to infer from a stray click. The host must render this one with an
/// inert scrim and no close affordance; the two buttons are the only exits.
pub const SCRIM_DISMISSABLE: bool = false;

/// What the open flow should do about a workspace it is trying to claim.
///
/// Pure — decided from the lock outcome plus whether this process's own window
/// registry already lists the folder — so the branch that decides between
/// "warn" and "just focus the other window" is testable without a UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalDecision {
    /// No live holder: open and claim.
    Proceed,
    /// Another window in this process has it: offer focus-existing.
    SameMachineInUse,
    /// A foreign machine holds a live record: warn.
    ConflictForeign(LockManifest),
    /// Same machine, live pid, but not in our registry. Should not happen
    /// under single-instance, so it blocks conservatively rather than
    /// guessing.
    BlockSameMachine,
}

/// `in_process` = this process's window registry already lists the folder.
pub fn decide(outcome: &AcquireOutcome, in_process: bool) -> ModalDecision {
    if in_process {
        return ModalDecision::SameMachineInUse;
    }
    match outcome {
        AcquireOutcome::Available | AcquireOutcome::Reclaimable => ModalDecision::Proceed,
        AcquireOutcome::ConflictForeign(h) => ModalDecision::ConflictForeign(h.clone()),
        AcquireOutcome::HeldSameMachine(_) => ModalDecision::BlockSameMachine,
    }
}

/// Humanise an age in seconds.
///
/// Dependency-free on purpose: no chrono, no timezone database. This is
/// display copy beside a hostname, not a timestamp anyone computes with.
pub fn humanize_age(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        "just now".to_string()
    } else if s < 3600 {
        format!("{} minute(s) ago", s / 60)
    } else if s < 86_400 {
        format!("{} hour(s) ago", s / 3600)
    } else {
        format!("{} day(s) ago", s / 86_400)
    }
}

/// The cross-machine warning body.
///
/// `h.started_at` is epoch-seconds-as-string. A value that does not parse
/// degrades to "just now" rather than showing a wrong age — an age is the
/// weaker half of this warning and the hostname carries it either way.
pub fn conflict_body(h: &LockManifest, now_secs: i64) -> String {
    let started = h.started_at.parse::<i64>().unwrap_or(now_secs);
    let age = humanize_age(now_secs - started);
    dat0_i18n::t("workspace.in_use.conflict.body")
        .replace("{host}", &h.hostname)
        .replace("{age}", &age)
}

/// Which of the two gates is being shown.
#[derive(Clone, PartialEq, Debug)]
pub enum InUse {
    /// A live holder on another machine.
    Conflict {
        holder: LockManifest,
        /// Unix seconds, passed in so the rendered age is deterministic.
        now_secs: i64,
    },
    /// Another window of this process.
    SameMachine,
}

impl InUse {
    /// A conflict against the current wall clock.
    pub fn conflict(holder: LockManifest) -> Self {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        InUse::Conflict { holder, now_secs }
    }
}

/// The header title the modal host should render above [`WorkspaceInUse`].
pub fn title(kind: &InUse) -> String {
    match kind {
        InUse::Conflict { .. } => dat0_i18n::t("workspace.in_use.conflict.title"),
        InUse::SameMachine => dat0_i18n::t("workspace.in_use.same_machine.title"),
    }
}

#[derive(Clone, PartialEq, Props)]
pub struct WorkspaceInUseProps {
    pub kind: InUse,
    /// Open anyway (conflict) / focus the existing window (same machine).
    pub on_proceed: EventHandler<()>,
    /// Do not open. The workspace is left exactly as it was.
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn WorkspaceInUse(props: WorkspaceInUseProps) -> Element {
    // The same-machine case is title-only in the GPUI build: there is no age
    // and no host to report, and inventing a body would imply a risk that is
    // not there.
    let body = match &props.kind {
        InUse::Conflict { holder, now_secs } => Some(conflict_body(holder, *now_secs)),
        InUse::SameMachine => None,
    };
    let proceed = match &props.kind {
        InUse::Conflict { .. } => dat0_i18n::t("workspace.in_use.open_anyway"),
        InUse::SameMachine => dat0_i18n::t("workspace.in_use.focus_existing"),
    };

    rsx! {
        div {
            class: "d0-confirm",
            "data-a11y-id": "workspace-in-use",
            role: AccessRole::Alert.aria(),
            "aria-label": title(&props.kind),

            if let Some(body) = body {
                p {
                    class: "d0-body",
                    "data-a11y-id": "workspace-in-use-body",
                    "aria-label": "{body}",
                    "{body}"
                }
            }

            div { class: "d0-confirm-actions",
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "workspace-in-use-cancel",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("common.cancel"),
                    onclick: move |_| props.on_cancel.call(()),
                    {dat0_i18n::t("common.cancel")}
                }
                button {
                    class: "d0-btn is-primary",
                    "data-a11y-id": "workspace-in-use-proceed",
                    role: AccessRole::Button.aria(),
                    "aria-label": "{proceed}",
                    onclick: move |_| props.on_proceed.call(()),
                    "{proceed}"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holder(host: &str, started_at: &str) -> LockManifest {
        LockManifest {
            pid: 4242,
            hostname: host.to_string(),
            started_at: started_at.to_string(),
            dat0_version: "0.1.0".to_string(),
            tombstoned: false,
        }
    }

    #[test]
    fn an_in_process_window_never_reaches_the_cross_machine_warning() {
        let h = holder("other-mac", "0");
        assert_eq!(
            decide(&AcquireOutcome::ConflictForeign(h), true),
            ModalDecision::SameMachineInUse,
            "our own window takes precedence over the lock file"
        );
    }

    #[test]
    fn a_dead_or_absent_holder_opens_without_a_prompt() {
        assert_eq!(
            decide(&AcquireOutcome::Available, false),
            ModalDecision::Proceed
        );
        assert_eq!(
            decide(&AcquireOutcome::Reclaimable, false),
            ModalDecision::Proceed
        );
    }

    #[test]
    fn a_live_same_machine_holder_outside_our_registry_blocks() {
        let h = holder("this-mac", "0");
        assert_eq!(
            decide(&AcquireOutcome::HeldSameMachine(h), false),
            ModalDecision::BlockSameMachine
        );
    }

    #[test]
    fn the_conflict_body_names_the_host_and_the_age() {
        let b = conflict_body(&holder("studio-imac", "1000"), 1000 + 7200);
        assert!(b.contains("studio-imac"), "{b}");
        assert!(b.contains("2 hour(s) ago"), "{b}");
        assert!(!b.contains("{host}") && !b.contains("{age}"), "{b}");
    }

    #[test]
    fn an_unparseable_start_time_degrades_to_just_now() {
        let b = conflict_body(&holder("nas", "not-a-number"), 1_700_000_000);
        assert!(b.contains("just now"), "{b}");
    }

    #[test]
    fn age_buckets() {
        assert_eq!(humanize_age(-5), "just now");
        assert_eq!(humanize_age(59), "just now");
        assert_eq!(humanize_age(60), "1 minute(s) ago");
        assert_eq!(humanize_age(3600), "1 hour(s) ago");
        assert_eq!(humanize_age(86_400), "1 day(s) ago");
    }
}
