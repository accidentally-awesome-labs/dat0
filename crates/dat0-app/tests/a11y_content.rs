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
//!
//! ## Task 6: Inspector field content
//! `inspector_renders_field_content` (below) extends this file with the
//! Inspector dock's profiling-content assertions. See that test's doc comment
//! for why it calls `inspector::panel::render_inspector` directly instead of
//! going through a mounted window (`InspectorModel`/`inspector_panel_visible`
//! are `pub(crate)`, so the ONLY production entry point that populates them,
//! `WorkspaceShell::open_table_tab`/`set_inspector_target`, is unreachable
//! from an integration test — `render_inspector` is a pure fn of `model`
//! precisely so a test can drive it directly instead).

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
use dat0_app::view::sql_console::{ResultRegion, SqlConsole};
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

/// Open a real window whose root is a `gpui_component::Root` wrapping a fresh
/// standalone [`SqlConsole`] — the console's production `render` then runs on
/// every frame. `WorkspaceShell::toggle_sql_console` (the production entry that
/// builds + mounts the console) is `pub(crate)`, so — like the whole
/// `spawn_sql_run`/`finish_sql_run` run orchestration — it is unreachable from
/// this integration crate (a different crate boundary); mounting the console
/// directly is the analogue of the inspector test's direct `render_inspector`
/// call. `SqlConsole::new` is `pub` and takes the same `&mut Window` the shell
/// hands it, so the console is built exactly as production builds it.
fn open_sql_console_window(
    cx: &mut TestAppContext,
) -> (Entity<SqlConsole>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<SqlConsole>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let console = cx.new(|c| {
            SqlConsole::new(
                &[],
                None,
                dat0_app::query::completion::new_shared_snapshot(),
                window,
                c,
            )
        });
        *slot2.borrow_mut() = Some(console.clone());
        Root::new(console, window, cx)
    });
    let console = slot.borrow().clone().expect("console captured");
    (console, vcx)
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

// ----------------------------------------------------------------------------
// Inspector renders profiled field text as AccessKit Label nodes (Task 6).
// ----------------------------------------------------------------------------

