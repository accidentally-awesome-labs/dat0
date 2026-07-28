//! First-run onboarding: the skip-able 7-panel tour carousel (design D2/D3).
//!
//! ## Modal mechanism (T0 spike decision — Approach A)
//!
//! Use Approach A — the one-shot gpui-component `Dialog` re-presented per panel.
//! The 7-panel tour is built as a single `present_panel(window, cx, index)` fn
//! that calls `window.open_dialog(...)` with a `Dialog` whose `.child(...)` body
//! contains the illustration + headline + body + dot pager plus a custom
//! Back/Next/Skip button row. Each button's `.on_click` receives `&mut Window` +
//! `&mut App` directly, so it advances by calling `window.close_dialog(cx)`
//! (pops the current panel) **then** `present_panel(window, cx, index±1)`
//! (pushes the next). `open_dialog` **STACKS** (`active_dialogs.push`, renders
//! each layer at a 16px offset) — it does NOT replace — so a carousel MUST
//! explicitly `close_dialog` before re-presenting, otherwise the panels pile up.
//! That close-then-open is the load-bearing detail. `close_dialog` pops only the
//! last dialog. This mirrors the `about::present` reach-from-`App` precedent
//! (`cx.active_window()` + `handle.update`) and needs no stateful `Entity`, no
//! `cx.notify()` plumbing, and no `WindowRegistry` hop. Programmatic dismiss =
//! `WindowExt::close_dialog`.

pub mod panels;

use gpui::{
    AnyView, App, ClickEvent, Image, ImageFormat, ImageSource, ParentElement as _, Styled as _,
    Window, div, img, px,
};
use gpui_component::WindowExt as _;
use gpui_component::button::Button;
use gpui_component::dialog::Dialog;
use gpui_component::{h_flex, v_flex};
use std::sync::Arc;

// UAT Gap 2: `.a11y(id, role, label)` / `.a11y_label(role, text)` annotate the
// carousel so the headless harness can assert WHICH panel headline is rendered
// and click carousel buttons BY LABEL (retiring the hard-coded pixel constant).
// Under `a11y-capture` these emit AccessKit nodes (+ `debug_selector` for the
// clickable buttons); in release builds they are identity no-ops (D-015 stays
// open — this is test instrumentation, not production accessibility).
use crate::a11y::{A11yExt as _, AccessRole};
use crate::theme::tokens::Dat0Theme as _;
use gpui_component::ActiveTheme as _;

use panels::{PANELS, back, is_last, next};

/// Open the first-run tour carousel at panel 0. Reached from `&mut App` via the
/// `about::present` precedent (`cx.active_window()` + `handle.update`). The
/// per-panel chrome (Back/Next/Skip) drives navigation from inside the dialog
/// body, so only this first panel needs the `App`→`Window` hop.
pub fn open(cx: &mut App) {
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, move |_root: AnyView, window: &mut Window, cx| {
            present_panel(window, cx, 0);
        });
    } else {
        tracing::warn!("onboarding::open: no active window; cannot show tour");
    }
}

/// Open the tour from a caller that may already be INSIDE a `window.update` of
/// the active window (gpui action-dispatch and mouse-event dispatch both run
/// inside one).
///
/// `open` does `cx.active_window().update(..)`. When that runs synchronously
/// from within the active window's own update, the window is already "taken"
/// out of its slot, so the re-entrant update returns `Err`, which `open`
/// swallows (`let _ = handle.update(..)`) — a silent no-op. That is exactly
/// why "Help → Take a Tour" and the hero "Take a tour" button used to do
/// nothing while the auto-show worked: the auto-show path hops the
/// process-global [`MainThreadDispatcher`](crate::main_bridge::MainThreadDispatcher),
/// which re-runs the open from a plain `App` context AFTER the current frame —
/// no active-window borrow held. The manual re-entry points share that hazard,
/// so they go through here, taking the same proven hop. Falls back to a direct
/// `open` when no dispatcher is installed (e.g. unit tests, or any caller that
/// is NOT inside a window-update — `open` is correct there).
pub fn open_deferred(cx: &mut App) {
    if let Some(dispatcher) = crate::window_registry::dispatcher() {
        let _ = dispatcher.dispatch(|cx: &mut App| open(cx));
    } else {
        open(cx);
    }
}

/// Persist `first_run_done = true` so the tour never auto-shows again. Logs (does
/// NOT panic) on any settings-store error — failing to set the flag must not take
/// down the UI; worst case the user sees the tour once more next launch.
fn mark_first_run_done() {
    let store = match crate::platform::config_dir() {
        Ok(dir) => crate::settings::store::SettingsStore::with_path(dir.join("settings.toml")),
        Err(e) => {
            tracing::warn!(error = %e, "onboarding: config_dir unavailable; first_run_done not set");
            return;
        }
    };
    if let Err(e) = crate::settings::set_first_run_done(&store, true) {
        tracing::warn!(error = %e, "onboarding: persisting first_run_done failed");
    }
}

