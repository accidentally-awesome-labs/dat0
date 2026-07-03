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
