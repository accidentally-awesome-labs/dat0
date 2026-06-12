//! Confirm-discard dialog for live re-import when the active tab carries
//! rowid-keyed edits (P7c D3).
//!
//! Re-importing a source file re-CTASes the base table, which regenerates the
//! `__dat0_rowid` surrogate — so in-place cell edits (`Edit`) and row deletions
//! (`RowDelete`) cannot be replayed and are discarded. The structural transforms
//! (filters, sorts, column projection) are kept. This dialog gates that discard
//! behind an explicit confirmation.
//!
//! Mirrors [`crate::workspace_in_use_modal::open_conflict_dialog`] exactly (the
//! P7b-proven `gpui_component::dialog::Dialog` pattern): reach a `&mut Window`
//! from `&mut App` via `cx.active_window()` + `handle.update`, then
//! `window.open_dialog(...)` with a fluent `.confirm()` builder. The body text is
//! set via `ParentElement::child`, so that trait must be in scope.

use std::rc::Rc;

use gpui::{AnyView, App, ParentElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::dialog::{Dialog, DialogButtonProps};

/// Show the confirm-discard dialog. `on_confirm` runs (on the main thread, with
/// `&mut App`) if the user chooses "Refresh anyway"; Cancel just closes.
///
/// `body` is the already-interpolated explanation (edit/delete counts), built by
/// the caller so this helper stays free of `split_replayable` coupling.
pub fn confirm_discard<F>(cx: &mut App, body: String, on_confirm: F)
where
    F: Fn(&mut App) + 'static,
{
    let title = dat0_i18n::t("livedata.refresh.confirm.title");
    let ok = dat0_i18n::t("livedata.refresh.confirm.continue");
    let cancel = dat0_i18n::t("common.cancel");
    let cb = Rc::new(on_confirm);
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
                        true // close the dialog after running the re-import
                    })
                    .on_cancel(move |_ev, _window, _cx| true)
            });
        });
    } else {
        tracing::warn!("confirm_discard: no active window; cannot show refresh dialog");
    }
}
