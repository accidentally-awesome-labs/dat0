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

/// A tokio runtime kept alive for the whole test so the foreground-polled
/// `cx.spawn` futures can call `tokio::task::spawn_blocking` (the engine and
/// file-import paths are built on it). Mirrors production `run_app`
/// (window.rs:1308/1377), which holds `runtime.enter()` for all of
/// `Application::run`. The caller MUST bind `enter()`'s guard to a `_guard`
/// held to end-of-test, and the harness MUST outlive every spawned task.
/// Copied verbatim from `tests/onboarding_gpui.rs:94-124` — T2 needs it because
/// `save_named_chart` ends by calling `refresh_catalog`, which `tokio::spawn`s.
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.rt.enter()
    }
    #[allow(dead_code)] // parity with the copied onboarding_gpui.rs harness; unused by T2's tests
    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }
}

/// Build the async harness and call `cx.executor().allow_parking()` so an
/// engine-backed import can be driven to completion via `block_test` on a
/// captured `Task`.
///
/// NOTE: `allow_parking` does NOT make `run_until_parked` wait for the
/// cross-thread `spawn_blocking` to re-enqueue — `run_until_parked` ticks the
/// foreground queue and returns the instant the task parks on the JoinHandle,
/// regardless of parking mode. See `engine_backed_async_flow_completes_in_harness`
/// for the verified mechanism, and drive engine flows with
/// `cx.executor().block_test(task)`, not `run_until_parked`.
fn enter_async_harness(cx: &mut TestAppContext) -> AsyncHarness {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    cx.executor().allow_parking();
    AsyncHarness { rt }
}

// UAT (Charts save/persist/lineage slice) T3: lineage-node render + click-reopen.

/// Build a deterministic `SavedChart` — a **fixed** id/`saved_at` (never
/// `Uuid::now_v7`/`now_unix_millis`) so the lineage-chart tests are
/// reproducible. `source` is the already-quoted engine source (e.g.
/// `"\"sales\""`), matching how `chart_panel.spec.source`/`SavedChart::spec`
/// stores it elsewhere in this file.
fn seeded_chart(
    name: &str,
    source: &str,
    chart_type: ChartType,
    x: &str,
    y: &str,
) -> dat0_app::session::charts::SavedChart {
    dat0_app::session::charts::SavedChart {
        id: uuid::Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001),
        name: name.into(),
        spec: dat0_engine::chart_spec::ChartSpec {
            chart_type,
            source: source.into(),
            x: Some(x.into()),
            y: Some(y.into()),
            group: None,
            color: None,
            title: String::new(),
        },
        saved_at: 1_700_000_000_000,
    }
}

/// A minimal injected-catalog `TableInfo` (fields per `dat0-engine/src/types.rs:117`).
/// The lineage closure matches on the bare `name` only, so `origin`/`columns`
/// are irrelevant to chart-descendant attachment.
fn tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::File(std::path::PathBuf::from("/data/sales.csv")),
    }
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

#[gpui::test]
#[serial]
fn chart_panel_empty_state_renders_hint(cx: &mut TestAppContext) {
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    // Visible panel, but no columns/axes bound → empty hint renders.
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.chart_bind_for_test("\"t\"".into(), vec![]);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains("Select columns"),
        "empty-state hint rendered (chart.panel.empty)"
    );
}

#[gpui::test]
#[serial]
fn chart_panel_renders_scatter_axes(cx: &mut TestAppContext) {
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.chart_bind_for_test(
                "\"m\"".into(),
                vec![("x".into(), "DOUBLE".into()), ("y".into(), "DOUBLE".into())],
            );
            ws.chart_set_axes_for_test(
                ChartType::Scatter,
                Some("x".into()),
                Some("y".into()),
                String::new(),
            );
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains("Scatter"),
        "scatter type seam rendered"
    );
    assert!(
        snap.has_label("x") && snap.has_label("y"),
        "axis seams rendered"
    );
}

