//! UAT "Update + About dialogs" slice (P10a / P10a-2 UI).
//!
//! Tests the About box and in-app updater DIALOGS: real rendered content,
//! the `is_manual` silent-background gating, and safe dismissal. Calls the
//! main-thread render helpers (`about::present`, `update::ui::show_*`) DIRECTLY
//! from a plain `&mut App` over a minimal `gpui_component::Root` host window —
//! no network, no `std::thread::spawn`, no dispatcher (unlike `about::open` /
//! `run_update_flow`, which do all three). Mirrors `tests/onboarding_gpui.rs`,
//! which proves the same `cx.active_window()` + `window.open_dialog` path and
//! that `.a11y_label`-annotated dialog bodies are read by `A11ySnapshot::capture`
//! (the dialog builder re-runs each frame, so the construction-time push()
//! re-fires under `capture`'s forced refresh).
//!
//! SAFETY: never dismiss a confirm-variant with `enter` — its OK button is
//! Download (About, newer) or Install & Restart (update prompt), whose `on_ok`
//! reaches `platform::open_url` (real browser) or the installer. Alerts →
//! `enter` (harmless `on_ok`); confirm-variants → `escape` (harmless `on_cancel`
//! = "Later"/Cancel).

mod support;

use std::time::Duration;

use gpui::{AppContext as _, ParentElement as _, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};

use dat0_app::about::build_info::BuildInfo;
use support::A11ySnapshot;

/// A minimal host view that mounts gpui-component's DIALOG overlay layer (via
/// `Root::render_dialog_layer`) but nothing of its own. LOAD-BEARING: `Root::render`
/// paints ONLY `self.view`, so a host that does not itself paint the dialog layer
/// leaves `open_dialog` setting `active_*` state while painting NOTHING — the
/// dialog subtree (and its `.a11y_label` content push) never renders, so
/// `A11ySnapshot::capture` sees zero nodes even though `has_active_dialog` is true.
/// Production mirrors this (window.rs:6573, settings_ui/panel.rs:540). We mount
/// only the dialog layer (not sheets), so the captured frame is the dialog's own
/// content.
struct DialogHost;
impl gpui::Render for DialogHost {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div().children(Root::render_dialog_layer(window, cx))
    }
}

/// Open a real, ACTIVATED window whose root is a `gpui_component::Root` wrapping
/// a `DialogHost` — mirrors `onboarding_gpui::open_shell_window`. Activation
/// makes `cx.active_window()` (which `present`/`show_*` rely on) resolve to it.
fn open_dialog_host(cx: &mut TestAppContext) -> &mut VisualTestContext {
    // Required before any gpui-component widget (Dialog) is built.
    cx.update(gpui_component::init);
    let (_root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let host = cx.new(|_| DialogHost);
        Root::new(host, window, cx)
    });
    vcx
}

/// True iff a dialog is currently on the window's `Root` stack.
fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, app| window.has_active_dialog(app))
}

// ----------------------------------------------------------------------------
// Task 0 — SPIKE HARD-GATE.
// ----------------------------------------------------------------------------

/// Proves, against the REAL `about::present`, that (a) the host window mounts
/// and activates so `active_window()` resolves; (b) `present_for_test(cx, None)`
/// opens a dialog (`has_active_dialog`); (c) the `.a11y_label`-annotated body is
/// read by the standard `A11ySnapshot::capture`; (d) `enter` dismisses the
/// alert. If (c) fails, STOP-and-report (design §7).
#[gpui::test]
fn spike_about_dialog_opens_captures_content_and_dismisses(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "(a) clean baseline: no dialog before open");

    // (b) Open the About box (up-to-date variant) from a plain App context —
    // `present` re-enters the active window itself, so it must NOT be nested in
    // a `VisualTestContext::update` window closure.
    vcx.cx
        .update(|app| dat0_app::about::present_for_test(app, None));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "(b) present must open the About dialog");

    // (c) Settle the open animation, then read the emitted tree. `has_label_contains`
    // finds the version substring inside the multi-line body node.
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(BuildInfo::current().version),
        "(c) GATE: dialog body content must be captured by A11ySnapshot \
         (version substring {:?} missing)",
        BuildInfo::current().version
    );
    // Teeth: a fabricated string must be absent — proves (c) reads real content.
    assert!(
        !snap.has_label_contains("NOTAREALVERSIONZZZ"),
        "a string the dialog never rendered must not be found"
    );

    // (d) `enter` fires the alert's harmless `on_ok` (|_,_,_| true) and closes it.
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "(d) enter must dismiss the About alert");
}

// ----------------------------------------------------------------------------
// Task 1 — About box content (up-to-date + newer-release variants).
// ----------------------------------------------------------------------------

