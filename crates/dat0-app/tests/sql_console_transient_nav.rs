//! SQL Console transient-bars keyboard-nav coverage (UAT carve-out #7, Task 1).
//!
//! Windowed tests driving the NL→SQL preview strip's focus management (the
//! `pending_focus` render-drain + the consolidated Escape ladder's NL rung)
//! through the real keystroke path. Harness helpers are copied per-binary from
//! `tests/sql_console_nav.rs` (this crate's per-binary-copy precedent).
//!
//! `#![allow(dead_code, unused_imports)]` rationale: the harness is copied
//! verbatim as a shared/growing helper set (mirrors `tests/input_nav.rs`); Task
//! 1's four tests exercise only a subset — the Tab-nav helpers and some imports
//! are here for the breadth suites Tasks 2-5 add to this same file.
#![allow(dead_code, unused_imports)]

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

/// T0 HARD GATE — de-risks the three empirical unknowns before any breadth:
///   Probe 1: injecting a streaming NL preview moves focus to `nl2sql-stop`
///            (focus-on-appear + the harness can observe the transient strip).
///   Probe 2: finishing the preview RE-HOMES focus to `nl2sql-insert` across the
///            Stop→Insert button swap (focus is not dropped to nowhere).
///   Probe 3: Escape while the finished strip is open discards it and returns
///            focus to the editor (the Escape ladder's NL rung routes).
#[gpui::test]
#[serial]
fn t0_transient_bars_gate(cx: &mut TestAppContext) {
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

    let stop = dat0_i18n::t("sql.ai.stop");
    let insert = dat0_i18n::t("sql.nl2sql.insert");

    // Probe 1: streaming preview → focus lands on Stop.
    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.begin_nl_preview_for_test("top users".into(), cx)
        })
    });
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(stop.as_str()),
        "STOP-1: focus must move to nl2sql-stop when the streaming strip appears"
    );

    // Probe 2: finish → focus re-homes to Insert across the button swap.
    vcx.update(|_w, app| console.update(app, |c, cx| c.finish_nl_preview_for_test(None, cx)));
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(insert.as_str()),
        "STOP-2: focus must re-home to nl2sql-insert when the stream finishes"
    );

    // Probe 3: Escape discards the finished strip and returns to the editor.
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.nl_preview_open_for_test()),
        "STOP-3: Escape must discard the finished NL strip"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "STOP-3: focus must return to the editor after discard"
    );
}

#[gpui::test]
#[serial]
fn nl2sql_stop_emits_stopaistream(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| console.update(app, |c, cx| c.begin_nl_preview_for_test("q".into(), cx)));
    vcx.run_until_parked(); // focus is on Stop (T0 Probe 1)
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::StopAiStream)),
        "Enter on the focused Stop must emit StopAiStream; got {:?}",
        log.borrow()
    );
}

#[gpui::test]
#[serial]
fn nl2sql_insert_opens_tab_and_returns_focus(cx: &mut TestAppContext) {
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

    let before = console.read_with(&vcx.cx, |c, _| c.tab_count_for_test());
    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.begin_nl_preview_for_test("q".into(), cx);
            c.push_nl_delta_for_test("SELECT 1", cx);
            c.finish_nl_preview_for_test(None, cx);
        })
    });
    vcx.run_until_parked(); // focus is on Insert (T0 Probe 2)
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()),
        before + 1,
        "Insert must open a new tab with the generated SQL"
    );
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.nl_preview_open_for_test()),
        "Insert must consume the preview"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "Insert must return focus to the (new tab's) editor"
    );
}

#[gpui::test]
#[serial]
fn nl2sql_discard_returns_focus_to_editor(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.begin_nl_preview_for_test("q".into(), cx);
            c.finish_nl_preview_for_test(None, cx);
        })
    });
    vcx.run_until_parked(); // focus on Insert; Discard is the next tab stop
    support::press_tab(vcx); // Insert (index 0) → Discard (index 1)
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.nl2sql.discard").as_str()),
        "Tab must reach Discard after Insert"
    );
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.nl_preview_open_for_test()),
        "Discard must close the strip"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "Discard must return focus to the editor"
    );
}

#[gpui::test]
#[serial]
fn explain_focuses_stop_then_rehomes_close(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.begin_explain_for_test("SELECT 1".into(), cx))
    });
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.ai.stop").as_str()),
        "streaming Explain must focus Stop"
    );
    vcx.update(|_w, app| console.update(app, |c, cx| c.finish_explain_for_test(None, cx)));
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.explain.close").as_str()),
        "finished Explain must re-home focus to Close"
    );
}

#[gpui::test]
#[serial]
fn explain_close_emits_and_returns_focus(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.begin_explain_for_test("SELECT 1".into(), cx);
            c.finish_explain_for_test(None, cx);
        })
    });
    vcx.run_until_parked(); // focus on Close
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::CloseExplain)),
        "Enter on Close must emit CloseExplain; got {:?}",
        log.borrow()
    );
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.explain_open_for_test()),
        "the shell's CloseExplain handler must clear the panel"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "Close must return focus to the editor"
    );
}

#[gpui::test]
#[serial]
fn explain_escape_streaming_stops_finished_closes(cx: &mut TestAppContext) {
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

    // Streaming: Escape emits StopAiStream.
    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.begin_explain_for_test("SELECT 1".into(), cx))
    });
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::StopAiStream)),
        "Escape while Explain streams must emit StopAiStream"
    );
}
