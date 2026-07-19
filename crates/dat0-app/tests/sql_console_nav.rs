//! SQL Console keyboard-nav behavioral coverage (UAT carve-out #5).
//!
//! Windowed tests driving the SHIPPED SQL Console toolbar + tab strip through
//! the real keystroke path. Harness helpers are copied per-binary from
//! `tests/ai_nav.rs` (this crate's per-binary-copy precedent).

mod support;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;

use support::{A11ySnapshot, press_tab};

use dat0_app::query::ResultTarget;
use dat0_app::session::Session;
use dat0_app::view::sql_console::{SqlConsole, SqlConsoleEvent};
use dat0_app::window::WorkspaceShell;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Point `config_dir()` at `dir` for the rest of this (serial) test.
fn set_config_dir(dir: &Path) {
    // SAFETY: tests are `#[serial]`, so no other thread races this process-global
    // write; each test sets it before doing anything that reads `config_dir()`.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

/// Build a real, EMPTY in-memory session inside a dedicated multi-thread tokio
/// runtime (`Session::new` is async + uses `spawn_blocking`). An empty session
/// renders the empty-state hero, and with `first_run_done` unset the ENRICHED
/// band (which paints `hero-take-tour`) is shown.
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

/// Open a real ACTIVATED window whose root is a `gpui_component::Root` wrapping a
/// fresh `WorkspaceShell` over `session` (mirrors production `open_window_view`).
/// Activation makes `cx.active_window()` (which `onboarding::open` relies on)
/// resolve to it.
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

/// Initialise the gpui-component theme global + key bindings — required before
/// any gpui-component widget renders AND before `Root`'s "tab"/"shift-tab"
/// bindings (→ `focus_next`/`focus_prev`) are live.
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
}

/// A tokio runtime kept alive for the whole test so foreground-polled `cx.spawn`
/// futures can call `tokio::task::spawn_blocking`. Needed here because
/// `toggle_sql_console` (inside `open_console_ready_for_test`) calls
/// `refresh_completion_snapshot`, which `tokio::spawn`s the off-thread
/// `get_tables` — without an ambient runtime that spawn panics ("no reactor
/// running"). Copied from `tests/motherduck_window.rs:77-101` (per-binary-copy
/// precedent; its routing-chip test hits the same console-open path).
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.rt.enter()
    }
    #[allow(dead_code)]
    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }
}

fn enter_async_harness(cx: &mut TestAppContext) -> AsyncHarness {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    cx.executor().allow_parking();
    AsyncHarness { rt }
}

/// Focus the shell the way a real keyboard user does: click a neutral spot in
/// the enriched band's top row (60px left of the take-tour button, which sits
/// top-RIGHT after a flex-grow tagline with no click handler of its own), then
/// PROVE the click landed on the shell's own focus handle and NOT a wired hero
/// button — the T1 review hardening of the T0 spike's precondition (see
/// `keyboard_nav.rs` for the full history of this helper).
fn focus_shell_neutrally(cx: &mut VisualTestContext) {
    let tt_bounds = cx
        .debug_bounds("hero-take-tour")
        .expect("hero-take-tour must be painted");
    let focus_pt = gpui::point(tt_bounds.origin.x - gpui::px(60.), tt_bounds.center().y);
    cx.simulate_click(focus_pt, gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.update(|window, app| window.focused(app).is_some()),
        "clicking into the workspace must focus something (precondition for Tab)"
    );
    let snap = A11ySnapshot::capture(cx);
    assert!(
        snap.focused_label().is_none(),
        "neutral click must land on the shell's own focus handle, not a wired \
         hero button (focus oracle named {:?}) — the offset needs to move to a \
         truly neutral point",
        snap.focused_label()
    );
}

/// Tab from the neutral shell focus until `want` is the focused stop, or panic
/// after 20 hops (catalog_nav.rs `tab_to_catalog` idiom).
fn tab_until(cx: &mut VisualTestContext, want: &str) {
    for _ in 0..20 {
        press_tab(cx);
        if A11ySnapshot::capture(cx).focused_label() == Some(want) {
            return;
        }
    }
    panic!("`{want}` was never the focused Tab stop within 20 hops");
}

/// Collect the labels Tab visits, in order, up to `n` hops (stops early on repeat).
fn tab_labels(vcx: &mut VisualTestContext, n: usize) -> Vec<String> {
    let mut seen = Vec::new();
    for _ in 0..n {
        press_tab(vcx);
        if let Some(l) = A11ySnapshot::capture(vcx).focused_label() {
            let l = l.to_string();
            if seen.last() != Some(&l) {
                seen.push(l);
            }
        }
    }
    seen
}