/// Present tour panel `index`. The body's Back/Next/Skip buttons close-then-
/// re-present to advance, so exactly one dialog is ever on the stack. Skipping
/// and finishing both `mark_first_run_done()` then `close_dialog`.
fn present_panel(window: &mut Window, cx: &mut App, index: usize) {
    let panel = &PANELS[index];
    let title = dat0_i18n::t(panel.title_key);
    let body = dat0_i18n::t(panel.body_key);
    // PNG bytes → gpui Image (decoded lazily by the asset cache on render).
    let image: Arc<Image> = Arc::new(Image::from_bytes(ImageFormat::Png, panel.image.to_vec()));
    let last = is_last(index);
    let show_back = index > 0;

    window.open_dialog(cx, move |dialog: Dialog, _w, dialog_cx| {
        // Skip: always available, bottom-left. Dismisses + marks done.
        let skip = Button::new("tour-skip")
            .label(dat0_i18n::t("onboarding.tour.skip"))
            // gpui-component `Button` impls `InteractiveElement`, so `.a11y`
            // applies directly (no wrapping div): it emits a `Button` AccessKit
            // node AND chains `debug_selector("tour-skip")` so a label-located
            // click can resolve painted bounds via `debug_bounds` — no pixel.
            .a11y(
                "tour-skip",
                AccessRole::Button,
                dat0_i18n::t("onboarding.tour.skip"),
            )
            .on_click(move |_ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                mark_first_run_done();
                window.close_dialog(cx);
            });

        // Next / Get started: bottom-right. On the last panel it finishes
        // (marks done + dismisses); otherwise it advances.
        let next_label = if last {
            dat0_i18n::t("onboarding.tour.get_started")
        } else {
            dat0_i18n::t("onboarding.tour.next")
        };
        // The primary button's ROLE flips on the last panel (Next → Get started),
        // so its a11y id + debug_selector must match the label it currently shows
        // — a test locating "Get started" resolves `debug_bounds("tour-get-started")`.
        let next_id = if last {
            "tour-get-started"
        } else {
            "tour-next"
        };
        let next_btn = Button::new("tour-next")
            .label(next_label.clone())
            .a11y(next_id, AccessRole::Button, next_label)
            .on_click(move |_ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                window.close_dialog(cx); // pop current panel
                if last {
                    mark_first_run_done(); // finish
                } else {
                    present_panel(window, cx, next(index)); // push next
                }
            });

        // Back: appears from panel 2 on (index > 0).
        let back_btn = show_back.then(|| {
            Button::new("tour-back")
                .label(dat0_i18n::t("onboarding.tour.back"))
                .a11y(
                    "tour-back",
                    AccessRole::Button,
                    dat0_i18n::t("onboarding.tour.back"),
                )
                .on_click(move |_ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    present_panel(window, cx, back(index));
                })
        });

        // Dot pager: ● for the current panel, ○ for the rest.
        // A6g: the two dot colours come from the theme. Read from the dialog's
        // OWN context — this is a `move` closure, so touching the outer `cx`
        // would capture a `&mut App` and make it escape the function.
        // The ●/○ glyphs stay as text: they are their own elements, but no dot
        // icon was vendored in A5 and inventing one is out of this slice's scope.
        let d0 = dialog_cx.theme().d0();
        let (dot_on, dot_off) = (d0.pager_dot_active, d0.pager_dot_inactive);
        let mut pager = h_flex().gap_1();
        for i in 0..PANELS.len() {
            let glyph = if i == index { "●" } else { "○" };
            pager = pager.child(
                div()
                    .text_color(if i == index { dot_on } else { dot_off })
                    .child(glyph),
            );
        }

        // Bottom chrome row: [Skip] … pager … [‹ Back] [Next ›].
        let mut controls = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(skip);
        controls = controls.child(pager);
        let mut right = h_flex().gap_2();
        if let Some(b) = back_btn {
            right = right.child(b);
        }
        right = right.child(next_btn);
        controls = controls.child(right);

        dialog
            .title(dat0_i18n::t("onboarding.tour.title"))
            // Only the custom Back/Next/Skip controls exit the tour.
            .close_button(false)
            .overlay_closable(false)
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        // Illustration (placeholder solid PNG; real art lands T11).
                        img(ImageSource::Image(image.clone()))
                            .w(px(360.0))
                            .h(px(240.0)),
                    )
                    // Content-only locators (release no-ops): emit `Label`
                    // AccessKit nodes so the headless UAT can assert WHICH panel
                    // headline/body is rendered (the per-panel content the old
                    // `#[ignore]`d test called un-observable). Not clickable.
                    .child(
                        div()
                            .text_xl()
                            .a11y_label(AccessRole::Label, title.clone())
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .a11y_label(AccessRole::Label, body.clone())
                            .child(body.clone()),
                    )
                    .child(controls),
            )
    });
}