/// Build an `InspectorModel` for a real, type-pinned fixture table via the
/// engine's genuine `SUMMARIZE` profiling (`profile_table` — the exact call
/// `WorkspaceShell::load_inspector_profile` makes in production), then invoke
/// the production `render_inspector` pure fn directly against a real
/// `WorkspaceShell` entity/`Context` and assert the rendered content — the
/// thing gpui itself cannot extract (UAT Gap 2).
///
/// ## Why call `render_inspector` directly (not through a mounted window)
/// `WorkspaceShell::render()` only calls `render_inspector` when
/// `inspector_panel_visible` is true, and only ever paints `self.inspector` —
/// both fields are `pub(crate)`, and the only production entry point that
/// populates them (`open_table_tab` → `set_inspector_target` →
/// `load_inspector_profile`) is `pub(crate)` too, so none of it is reachable
/// from an integration test (a different crate): there is no test-reachable
/// setter for `InspectorModel` or the panel's visibility. This test therefore
/// calls the production `render_inspector` pub fn directly — an implementer
/// judgment call, not something sanctioned by the task brief, the plan, or
/// the design doc. `render_inspector` / `column_card` are documented as
/// *pure functions of `model`*, which is what makes the direct call sound:
/// it still fires the `.a11y_label` pushes at element-build time exactly as
/// a mounted render would.
/// We still need a REAL `Entity<WorkspaceShell>` + `Context<WorkspaceShell>`
/// because the fn signature is hard-typed to it (the mode-toggle button
/// builds a `cx.listener` closure) — but no window / paint cycle is required:
/// `.a11y_label` pushes into the thread-local FRAME collector synchronously,
/// at element-BUILD time, not at paint time (see `a11y/mod.rs`), so one
/// direct call already yields exactly the nodes this render produces. That
/// also means the usual `window.refresh()` frame-reset bracket
/// (`A11ySnapshot::capture`) is unnecessary here — that bracket exists to
/// force exactly one clean re-render of a window-mounted view; with no
/// window in the loop there is exactly one render by construction. Since
/// every `A11ySnapshot` field is `pub`, we build the snapshot straight from
/// the raw `take_tree_update()` capture instead.
///
/// ## Determinism
/// The fixture casts both columns (`CAST(... AS BIGINT)` / `CAST(... AS
/// VARCHAR)`) so DuckDB's `SUMMARIZE` `column_type` is pinned rather than
/// left to literal-inference — the column card headers ("id · BIGINT",
/// "val · VARCHAR") are then stable, known strings. No timestamps/paths/
/// random values are asserted.
#[gpui::test]
#[serial]
fn inspector_renders_field_content(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test (profile_table needs spawn_blocking)

    let session = build_empty_session_in(&harness, state.path());
    let engine = session.lock().engine.clone();

    // A tiny, TYPE-PINNED table — see "Determinism" above.
    harness
        .block_on(async {
            engine
                .create_table(
                    "probe",
                    "SELECT CAST(id AS BIGINT) AS id, CAST(val AS VARCHAR) AS val \
                     FROM (VALUES (1, 'a'), (2, 'b'), (3, 'c')) AS t(id, val)",
                    dat0_engine::DerivedOrigin::Sql("probe fixture".into()),
                )
                .await
        })
        .expect("create_table probe");

    // The exact call `WorkspaceShell::load_inspector_profile` makes in
    // production for `ProfileTargetMode::WholeTable`.
    let profile = harness
        .block_on(async { engine.profile_table("probe", None).await })
        .expect("profile_table probe");
    assert_eq!(profile.rows, 3, "fixture has 3 rows");
    // 3, not 2: `create_table` eagerly injects the `__dat0_rowid` surrogate
    // (design §5, "at create time"), which SUMMARIZE profiles too — the
    // overview line counts the raw profile column list (below), while
    // `project_cards` (projection=None) filters the surrogate back out for
    // the per-column CARDS, so exactly 2 cards render (id, val).
    assert_eq!(
        profile.columns.len(),
        3,
        "fixture has 3 profiled columns (id, val, __dat0_rowid surrogate)"
    );

    let mut model = dat0_app::inspector::InspectorModel::new();
    model.set_target("probe".to_string());
    model.put(profile);

    // A real `WorkspaceShell` entity — no window needed (see doc comment).
    let shell = cx.new(|cx| WorkspaceShell::new(Arc::clone(&session), cx));

    dat0_app::a11y::reset();
    shell.update(cx, |_ws, cx| {
        let _ = dat0_app::inspector::panel::render_inspector(&model, None, cx);
    });
    let cap = dat0_app::a11y::take_tree_update();
    let snap = A11ySnapshot {
        state: kittest::State::new(cap.update),
        click_ids: cap.click_ids,
    };

    // Overview line: brittle to reconstruct exactly in general (row/col
    // counts are computed live), so assert the stable substring per the task
    // brief rather than the whole formatted string.
    assert!(
        snap.has_label_contains("probe — 3 rows · 3 cols"),
        "overview line must render the table's real profiled row/col counts \
         (raw profile column count, INCLUDING the surrogate — see the comment \
         at the `profile.columns.len()` assertion above)"
    );

    // Column card headers: exact-match is safe here — both are unique in
    // this 2-column fixture (would panic on 2+ matches, not expected).
    assert!(
        snap.has_label("id · BIGINT"),
        "the id column's card header must render its real profiled type"
    );
    assert!(
        snap.has_label("val · VARCHAR"),
        "the val column's card header must render its real profiled type"
    );

    // Teeth: content NOT in the fixture must be absent — proves the positive
    // assertions are bound to real rendered content, not always-true.
    assert!(
        !snap.has_label_contains("nonexistent_col_zzz"),
        "a column name absent from the table must not render as inspector content"
    );
    assert!(
        !snap.has_label("id · DOUBLE"),
        "the id column's real profiled type is BIGINT, not DOUBLE"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// SQL console result-pane + timing chip render as AccessKit content (Task 7).
// ----------------------------------------------------------------------------

/// Genuinely execute `SELECT 1 AS x` through the engine (create the result VIEW
/// + build a `GridDataSource` over it — its ctor runs the async schema/row-count
/// probes, so the query really runs), mount a standalone [`SqlConsole`],
/// replicate `WorkspaceShell::finish_sql_run`'s Pane arm via the console's `pub`
/// result-binding setters, settle the page-0 prefetch, then assert the console
/// renders (a) the timing chip and (b) the result cell value `1` as AccessKit
/// content — the rendered text gpui itself cannot extract (UAT Gap 2).
///
/// ## Why replicate finish_sql_run instead of driving it
/// The production run pipeline — `toggle_sql_console` (build+mount the console),
/// `spawn_sql_run` → `finish_sql_run` (run it, then bind the result) — is
/// entirely `pub(crate)`, so none of it is reachable from an integration test (a
/// different crate): the same wall the inspector test hit. This test therefore
/// executes the query itself and calls the console's `pub` result-binding
/// setters — the same result-binding calls `finish_sql_run`'s Pane arm makes
/// (`set_last_elapsed`, `set_pane_source`, `set_region(Pane)`); its extra
/// `set_running(false)` is elided as a no-op for a never-started console
/// (`running` defaults false). The console's real `render` then emits the
/// timing chip + mounts the pane grid.
///
/// ## Result cells reuse the shared grid delegate (no separate annotation)
/// `SqlConsole::render`'s Pane arm builds a `GridTableDelegate` and renders it in
/// a `Table` — the SAME delegate the main grid uses, so its `render_td` cell
/// values are ALREADY emitted as `Cell` nodes (Task 5). Task 7 therefore adds no
/// new cell annotation; only the timing/status/error/cancelled/running text is
/// newly annotated. Hence the cell assertion below is served by the Task-5
/// annotation and the timing assertion by the Task-7 one.
///
/// ## Determinism
/// Asserts only STABLE content: the chip's "ms" + "· local" substrings (never
/// the elapsed-ms number, which is wall-clock) and the fixed result value `1`.
#[gpui::test]
#[serial]
fn sql_console_renders_result_and_timing_content(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test (engine paths need spawn_blocking)
    init_components(cx);
    // The console's per-tab editor is a `code_editor("sql")`; register the SQL
    // grammar exactly as production does at boot so the editor builds/highlights
    // as it does in the app (idempotent — safe to call once here).
    dat0_app::query::highlight::register_sql_language();
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    let session = build_empty_session_in(&harness, state.path());
    let engine = session.lock().engine.clone();

    // Genuinely run `SELECT 1 AS x` exactly as spawn_sql_run's Result arm does:
    // create the result VIEW then build a `GridDataSource` over it (the ctor runs
    // the async schema + row-count probes → the query really executes). Driven to
    // completion on the harness runtime via `block_on`.
    let stmt = "SELECT 1 AS x";
    let view_name = "sql_result_pane".to_string();
    let started = std::time::Instant::now();
    harness
        .block_on(async { engine.create_or_replace_view(&view_name, stmt).await })
        .expect("create_or_replace_view SELECT 1");
    let ds = harness
        .block_on(async { GridDataSource::new(Arc::clone(&engine), view_name.clone()).await })
        .expect("GridDataSource::new over the result view");
    let ds = Arc::new(ds);
    let elapsed_ms = started.elapsed().as_millis() as u64;
    // No MotherDuck attached → Local routing → chip suffix "· local".
    let routing = dat0_app::connections::routing::classify_routing(stmt, &[]);
    assert_eq!(
        routing,
        dat0_app::connections::routing::Routing::Local,
        "a local SELECT with no MD attached routes Local"
    );

    // Mount the console standalone; its production `render` now runs each frame.
    let (console, cx) = open_sql_console_window(cx);
    cx.run_until_parked();

    // Replicate finish_sql_run's Pane arm (see doc comment). `set_pane_source`
    // kicks the first-page prefetch (a `tokio::spawn`) and posts its re-render
    // notify back through the installed dispatcher — same as production.
    let ds_for_bind = Arc::clone(&ds);
    console.update(cx, |c, cx| {
        c.set_last_elapsed(elapsed_ms, routing, cx);
        c.set_pane_source(ds_for_bind, gpui::WeakEntity::new_invalid(), cx);
        c.set_region(ResultRegion::Pane, cx);
    });

    // Settle page 0 into the pane grid LRU (else render_td paints the em-dash
    // placeholder and the cell value would be absent) — same recipe as the grid
    // cell test above.
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
        "page 0 must load into the pane grid LRU so render_td paints the real \
         cell value (else it is the em-dash placeholder)"
    );

    let snap = A11ySnapshot::capture(cx);

    // (a) Timing chip: assert STABLE substrings only — never the elapsed-ms
    // number (wall-clock, non-deterministic). The chip renders "⏱ N ms · local".
    assert!(
        snap.has_label_contains("ms"),
        "the timing chip must render as AccessKit content (⏱ N ms · local)"
    );
    assert!(
        snap.has_label_contains("· local"),
        "the timing chip's routing tag (· local) must render"
    );

    // (b) Result cell value: the SELECT 1 result renders as a Cell node. The
    // pane grid reuses the virtualized `Table`, which re-runs render_td a
    // non-uniform number of times, so assert EXISTENCE (Task-5 finding), not
    // multiplicity.
    assert!(
        snap.has_label_any("1"),
        "the SELECT 1 result cell must render as an AccessKit Cell node (count {})",
        snap.count_label("1")
    );

    // Teeth: content NOT produced by this run must be absent — proves the
    // positives are bound to real rendered content, not always-true.
    assert!(
        !snap.has_label_any("424242"),
        "a value not in the result must not render as a Cell node"
    );
    assert!(
        !snap.has_label_contains("· remotexyz"),
        "a routing tag this run never produced must not render"
    );

    // Settle remaining detached-task work while `state` is still alive.
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(50));
        cx.run_until_parked();
    }

    drop(state);
}