/// The up-to-date About box shows version + Apache-2.0 + the NOTICE line + the
/// "latest version" line, and NOT the "update available" line. Dismiss via
/// `enter` (alert OK is harmless `|_,_,_| true`).
#[gpui::test]
fn about_up_to_date_content(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx
        .update(|app| dat0_app::about::present_for_test(app, None));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "About dialog must be open");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(BuildInfo::current().version),
        "About must show the crate version"
    );
    assert!(
        snap.has_label_contains("Apache-2.0"),
        "About must show the Apache-2.0 license id"
    );
    assert!(
        snap.has_label_contains(&dat0_i18n::t("about.acknowledgements")),
        "About must show the NOTICE acknowledgements line"
    );
    assert!(
        snap.has_label_contains(&dat0_i18n::t("about.update.current")),
        "up-to-date About must show the 'latest version' line"
    );
    // Teeth: the newer-release nudge line must be ABSENT in the up-to-date box.
    assert!(
        !snap.has_label_contains(&dat0_i18n::t("about.update.available")),
        "up-to-date About must NOT show the 'update available' nudge"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the About alert");
}

/// The newer-release About box shows the "update available" line + the tag, and
/// NOT the "latest version" line. Dismiss via `escape` (Cancel) — NEVER `enter`,
/// whose OK is Download (opens the browser via `platform::open_url`).
#[gpui::test]
fn about_newer_release_content(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx
        .update(|app| dat0_app::about::present_for_test(app, Some("0.2.0".to_string())));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "About dialog must be open");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("about.update.available")),
        "newer-release About must show the 'update available' line"
    );
    assert!(
        snap.has_label_contains("0.2.0"),
        "newer-release About must show the newer tag"
    );
    // Teeth: the up-to-date line must be ABSENT in the newer-release box.
    assert!(
        !snap.has_label_contains(&dat0_i18n::t("about.update.current")),
        "newer-release About must NOT show the 'latest version' line"
    );

    // Dismiss via Cancel (escape) — must NOT fire the Download on_ok.
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "escape must dismiss the newer-release About");
}

// ----------------------------------------------------------------------------
// Task 2 — update "checking…" + "up to date" (manual shows / background silent).
// ----------------------------------------------------------------------------

/// The manual-path "checking…" alert opens with its text and dismisses on enter.
#[gpui::test]
fn update_checking_alert_content(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx.update(|app| {
        dat0_app::update::ui::show_alert_dialog_for_test(app, dat0_i18n::t("update.checking"))
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "checking alert must be open");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("update.checking")),
        "checking alert must show its text"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the checking alert");
}

/// Manual path (`is_manual=true`): "up to date" alert is SHOWN with its text.
#[gpui::test]
fn update_up_to_date_manual_shows(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx
        .update(|app| dat0_app::update::ui::show_up_to_date_for_test(app, true));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "manual up-to-date must open a dialog");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("update.up_to_date")),
        "manual up-to-date must show its text"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the up-to-date alert");
}

/// Background path (`is_manual=false`): "up to date" is SILENT — no dialog.
/// (Teeth: `update_up_to_date_manual_shows` proves the same helper DOES open a
/// dialog when `is_manual=true`, so this negative is meaningful, not vacuous.)
#[gpui::test]
fn update_up_to_date_background_silent(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "clean baseline");

    vcx.cx
        .update(|app| dat0_app::update::ui::show_up_to_date_for_test(app, false));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    assert!(
        !dialog_open(vcx),
        "background up-to-date must stay silent (no dialog)"
    );
}

// ----------------------------------------------------------------------------
// Task 3 — update error dialog (manual shows / background silent).
// ----------------------------------------------------------------------------

/// Manual path: a failed check shows the "Update failed: {msg}" alert with both
/// the failure label and the underlying message.
#[gpui::test]
fn update_error_manual_shows(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx.update(|app| {
        dat0_app::update::ui::show_error_banner_for_test(app, true, "network down")
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "manual error must open a dialog");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("update.failed")),
        "manual error must show the 'Update failed' label"
    );
    assert!(
        snap.has_label_contains("network down"),
        "manual error must show the underlying message"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the error alert");
}

/// Background path: a failed launch-check stays SILENT — no dialog.
/// (Teeth: `update_error_manual_shows` proves the same helper DOES open when
/// `is_manual=true`.)
#[gpui::test]
fn update_error_background_silent(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "clean baseline");

    vcx.cx.update(|app| {
        dat0_app::update::ui::show_error_banner_for_test(app, false, "network down")
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    assert!(
        !dialog_open(vcx),
        "background error must stay silent (no dialog)"
    );
}
