//! UAT "Charts save/persist/lineage" slice — T0 SPIKE (HARD GATE).
//!
//! Proves the whole mechanism end-to-end before any real chart tests are
//! written: mounts the full production `WorkspaceShell` (not a minimal host,
//! unlike the dialog-layer spikes — the chart dock is a real child of the
//! shell's own `render`, gated on `chart_panel_visible`, window.rs:6544), binds
//! and sets a spec via the `*_for_test` shims, forces a settle and capture, and
//! asserts the `.a11y_label` content seams added to
//! `charts::panel::render_chart_body` are visible in the captured a11y tree.
//! If this captures 0 nodes, the paint path is wrong (chart dock not mounted,
//! or the seam wrapper never emits) — that must be caught HERE, not in a later
//! task's assertions (mirrors Slice-1/2 T0, which caught the dialog-layer gap).
//!
//! Harness helpers below are COPIED verbatim from `tests/onboarding_gpui.rs`
//! (decided: duplicate per test binary rather than extract to `tests/support/`,
//! matching that file's own precedent and leaving passing tests untouched).

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;
use std::cell::RefCell;
use std::rc::Rc;

use support::A11ySnapshot;

use dat0_app::charts::spec::ChartType;
use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Point `config_dir()` at `dir` for the rest of this (serial) test.
/// Copied verbatim from `tests/onboarding_gpui.rs:60-64`.
fn set_config_dir(dir: &Path) {
    // SAFETY: tests are `#[serial]`, so no other thread races this process-global
    // write; each test sets it before doing anything that reads `config_dir()`.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

/// Build a real, EMPTY in-memory session (no tabs/recents → the shell renders
/// its empty-state hero). Copied verbatim from `tests/onboarding_gpui.rs:77-86`.
///
/// `Session::new` is async and the engine uses `tokio::task::spawn_blocking`
/// internally, so it must run inside a tokio runtime — which the gpui test
/// executor is not. We block on it with a dedicated multi-thread runtime BEFORE
/// the window is opened. `Session::new` awaits all of its own spawn_blocking work
/// (migrations etc.) before returning, and the engine holds only a synchronous
/// `duckdb::Connection` (no Drop needs a runtime), so the runtime can be dropped
/// once construction completes.
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

/// Open a real window whose root view is a `gpui_component::Root` wrapping a
/// fresh `WorkspaceShell` over `session` — exactly mirroring production
/// (`window.rs::open_window_view`). The window is ACTIVATED inside the build
/// closure so `cx.active_window()` resolves to it before the first render
/// frame runs. Returns the live shell entity plus the windowed test context.
/// Copied verbatim from `tests/onboarding_gpui.rs:141-157`.
fn open_shell_window(
    cx: &mut TestAppContext,
    session: Arc<Mutex<Session>>,
) -> (Entity<WorkspaceShell>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<WorkspaceShell>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        // Activate now so the auto-show dispatcher hop (and any later
        // `onboarding::open`) sees this as the active window.
        window.activate_window();
        let shell = cx.new(|c| WorkspaceShell::new(session, c));
        *slot2.borrow_mut() = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = slot.borrow().clone().expect("shell captured");
    (shell, vcx)
}

/// Initialise the gpui-component theme global — required before any view that
/// renders gpui-component widgets (the chart toolbar uses `Button`; every
/// existing shell-mounting test in `onboarding_gpui.rs`/`settings_window.rs`/
/// `update_about_window.rs` calls this before opening the window).
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
}

#[gpui::test]
#[serial]
fn spike_bound_chart_renders_spec_content(cx: &mut TestAppContext) {
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.chart_bind_for_test(
                "\"sales\"".into(),
                vec![
                    ("region".into(), "VARCHAR".into()),
                    ("amt".into(), "DOUBLE".into()),
                ],
            );
            ws.chart_set_axes_for_test(
                ChartType::Bar,
                Some("region".into()),
                Some("amt".into()),
                "Sales by region".into(),
            );
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label_contains("Bar"), "chart type seam rendered");
    assert!(snap.has_label("region"), "x-axis seam rendered");
    assert!(snap.has_label("amt"), "y-axis seam rendered");
    assert!(snap.has_label("Sales by region"), "title seam rendered");
}
