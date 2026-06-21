//! About box: version/build/license/NOTICE + update nudge.
//!
//! Split into a pure, tested `summary_lines` (the text rows) and a GPUI
//! `open` that presents them in a gpui-component `Dialog`, mirroring the
//! modal pattern in `workspace_in_use_modal.rs`. The update line depends on
//! a network result, and gpui-component's `Dialog` is one-shot (its content
//! is fixed when the `open_dialog` closure runs), so `open` shows the box
//! immediately with the "latest version" line, then re-presents with the
//! nudge once the off-thread release check returns newer.

pub mod build_info;

use build_info::BuildInfo;

use gpui::{AnyView, App, ParentElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::dialog::{Dialog, DialogButtonProps};

/// The human-facing GitHub Releases page (NOT the API endpoint) opened by the
/// "Download" button when a newer release is available.
const RELEASES_PAGE_URL: &str =
    "https://github.com/accidentally-awesome-labs/dat0/releases/latest";

/// Pure, testable text rows for the About box. `newer` = Some(tag) when a newer
/// release exists (drives the nudge line).
pub fn summary_lines(b: &BuildInfo, newer: Option<&str>) -> Vec<String> {
    let mut lines = vec![
        dat0_i18n::t("about.title"),
        format!("{} {}", dat0_i18n::t("about.version"), b.version),
        format!("{} {}", dat0_i18n::t("about.build"), b.git_sha),
        format!("{} Apache-2.0", dat0_i18n::t("about.license")),
        dat0_i18n::t("about.acknowledgements"),
    ];
    match newer {
        Some(tag) => lines.push(format!("{} {}", dat0_i18n::t("about.update.available"), tag)),
        None => lines.push(dat0_i18n::t("about.update.current")),
    }
    lines
}

/// Open the About box. Appears instantly with version/build/license/NOTICE and
/// the "latest version" line, then spawns the (blocking, ureq-based) release
/// check off the main thread; if a newer release exists, posts back via the
/// main-thread dispatcher to re-present with the update nudge + Download button.
pub fn open(cx: &mut App) {
    present(cx, None);

    // Off-thread release check (blocking ureq — must not run on the UI thread).
    // On a strictly-newer tag, hop back to the main thread and re-present.
    std::thread::spawn(move || {
        let current = BuildInfo::current().version;
        match crate::update::fetch_latest(crate::update::LATEST_RELEASE_API) {
            Ok(tag) => {
                if crate::update::newer_than(current, &tag) {
                    if let Some(dispatcher) = crate::window_registry::dispatcher() {
                        let _ = dispatcher.dispatch(move |cx: &mut App| present(cx, Some(tag)));
                    }
                }
            }
            Err(e) => tracing::debug!(error = %e, "about: update check failed (non-fatal)"),
        }
    });
}

/// Present the About dialog once. `newer = Some(tag)` adds the nudge line and a
/// "Download" button that opens the Releases page; `None` shows a single OK.
///
/// Mirrors `workspace_in_use_modal::open_conflict_dialog`: reach a `Window` from
/// `&mut App` via `cx.active_window()` + `handle.update`, then `open_dialog` with
/// the body as a `ParentElement::child` (that import must be in scope).
fn present(cx: &mut App, newer: Option<String>) {
    let body = summary_lines(&BuildInfo::current(), newer.as_deref()).join("\n");
    let title = dat0_i18n::t("about.title");
    let download = dat0_i18n::t("about.update.download");

    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, move |_root: AnyView, window: &mut Window, cx| {
            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                let dialog = dialog.title(title.clone()).child(body.clone());
                match newer {
                    // Newer release: confirm() = Cancel + OK; relabel OK "Download"
                    // and open the human Releases page on confirm.
                    Some(_) => dialog
                        .confirm()
                        .button_props(DialogButtonProps::default().ok_text(download.clone()))
                        .on_ok(move |_ev, _window, _cx| {
                            if let Err(e) = crate::platform::open_url(RELEASES_PAGE_URL) {
                                tracing::warn!(error = %e, "about: open releases page failed");
                            }
                            true // close the dialog
                        })
                        .on_cancel(move |_ev, _window, _cx| true),
                    // Up to date: alert() = single OK button, no Download.
                    None => dialog.alert().on_ok(move |_ev, _window, _cx| true),
                }
            });
        });
    } else {
        tracing::warn!("about::open: no active window; cannot show modal");
    }
}
