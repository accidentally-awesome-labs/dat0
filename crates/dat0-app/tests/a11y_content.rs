//! UAT Gap 2 — Task 5: grid cell CONTENT assertions.
//!
//! Proves the headless harness can assert the *rendered values* of grid cells,
//! the thing gpui itself cannot extract. We build a real windowed
//! `#[gpui::test]`, import a tiny known table via the production `handle_drop`
//! flow (driven to completion with the Gap-3 async harness), mount it as the
//! active grid view, drive the PD-018 prefetch-on-bind so page 0 lands in the
//! grid's LRU (else every cell paints the `—` placeholder), then
//! `A11ySnapshot::capture` reads the AccessKit tree the grid's `render_td`
//! emitted and asserts the cell values are present as `Cell` nodes.
//!
//! ## Duplicate-tolerant queries (Task-2 review carry-forward)
//! Cell values repeat across a real grid (categories, booleans, small ints), so
//! the unique-match `has_label`/`query_by_role` (which panic on 2+ matches) are
//! unusable for cells. We assert via the duplicate-tolerant `has_label_any` /
//! `count_label` added to `support`.
//!
//! ## The em-dash gotcha (PD-018)
//! `render_td` paints a cell's real value only when the row's page is resident
//! in the `GridDataSource` LRU; otherwise it paints a `—` placeholder that is
//! deliberately NOT annotated. So we must settle the background page fetch
//! (`prefetch_visible_rows`, kicked on bind) BEFORE capturing — else the cells
//! render `—` and `has_label_any("1")` is (correctly) false.
//!
//! Hermeticity: `DAT0_CONFIG_DIR` points at a fresh temp dir; `#[serial]`
//! because `set_var` is process-global and `#[gpui::test]` is multithreaded.
//! The `a11y-capture` feature is auto-ON via the self-dev-dependency in
//! Cargo.toml, so `dat0_app::a11y::*` are the real capture symbols.

mod support;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::grid::GridDataSource;
use dat0_app::main_bridge::{MainLoop, MainThreadDispatcher};
use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;
use dat0_engine::QueryEngine as _;
use support::A11ySnapshot;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Point `config_dir()` at `dir` for the rest of this (serial) test.
fn set_config_dir(dir: &Path) {
    // SAFETY: tests are `#[serial]`, so no other thread races this process-global
    // write; each test sets it before doing anything that reads `config_dir()`.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

/// A tokio runtime kept alive for the whole test so the foreground-polled
/// `cx.spawn` futures — and the `tokio::spawn`ed grid prefetch — can call
/// `tokio::task::spawn_blocking` (the engine paths are built on it). Mirrors
/// production `run_app`, which holds `runtime.enter()` for all of
/// `Application::run`, and the Gap-3 harness in `onboarding_gpui.rs`.
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.rt.enter()
    }
    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }
}

/// Build the async harness and `allow_parking` so an engine-backed import can be
/// driven to completion via `block_test` on a captured `Task`. (See
/// `onboarding_gpui.rs::engine_backed_async_flow_completes_in_harness` for the
/// verified mechanism.)
fn enter_async_harness(cx: &mut TestAppContext) -> AsyncHarness {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    cx.executor().allow_parking();
    AsyncHarness { rt }
}

/// Construct an EMPTY session on the harness runtime so its engine shares the
/// runtime the test has entered.
fn build_empty_session_in(h: &AsyncHarness, state_root: &Path) -> Arc<Mutex<Session>> {
    let sess = h
        .block_on(Session::new(state_root, BUDGET))
        .expect("Session::new");
    Arc::new(Mutex::new(sess))
}

/// Open a real window whose root view is a `gpui_component::Root` wrapping a
/// fresh `WorkspaceShell` over `session` — exactly mirroring production
/// (`window.rs::open_window_view`). Returns the live shell entity plus the
/// windowed test context.
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

/// Initialise the gpui-component theme global (required before any view that
/// renders gpui-component widgets — the grid uses `Table`).
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
}

/// Process-global `MainLoop`; the dispatcher lives in `window_registry`'s
/// single-shot `OnceCell`. `prefetch_rows_for` posts its re-render `notify` onto
/// this dispatcher, so we install one (production parity) and drain it while
/// pumping. Its receiver must outlive the test, so it is stashed here.
static MAIN_LOOP: OnceLock<Mutex<Option<MainLoop>>> = OnceLock::new();

fn ensure_dispatcher() {
    let slot = MAIN_LOOP.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock();
    if guard.is_none() {
        let (dispatcher, main_loop) = MainThreadDispatcher::new();
        dat0_app::window_registry::install_dispatcher(dispatcher);
        *guard = Some(main_loop);
    }
}

fn drain_dispatcher(cx: &mut TestAppContext) {
    let Some(slot) = MAIN_LOOP.get() else {
        return;
    };
    let mut guard = slot.lock();
    if let Some(main_loop) = guard.as_mut() {
        cx.update(|app| main_loop.drain_for_test(app));
    }
}

// ----------------------------------------------------------------------------
// Grid cell values render as AccessKit `Cell` nodes (content assertion).
// ----------------------------------------------------------------------------