/// Press Tab (up to `budget` times) until the tab-strip container holds focus.
/// Returns true if reached. Mirrors `tab_until` but keys off the focus accessor
/// (the tab strip's accessible name is the dynamic active-tab title).
fn tab_until_tabstrip(
    vcx: &mut VisualTestContext,
    console: &Entity<SqlConsole>,
    budget: usize,
) -> bool {
    for _ in 0..budget {
        press_tab(vcx);
        let f = vcx.update(|window, app| console.read(app).tabstrip_focused_for_test(window));
        if f {
            return true;
        }
    }
    false
}

/// Open the console ready and subscribe to its events. Returns (console, log).
fn open_console_with_log(
    shell: &Entity<WorkspaceShell>,
    vcx: &mut VisualTestContext,
) -> (Entity<SqlConsole>, Rc<RefCell<Vec<SqlConsoleEvent>>>) {
    let console = vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.open_console_ready_for_test(window, cx))
    });
    vcx.run_until_parked();
    let log: Rc<RefCell<Vec<SqlConsoleEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let log2 = log.clone();
    // NOTE: the returned Subscription is intentionally leaked for the test's life
    // via mem::forget so it keeps firing (the test process is short-lived).
    let sub = vcx.cx.update(|app| {
        app.subscribe(&console, move |_c, ev: &SqlConsoleEvent, _app| {
            log2.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(sub);
    vcx.run_until_parked(); // flush the deferred subscription activation
    (console, log)
}

/// T0 HARD GATE — proves the two drive patterns the slice rests on:
///   Probe 1: `sql-run` is Tab-reachable across the shell→console boundary
///            (the oracle names it by its `sql.run` label).
///   Probe 2: Enter on the focused Run button emits `SqlConsoleEvent::Run`.
///   Probe 3: the tab strip is Tab-reachable and `left` switches the active tab
///            (auto-activate) and emits `Persist`.
///   Probe 4: `delete` on the focused tab strip closes a tab (count 2 → 1).
#[gpui::test]
#[serial]
fn t0_sql_console_nav_gate(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, log) = open_console_with_log(&shell, vcx);

    // Seed a 2nd tab so switch/close have something to act on. `new_tab` needs a
    // `&mut Window`; it makes the new tab active (active == 1 afterwards).
    vcx.update(|window, app| console.update(app, |c, cx| c.new_tab(window, cx)));
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()),
        2,
        "seed: 2 tabs open"
    );

    let run = dat0_i18n::t("sql.run");

    // Probe 1: Run is Tab-reachable.
    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 60);
    assert!(
        seen.contains(&run),
        "STOP-1: sql-run must be Tab-reachable across the shell→console boundary; visited {seen:?}"
    );

    // Probe 2: Enter on Run emits Run.
    focus_shell_neutrally(vcx);
    tab_until(vcx, &run);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Run { .. })),
        "STOP-2: Enter on sql-run must emit Run; got {:?}",
        log.borrow()
    );

    // Probe 3: reach the tab strip, then `left` switches active (2nd tab → 1st)
    // and emits Persist. (Reach oracle is focus on the container handle — the
    // strip's accessible name is the DYNAMIC active-tab title, so `tab_until`
    // by label can't name it; hoisted `vcx.update` form per the brief's note.)
    focus_shell_neutrally(vcx);
    let mut reached = false;
    for _ in 0..60 {
        press_tab(vcx);
        let f = vcx.update(|window, app| console.read(app).tabstrip_focused_for_test(window));
        if f {
            reached = true;
            break;
        }
    }
    assert!(reached, "STOP-3: the tab strip must be Tab-reachable");
    let before = console.read_with(&vcx.cx, |c, _| c.active_tab_for_test());
    assert_eq!(before, 1, "seed: new tab is active");
    log.borrow_mut().clear();
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.active_tab_for_test()),
        0,
        "STOP-3: left must switch the active tab 1 → 0"
    );
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Persist)),
        "STOP-3: switching a tab must emit Persist; got {:?}",
        log.borrow()
    );

    // Probe 4: `delete` on the focused tab strip closes a tab (2 → 1).
    vcx.simulate_keystrokes("delete");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()),
        1,
        "STOP-4: delete on the focused tab strip must close the active tab"
    );
    drop(state);
}

