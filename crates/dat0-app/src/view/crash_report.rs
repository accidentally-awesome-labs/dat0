//! Crash / bug-report modal.
//!
//! `open_report` reaches the active window, creates a note `InputState`, and
//! opens a gpui-component `Dialog` with Send / Dismiss actions.
//!
//! Side-effect contract (checked by the T7 opus review):
//!  - **Send**    → submit (submit_staged or submit_report) → clear_staged → close.
//!  - **Dismiss** → clear_staged → close. Never submits.

use crate::telemetry::report_logic::{ReportKind, dialog_body_key, dialog_title_key};
use crate::telemetry::{self, crash};
// Content seam for the headless UAT harness (`A11ySnapshot::capture`): the plain
// `.child(body)` text is AccessKit-invisible, so under `a11y-capture` the body is
// wrapped in a `div` carrying an `.a11y_label`. `cfg`-SELECTED (not an
// unconditional wrapper) so the release element tree is byte-identical to the
// pre-seam markup — no inert wrapper ships, no human visual glance owed.
#[cfg(feature = "a11y-capture")]
use crate::a11y::{A11yExt as _, AccessRole};
use gpui::{AnyView, App, AppContext as _, ParentElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::dialog::{Dialog, DialogButtonProps};
use gpui_component::input::{Input, InputState};
use std::path::PathBuf;
use std::rc::Rc;

/// Open the crash/bug-report modal from `&mut App`.
///
/// Presents a dialog with a free-form note field and two buttons:
///  - **Send** — reads the note field, calls `submit_staged` or `submit_report`,
///    then `crash::clear_staged`, and closes.
///  - **Dismiss** — calls `crash::clear_staged` and closes. Nothing is submitted.
///
/// # UI-thread flush warning
/// `submit_staged` / `submit_report` call `sentry::flush(5s)` synchronously.
/// For v1 this is accepted; a Send click may block the UI for up to five
/// seconds. A background-thread submit path is tracked as a future improvement.
pub fn open_report(cx: &mut App, kind: ReportKind, data_dir: PathBuf) {
    let title = dat0_i18n::t(dialog_title_key(&kind));
    let body = dat0_i18n::t(dialog_body_key(&kind));
    let send = dat0_i18n::t("report.dialog.send");

    // Wrap in Rc so the values can be cloned into the `Fn` (not `FnOnce`) builder
    // each time gpui calls it — mirrors the `cb` pattern in workspace_in_use_modal.rs.
    let kind = Rc::new(kind);
    let data_dir = Rc::new(data_dir);

    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, move |_root: AnyView, window: &mut Window, cx| {
            // Create the InputState before the builder so it can be referenced
            // by value (for Input::new) and by clone (for on_ok).
            let placeholder = dat0_i18n::t("report.dialog.note_placeholder");
            let note = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));

            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                // Per-call clones — required because the builder closure is `Fn`.
                let kind_c = kind.clone();
                let data_dir_ok = data_dir.clone();
                let data_dir_cancel = data_dir.clone();
                let note_c = note.clone(); // for on_ok read; note is also used by-ref below

                let dialog = dialog.title(title.clone());
                // Body child: plain text in release, an `.a11y_label`-carrying
                // `div` (same body string, same render condition) in test builds.
                #[cfg(feature = "a11y-capture")]
                let dialog = dialog.child(
                    gpui::div()
                        .child(body.clone())
                        .a11y_label(AccessRole::Label, body.clone()),
                );
                #[cfg(not(feature = "a11y-capture"))]
                let dialog = dialog.child(body.clone());
                dialog
                    .child(Input::new(&note)) // borrows the closure-owned note entity
                    .confirm()
                    .button_props(DialogButtonProps::default().ok_text(send.clone()))
                    .on_cancel(move |_ev, _w, _cx| {
                        // Dismiss: discard staged crash, submit nothing.
                        crash::clear_staged(&data_dir_cancel);
                        true
                    })
                    .on_ok(move |_ev, _w, cx| {
                        // Send: read note → optional text → submit → clear staging.
                        let text = note_c.read(cx).value().to_string();
                        let note_opt = (!text.trim().is_empty()).then(|| text.clone());
                        match kind_c.as_ref() {
                            ReportKind::Crash(staged) => {
                                telemetry::submit_staged(staged, note_opt.as_deref());
                            }
                            ReportKind::Bug => {
                                telemetry::submit_report(note_opt.as_deref().unwrap_or(""));
                            }
                        }
                        crash::clear_staged(&data_dir_ok);
                        true
                    })
            });
        });
    } else {
        tracing::warn!("crash_report: no active window; cannot show report modal");
    }
}