/// Import `a,b\n1,2\n3,4\n`, mount it as the active grid, settle the page-0
/// prefetch, and assert the four rendered cell values ("1","2","3","4") are
/// present as `Cell` nodes — the rendered-text extraction gpui cannot do.
///
/// Teeth: a value NOT in the fixture ("9999") is absent (`has_label_any` false,
/// `count_label` == 0). The positive checks assert EXISTENCE only — the
/// virtualized `Table` re-renders cells a non-uniform number of times per frame,
/// so a present value's exact `count_label` is a `Table` implementation detail
/// and is not asserted.
#[gpui::test]
#[serial]
fn grid_renders_cell_values_as_a11y_cells(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test
    init_components(cx);
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    let session = build_empty_session_in(&harness, state.path());

    // A tiny known table on disk. Fixed CSV data → deterministic cell values
    // (no timestamps/paths/random in the assertions). `cells.csv` → table
    // `cells` (register::derive_table_name: lowercase-alnum stem).
    let csv = state.path().join("cells.csv");
    std::fs::write(&csv, "a,b\n1,2\n3,4\n").unwrap();

    let (shell, cx) = open_shell_window(cx, Arc::clone(&session));
    cx.run_until_parked();

    // Import via the production `handle_drop` flow, driven to completion with
    // `block_test` (Gap-3 fallback 1) so the cross-thread `spawn_blocking` wake
    // is awaited and the table is actually registered.
    let sess = Arc::clone(&session);
    let csv2 = csv.clone();
    let task = cx.cx.spawn(async move |_app| {
        let _ = dat0_app::file_drop::handle_drop(vec![csv2], sess).await;
    });
    cx.executor().block_test(task);

    // Resolve the registered table + build a `GridDataSource` on the harness
    // runtime (its ctor runs the async schema/row-count probes).
    let engine = session.lock().engine.clone();
    let tables = harness
        .block_on(async { engine.get_tables().await })
        .expect("get_tables");
    let table_name = tables
        .iter()
        .map(|t| t.name.clone())
        .next()
        .expect("the CSV import must register exactly one table");
    assert_eq!(
        table_name, "cells",
        "table name derived from cells.csv stem"
    );
    let ds = harness
        .block_on(async { GridDataSource::new(Arc::clone(&engine), table_name).await })
        .expect("GridDataSource::new");
    let ds = Arc::new(ds);

    // Mount it as the active grid view (mirrors `route_drop_outcomes`'
    // `set_data_source` + `notify`). The NEXT render promotes the `Arc` into a
    // `TableState` and kicks the PD-018 page-0 prefetch (a `tokio::spawn`).
    let ds_for_mount = Arc::clone(&ds);
    shell.update(cx, |view, cx| {
        view.set_data_source(ds_for_mount);
        cx.notify();
    });

    // Pump the foreground queue + drain the dispatcher until page 0 is resident
    // in the grid LRU — `cell_render(0, 0)` returns `Some` once the background
    // `page_for` fetch commits. Until then every cell paints the unannotated
    // `—` placeholder (PD-018), so we MUST settle here before capturing.
    let mut page_ready = false;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        if ds.cell_render(0, 0).is_some() {
            page_ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        page_ready,
        "page 0 must load into the grid LRU so render_td paints real values \
         (else every cell is the em-dash placeholder)"
    );

    // Capture the AccessKit tree the grid render emitted and assert the rendered
    // cell values are present. Duplicate-tolerant queries are MANDATORY here:
    // unlike the hero's one-shot widgets, the virtualized `Table` re-runs
    // `render_td` a NON-uniform number of times per captured frame (a
    // column-width measure pass over some cells PLUS the paint pass), so a value
    // may appear more than once and different values may appear different numbers
    // of times (observed counts e.g. [4,1,2,1] for [1,2,3,4]). The unique-match
    // `has_label` would therefore panic; `has_label_any`/`count_label` (Task-2
    // review carry-forward) wrap kittest's non-panicking `query_all_by_label`.
    //
    // The load-bearing invariant is EXISTENCE: every visible cell paints at
    // least once per full frame (maximized test window, both rows visible), so
    // each real value has `count_label >= 1`. The exact multiplicity is a
    // gpui-component `Table` implementation detail and is deliberately NOT
    // asserted (that would be a fragile coupling).
    let snap = A11ySnapshot::capture(cx);
    for v in ["1", "2", "3", "4"] {
        assert!(
            snap.has_label_any(v),
            "cell value {v:?} must render as an AccessKit Cell node (count {})",
            snap.count_label(v)
        );
    }

    // Teeth: a value NOT in the table must be absent (proves the positive
    // assertions are bound to real rendered content, not always-true).
    assert!(
        !snap.has_label_any("9999"),
        "a value not in the table must not render as a Cell node"
    );
    assert_eq!(snap.count_label("9999"), 0);

    // Settle any remaining detached-task work while `state` is still alive
    // (persist / GridDataSource background paths), mirroring the hero test.
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(50));
        cx.run_until_parked();
    }

    drop(state);
}
