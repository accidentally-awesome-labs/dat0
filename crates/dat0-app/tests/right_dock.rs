//! B6: the right dock — Inspector + Charts as real `DockArea` panels.
//!
//! These assert through the a11y capture rather than through private shell
//! state, so they exercise the whole path the user sees: bool → `Panel::visible`
//! → `TabPanel` chrome → rendered body. The one exception is
//! `right_dock_open_for_test`, which reads the DOCK's own open flag — deliberate,
//! because a test that re-read the bool it just wrote would prove only that
//! assignment works, not that `sync_right_dock` ran.
//!
//! Hermeticity: `DAT0_CONFIG_DIR` points at a fresh temp dir; `#[serial]`
//! because both `set_var` and the capture collector are process-global.

mod support;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;
use support::A11ySnapshot;

const BUDGET: u64 = 128 * 1024 * 1024;

/// The i18n strings the two title bars render. Read through `dat0_i18n` rather
/// than hardcoded so a translation change moves the test with the UI.
fn inspector_title() -> String {
    dat0_app::dat0_i18n::t("inspector.title")
}
fn charts_title() -> String {
    dat0_app::dat0_i18n::t("charts.title")
}

fn set_config_dir(dir: &Path) {
    // SAFETY: `#[serial]` — no other thread races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    // The panel registry is prod-only otherwise, and a registration absent
    // under test is silently absent (the `register_modal_keys` lesson).
    cx.update(dat0_app::panels::register_panels);
}

fn build_empty_session(state_root: &Path) -> Arc<Mutex<Session>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let sess = rt
        .block_on(Session::new(state_root, BUDGET))
        .expect("Session::new");
    Arc::new(Mutex::new(sess))
}

/// Open a real window whose root is a `Root` wrapping a live `WorkspaceShell`,
/// mirroring production (`window.rs::open_window_view`).
fn open_shell_window(
    cx: &mut TestAppContext,
    session: Arc<Mutex<Session>>,
) -> (Entity<WorkspaceShell>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<WorkspaceShell>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let shell = cx.new(|c| WorkspaceShell::new(session, c));
        *slot2.borrow_mut() = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = slot.borrow().clone().expect("shell captured");
    (shell, vcx)
}

/// Boot a shell over an empty session with a fresh config dir.
fn boot(cx: &mut TestAppContext) -> (Entity<WorkspaceShell>, &mut VisualTestContext) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // The tempdir must outlive the window; leak it rather than let the session
    // read a deleted path mid-test.
    std::mem::forget(tmp);
    (shell, vcx)
}

fn right_dock_open(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> bool {
    vcx.cx
        .update(|app| shell.read(app).right_dock_open_for_test(app))
}

/// Force one frame so `sync_right_dock` — which runs inside `render` — gets a
/// chance to act on whatever bools the test just wrote.
///
/// Production does not need this: both toggles (`toggle_chart_panel` and the
/// View-menu Inspector listener) end in `cx.notify()`. But the older
/// `chart_bind_for_test` shim predates B6 and takes no `cx`, so it cannot
/// notify. Rather than change that shim's signature and churn its existing
/// callers in `chart_uat_window.rs`, the tests here pump explicitly.
fn settle(vcx: &mut VisualTestContext) {
    vcx.update(|window, _app| window.refresh());
    vcx.run_until_parked();
}

#[gpui::test]
#[serial]
fn right_dock_is_closed_when_both_panels_are_hidden(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    assert!(
        vcx.cx.update(|app| shell.read(app).dock_mounted_for_test()),
        "the DockArea should be built on the first render"
    );
    assert!(
        !right_dock_open(&shell, vcx),
        "a fresh workspace shows neither panel, so the right dock must be closed"
    );

    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.count_label(&inspector_title()),
        0,
        "no Inspector title bar while the panel is hidden"
    );
    assert_eq!(
        snap.count_label(&charts_title()),
        0,
        "no Charts title bar while the panel is hidden"
    );
}

#[gpui::test]
#[serial]
fn showing_the_inspector_opens_the_dock_and_titles_it(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.seed_lineage_target_for_test("orders".into(), cx);
        });
    });
    vcx.run_until_parked();

    assert!(
        right_dock_open(&shell, vcx),
        "showing the Inspector must open the right dock — this is the assertion \
         that proves sync_right_dock ran, not just that the bool was written"
    );

    let snap = A11ySnapshot::capture(vcx);
    // EXACTLY one. Two would mean the inspector's own body title row survived
    // its move into `InspectorPanel::title` (B6 T1) and is now rendered
    // alongside the dock's title bar.
    assert_eq!(
        snap.count_label(&inspector_title()),
        1,
        "the Inspector title must appear exactly once — in the dock title bar"
    );
    assert_eq!(
        snap.count_label(&charts_title()),
        0,
        "Charts stays hidden when only the Inspector was shown"
    );
}

#[gpui::test]
#[serial]
fn showing_charts_renders_its_title_and_export_buttons(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.chart_bind_for_test(
                "\"sales\"".into(),
                vec![
                    ("region".into(), "VARCHAR".into()),
                    ("amt".into(), "DOUBLE".into()),
                ],
            );
        });
    });
    settle(vcx);

    assert!(
        right_dock_open(&shell, vcx),
        "showing Charts must open the right dock"
    );

    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.count_label(&charts_title()),
        1,
        "the Charts title must appear exactly once — in the dock title bar"
    );
    // The export buttons now live in the title bar via
    // `ChartsPanel::toolbar_buttons`. Upstream forces `tab_stop(false)` on them,
    // which is why B6 T2 registered the palette descriptors; this asserts the
    // MOUSE affordance still exists after the move.
    for key in ["chart.export.png", "chart.export.svg"] {
        let label = dat0_app::dat0_i18n::t(key);
        assert!(
            snap.has_label_any(&label),
            "{key} = {label:?} should be rendered in the Charts title bar"
        );
    }
}

#[gpui::test]
#[serial]
fn hiding_both_panels_closes_the_dock_again(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.seed_lineage_target_for_test("orders".into(), cx);
            ws.chart_bind_for_test("\"sales\"".into(), vec![("amt".into(), "DOUBLE".into())]);
        });
    });
    settle(vcx);
    assert!(
        right_dock_open(&shell, vcx),
        "both panels visible => dock open"
    );

    // Now hide both. This is the only bidirectional proof of the reconcile
    // loop: a sync that only ever opened would pass every test above.
    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.hide_right_dock_panels_for_test(cx);
        });
    });
    vcx.run_until_parked();

    assert!(
        !right_dock_open(&shell, vcx),
        "hiding both panels must close the right dock again"
    );
    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(snap.count_label(&inspector_title()), 0);
    assert_eq!(snap.count_label(&charts_title()), 0);
}

#[gpui::test]
#[serial]
fn inspector_body_content_reaches_the_capture_through_the_dock(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.seed_lineage_target_for_test("orders".into(), cx);
        });
    });
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    // The title bar is rendered OUTSIDE the `.cached(..)` wrapper, so asserting
    // on it alone would keep passing even if the cache swallowed every body
    // node. This asserts on real BODY content — the overview line the inspector
    // renders for its target — which is the only thing that proves the panel
    // body survives `tab_panel.rs`'s cached child.
    assert!(
        snap.has_label_contains("orders"),
        "the Inspector's own body content must reach the capture through the \
         dock's cached wrapper, not just its title bar"
    );
}