// ----------------------------------------------------------------------------
// Error banner renders title/body as AccessKit content (Task 8).
// ----------------------------------------------------------------------------

/// `render_banner` is a pure fn of `&Banner` (no `cx`, no window, no engine), so
/// — like the inspector/SQL tests, which hit the same crate-boundary wall (the
/// shell's `banners` vec and its `pub(crate)` push path are not driven from an
/// integration crate) — we call the production `render_banner` directly and
/// capture the AccessKit nodes it emits at element-BUILD time (`.a11y_label`
/// pushes synchronously into the thread-local FRAME, not at paint), then assert
/// the title renders as an `Alert` node and the body as a `Label` node. Calling
/// it directly is an implementer judgment call given the fn's pure signature —
/// not attributed to the brief/plan/design. No window/refresh bracket is needed:
/// with no mounted view there is exactly one render by construction, so we build
/// the snapshot straight from the raw `take_tree_update()` (same as the inspector
/// test).
///
/// Determinism: the asserted title/body are fixed literal strings — no
/// timestamps/paths/random.
#[gpui::test]
#[serial]
fn banner_renders_title_and_body_content(cx: &mut TestAppContext) {
    use dat0_app::a11y::AccessRole;
    use dat0_app::error_ux::banner::{Banner, render_banner};

    const TITLE: &str = "Disk almost full";
    const BODY: &str = "Free up space before importing large files.";
    let banner = Banner::warning_with_body(TITLE, BODY);

    // Build the element (fires the `.a11y_label` pushes) inside an App context;
    // reset the collector immediately before so the captured tree holds ONLY
    // this banner's nodes.
    dat0_app::a11y::reset();
    cx.update(|_app| {
        let _ = render_banner(&banner);
    });
    let cap = dat0_app::a11y::take_tree_update();
    let snap = A11ySnapshot {
        state: kittest::State::new(cap.update),
        click_ids: cap.click_ids,
    };

    // Title → `Alert` node (always rendered). `query_by_role` disambiguates by
    // role and is safe (exactly one Alert node with this title).
    assert!(
        snap.query_by_role(AccessRole::Alert, TITLE),
        "banner title must render as an AccessKit Alert node"
    );
    // Body → `Label` node (rendered because non-empty).
    assert!(
        snap.has_label(BODY),
        "banner body must render as an AccessKit Label node"
    );

    // Teeth: content this banner never rendered must be absent — proves the
    // positives are bound to real rendered content, not always-true.
    assert!(
        !snap.query_by_role(AccessRole::Alert, "A title this banner never set"),
        "a title absent from the banner must not render as an Alert node"
    );
    assert!(
        !snap.has_label("Body text that was never set"),
        "body text absent from the banner must not render as a Label node"
    );
}