// UAT (Charts save/persist/lineage slice) T2: save → toast + persist + guards.
//
// `save_named_chart` (window.rs:4277) pushes an info banner onto the
// process-global `error_ux::banner::PENDING` queue, upserts the chart into the
// session's persisted list, then calls `refresh_catalog` (`tokio::spawn`) and
// `maybe_prompt_save_workspace` (in-memory guard, no I/O, no dialog — see
// window.rs:2338). Because `PENDING` is a process-global static shared by every
// serial test in this binary (and by `tests/onboarding_gpui.rs`'s own banner
// pushes, if run in the same process), each save test drains it at entry.

#[gpui::test]
#[serial]
fn save_chart_shows_toast_and_persists(cx: &mut TestAppContext) {
    let _ = dat0_app::error_ux::banner::drain_pending(); // clear cross-test PENDING
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx); // save_named_chart → refresh_catalog spawns
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
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
                String::new(),
            );
            ws.save_named_chart_for_test("Q1 sales".into(), cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    // Persisted into the session.
    {
        let s = session.lock();
        assert_eq!(s.charts().len(), 1, "one saved chart");
        assert_eq!(s.charts()[0].name, "Q1 sales");
        assert_eq!(s.charts()[0].spec.chart_type, ChartType::Bar);
        assert_eq!(s.charts()[0].spec.y.as_deref(), Some("amt"));
    }
    // Toast rendered.
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.query_by_role(dat0_app::a11y::AccessRole::Alert, "Chart saved"),
        "Chart saved alert rendered"
    );
}

#[gpui::test]
#[serial]
fn save_chart_empty_name_is_noop(cx: &mut TestAppContext) {
    let _ = dat0_app::error_ux::banner::drain_pending();
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.chart_bind_for_test("\"sales\"".into(), vec![("amt".into(), "DOUBLE".into())]);
            ws.save_named_chart_for_test("   ".into(), cx); // whitespace → guard at window.rs:4278
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    assert_eq!(
        session.lock().charts().len(),
        0,
        "whitespace name saves nothing"
    );
}

#[gpui::test]
#[serial]
fn save_chart_without_source_is_noop(cx: &mut TestAppContext) {
    let _ = dat0_app::error_ux::banner::drain_pending();
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            // No bind → chart_panel.source is None → guard at window.rs:4284.
            ws.save_named_chart_for_test("orphan".into(), cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    assert_eq!(session.lock().charts().len(), 0, "no source saves nothing");
}

#[gpui::test]
#[serial]
fn saved_chart_appears_as_lineage_node(cx: &mut TestAppContext) {
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    // Seed the session with a chart rooted on table "sales".
    session
        .lock()
        .set_charts(vec![seeded_chart(
            "Region totals",
            "\"sales\"",
            ChartType::Bar,
            "region",
            "amt",
        )])
        .unwrap();
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            // Inject a catalog containing "sales" so closure("sales") exists, then
            // target it → recompute_lineage attaches the chart as a descendant.
            ws.seed_catalog_for_test(vec![tbl("sales")]);
            ws.seed_lineage_target_for_test("sales".into(), cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains("Region totals"),
        "chart node rendered in lineage"
    );
}

#[gpui::test]
#[serial]
fn click_lineage_chart_reopens_panel_with_restored_spec(cx: &mut TestAppContext) {
    init_components(cx);

    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx); // open_saved_chart → show_chart_with_spec tokio::spawn
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    session
        .lock()
        .set_charts(vec![seeded_chart(
            "Region totals",
            "\"sales\"",
            ChartType::Scatter,
            "region",
            "amt",
        )])
        .unwrap();
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.seed_catalog_for_test(vec![tbl("sales")]);
            ws.seed_lineage_target_for_test("sales".into(), cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    // Real click on the rendered chart node → routes to open_saved_chart.
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, "Region totals");
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    // Panel reopened with the persisted spec (verbatim, not blanked).
    let (visible, spec) = vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            (ws.chart_visible_for_test(), ws.chart_spec_for_test())
        })
    });
    assert!(visible, "chart panel reopened");
    assert_eq!(spec.chart_type, ChartType::Scatter, "restored type");
    assert_eq!(spec.x.as_deref(), Some("region"));
    assert_eq!(spec.y.as_deref(), Some("amt"));

    // And the restored spec is visible in the rendered panel via the seams.
    let snap2 = A11ySnapshot::capture(vcx);
    assert!(
        snap2.has_label_contains("Scatter"),
        "restored type rendered"
    );
}
