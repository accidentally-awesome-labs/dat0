//! The "workspace in use" gate. Maps an `AcquireOutcome` (+ in-process state)
//! to a `ModalDecision`, and renders the cross-machine warning via
//! gpui-component's `Dialog`. Force-unlock is v1.x — we only warn.

use std::rc::Rc;

use gpui::{AnyView, App, ParentElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::dialog::{Dialog, DialogButtonProps};

use crate::workspace::lock_manifest::{AcquireOutcome, LockManifest};

/// What the open flow should do, decided purely (no GPUI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalDecision {
    /// Proceed to open + claim the manifest (Available | Reclaimable).
    Proceed,
    /// Another in-process window already has it → offer focus-existing.
    SameMachineInUse,
    /// Foreign machine holds a live record → warn (Cancel / Open anyway).
    ConflictForeign(LockManifest),
    /// Same machine, live pid, but not in our registry (shouldn't happen under
    /// single-instance) → block conservatively.
    BlockSameMachine,
}

/// `in_process` = the window_registry already lists this folder (this process).
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

/// Humanize an age in seconds — dep-free (no chrono/tz). Display-only.
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

/// Body text for the cross-machine warning (pure → testable). `now_secs` is the
/// current unix time; `h.started_at` is epoch-secs-as-string (P7a `now` convention).
/// A non-numeric `started_at` degrades to "just now".
pub fn conflict_body(h: &LockManifest, now_secs: i64) -> String {
    let started = h.started_at.parse::<i64>().unwrap_or(now_secs);
    let age = humanize_age(now_secs - started);
    dat0_i18n::t("workspace.in_use.conflict.body")
        .replace("{host}", &h.hostname)
        .replace("{age}", &age)
}

/// Show the cross-machine conflict dialog. `on_open_anyway` runs if the user
/// chooses "Open anyway"; Cancel just closes. T0 §7.4 confirmed reaching a
/// `Window` from `&mut App` via `cx.active_window()` + `handle.update`, and the
/// `WindowExt::open_dialog` + `ParentElement::child` body pattern.
pub fn open_conflict_dialog<F>(cx: &mut App, holder: LockManifest, on_open_anyway: F)
where
    F: Fn(&mut App) + 'static,
{
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let body = conflict_body(&holder, now);
    let title = dat0_i18n::t("workspace.in_use.conflict.title");
    let ok = dat0_i18n::t("workspace.in_use.open_anyway");
    let cancel = dat0_i18n::t("common.cancel");
    let cb = Rc::new(on_open_anyway);
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, move |_root: AnyView, window: &mut Window, cx| {
            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                let cb = Rc::clone(&cb);
                dialog
                    .title(title.clone())
                    .confirm()
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text(ok.clone())
                            .cancel_text(cancel.clone()),
                    )
                    .child(body.clone()) // body text — ParentElement must be in scope
                    .on_ok(move |_ev, _window, cx| {
                        (cb)(cx);
                        true // close the dialog
                    })
                    .on_cancel(move |_ev, _window, _cx| true)
            });
        });
    } else {
        tracing::warn!("open_conflict_dialog: no active window; cannot show modal");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foreign() -> LockManifest {
        LockManifest {
            pid: 1,
            hostname: "salar-mbp".into(),
            started_at: "2026-06-11T10:04:00Z".into(),
            dat0_version: "0.1.0".into(),
            tombstoned: false,
        }
    }

    #[test]
    fn in_process_always_offers_focus() {
        assert_eq!(
            decide(&AcquireOutcome::Available, true),
            ModalDecision::SameMachineInUse
        );
    }

    #[test]
    fn available_proceeds() {
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
    fn foreign_conflicts() {
        assert_eq!(
            decide(&AcquireOutcome::ConflictForeign(foreign()), false),
            ModalDecision::ConflictForeign(foreign())
        );
    }

    #[test]
    fn humanize_age_buckets() {
        assert_eq!(humanize_age(10), "just now");
        assert_eq!(humanize_age(120), "2 minute(s) ago");
        assert_eq!(humanize_age(7200), "2 hour(s) ago");
        assert_eq!(humanize_age(172_800), "2 day(s) ago");
    }

    #[test]
    fn conflict_body_interpolates_host_and_age() {
        // The i18n keys are added in THIS task (Step 2b), so the template resolves.
        let mut h = foreign();
        h.started_at = "0".into(); // deterministic epoch-secs start
        let body = conflict_body(&h, 7200);
        assert!(body.contains("salar-mbp"), "body: {body}");
        assert!(body.contains("2 hour(s) ago"), "body: {body}");
    }
}