/// Every fixed toolbar button is Tab-reachable (labels appear as Tab walks the
/// console). Uses the label oracle — each button carries its localized `.a11y`
/// twin (glyph child, text label).
#[gpui::test]
#[serial]
fn toolbar_buttons_reachable(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, _log) = open_console_with_log(&shell, vcx);

    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 80);
    for key in [
        "sql.run",
        "sql.run_in_pane",
        "sql.new_tab",
        "sql.history",
        "sql.save_query",
        "sql.load_query",
        "sql.save_as_table",
    ] {
        let label = dat0_i18n::t(key);
        assert!(
            seen.contains(&label),
            "{key} ({label:?}) Tab-reachable; visited {seen:?}"
        );
    }
    drop(state);
}

/// Enter on the focused Run button emits `Run { MainGrid }` while idle.
#[gpui::test]
#[serial]
fn enter_on_run_emits_run(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, log) = open_console_with_log(&shell, vcx);

    let run = dat0_i18n::t("sql.run");
    focus_shell_neutrally(vcx);
    tab_until(vcx, &run);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow().iter().any(|e| matches!(
            e,
            SqlConsoleEvent::Run {
                target: ResultTarget::MainGrid
            }
        )),
        "Enter on Run must emit Run{{MainGrid}}; got {:?}",
        log.borrow()
    );
    drop(state);
}

/// While running, the same control shows Cancel and Enter emits Cancel.
#[gpui::test]
#[serial]
fn enter_on_run_while_running_emits_cancel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, log) = open_console_with_log(&shell, vcx);

    // Force the running state so the primary button is Cancel.
    vcx.update(|_w, app| console.update(app, |c, cx| c.set_running(true, cx)));
    vcx.run_until_parked();

    let cancel = dat0_i18n::t("sql.cancel");
    focus_shell_neutrally(vcx);
    tab_until(vcx, &cancel);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Cancel)),
        "Enter on the running Cancel button must emit Cancel; got {:?}",
        log.borrow()
    );
    drop(state);
}

/// Seed 3 tabs; from the focused tab strip, ← / → move the active tab and clamp
/// at both ends.
#[gpui::test]
#[serial]
fn tabstrip_arrows_switch_and_clamp(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, _log) = open_console_with_log(&shell, vcx);

    // 1 → 3 tabs (each new_tab makes itself active; end active == 2).
    for _ in 0..2 {
        vcx.update(|window, app| console.update(app, |c, cx| c.new_tab(window, cx)));
        vcx.run_until_parked();
    }
    assert_eq!(console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()), 3);

    focus_shell_neutrally(vcx);
    assert!(tab_until_tabstrip(vcx, &console, 60), "tab strip reachable");
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.active_tab_for_test()),
        2
    );

    // → at the right edge clamps (stays 2).
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.active_tab_for_test()),
        2,
        "right clamps at end"
    );

    // ← walks back to 0 and clamps.
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.active_tab_for_test()),
        1
    );
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.active_tab_for_test()),
        0
    );
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.active_tab_for_test()),
        0,
        "left clamps at start"
    );

    // → moves forward again (auto-activate).
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.active_tab_for_test()),
        1
    );
    drop(state);
}

/// Delete on the focused tab strip closes the active tab (count drops, active
/// clamps).
#[gpui::test]
#[serial]
fn tabstrip_delete_closes_active(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, _log) = open_console_with_log(&shell, vcx);

    vcx.update(|window, app| console.update(app, |c, cx| c.new_tab(window, cx)));
    vcx.run_until_parked();
    assert_eq!(console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()), 2);

    focus_shell_neutrally(vcx);
    assert!(tab_until_tabstrip(vcx, &console, 60), "tab strip reachable");
    vcx.simulate_keystrokes("delete");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()),
        1,
        "delete closes the active tab"
    );
    drop(state);
}

/// Delete with a single tab open is a no-op (never an empty console).
#[gpui::test]
#[serial]
fn tabstrip_delete_last_tab_is_noop(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, _log) = open_console_with_log(&shell, vcx);

    assert_eq!(console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()), 1);
    focus_shell_neutrally(vcx);
    assert!(tab_until_tabstrip(vcx, &console, 60), "tab strip reachable");
    vcx.simulate_keystrokes("delete");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()),
        1,
        "delete on the last tab must be a no-op"
    );
    drop(state);
}

/// With the console CLOSED, none of the new toolbar labels are Tab stops (the
/// console render doesn't paint, so the `.a11y` twins are absent).
#[gpui::test]
#[serial]
fn toolbar_not_tab_stops_when_console_closed(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (_shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // Do NOT open the console.

    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 60);
    let run = dat0_i18n::t("sql.run");
    assert!(
        !seen.contains(&run),
        "with the console closed, sql-run must not be a Tab stop; visited {seen:?}"
    );
    drop(state);
}
