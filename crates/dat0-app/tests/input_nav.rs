//! SQL-console `Input` keyboard-operability coverage (UAT carve-out #6).
//!
//! Windowed tests driving the SHIPPED SQL editor trap-exit (Escape → Run) and
//! the shared `NamePrompt` modal (focus-on-open, Enter-submit, Escape-cancel,
//! keyboard-reachable OK/Cancel) through the real dispatch path. Harness helpers
//! are copied per-binary from `tests/sql_console_nav.rs` (per-binary-copy precedent).
//! Inputs are driven with `cx.dispatch_action(...)`, NEVER `simulate_keystrokes`
//! (the cell-editor slice proved a stray "\n" panics a single-line Input).
//!
//! T0 (this file's first test, `t0_input_nav_gate`) only exercises a subset of
//! the copied harness surface — `ResultTarget`, the `NamePrompt` type name, and
//! `tab_until` are here for the breadth suites Tasks 2-3 add to this same file,
//! per the copied-verbatim harness convention (mirrors `tests/support/mod.rs`'s
//! `#![allow(dead_code)]` rationale: a shared/growing helper set, not every
//! caller exercises every helper).
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
use dat0_app::view::name_prompt::{NamePrompt, NamePromptEvent};
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

/// T0 HARD GATE — proves the three drive mechanisms the slice rests on:
///   Probe 1: focus the editor, `dispatch_action(Escape)` → focus lands on Run
///            (the ancestor Escape→Run handler fires; editor no longer focused).
///   Probe 2: with the editor focused, the run shortcut still runs — dispatch the
///            `SqlRun` menu action and assert the console reflects a run request.
///   Probe 3: open a NamePrompt → its field is focused; seed a value;
///            `dispatch_action(Enter)` → `Confirm(value)` emitted + overlay dismissed.
///   Probe 4: re-open → `dispatch_action(Escape)` → `Cancel` emitted + dismissed;
///            OK/Cancel are Tab-reachable (labels "Save"/"Cancel").
#[gpui::test]
#[serial]
fn t0_input_nav_gate(cx: &mut TestAppContext) {
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

    // ── Probe 1: editor trap-exit (Escape → Run) ────────────────────────────
    let editor_fh = vcx.update(|_w, app| console.read(app).editor_focus_handle_for_test(app));
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "precondition: editor focused"
    );
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        !vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "STOP-1: Escape must move focus OUT of the editor"
    );
    let run = dat0_i18n::t("sql.run");
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(run.as_str()),
        "STOP-1: Escape must land focus on Run; got {:?}",
        A11ySnapshot::capture(vcx).focused_label()
    );

    // ── Probe 2: run shortcut still works from the editor ────────────────────
    // STOP-2 fallback (b): `SqlRun`'s handler on `WorkspaceShell` (window.rs
    // ~6631) calls `spawn_sql_run` DIRECTLY — it never emits a
    // `SqlConsoleEvent::Run` (that event is a separate path, fired only by the
    // toolbar Run button's own click/focus_stop handlers). So the brief's
    // `SqlConsoleEvent::Run` observable is unreachable via this action; per the
    // brief's own STOP-2(b) note ("assert the real observable instead ... the
    // point is the run shortcut works while the editor is focused, not the
    // specific event"), assert on `SqlConsole.running` (already `pub`) instead
    // of adding a redundant `is_running_for_test()` accessor.
    //
    // `spawn_sql_run` also bails out before touching `running` when the
    // statement-under-cursor is empty (window.rs:4044) — a fresh tab's editor
    // starts empty — so seed real SQL first via the already-`pub`
    // `tabs`/`input` fields (no new accessor needed for this either).
    vcx.update(|window, app| {
        console.update(app, |c, cx| {
            let input = c.tabs[c.active].input.clone();
            input.update(cx, |s, cx| s.set_value("SELECT 1", window, cx));
        })
    });
    vcx.run_until_parked();
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "precondition: editor focused before dispatching the run action"
    );
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.running),
        "precondition: not running before dispatch"
    );
    vcx.dispatch_action(dat0_app::menu_macos::SqlRun);
    vcx.run_until_parked();
    assert!(
        console.read_with(&vcx.cx, |c, _| c.running),
        "STOP-2: the run action must start a run (SqlConsole.running) while the editor is focused"
    );

    // ── Probe 3: NamePrompt opens focused, Enter submits ─────────────────────
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.open_name_prompt_for_test(window, cx)));
    vcx.run_until_parked();
    let prompt = vcx
        .update(|_w, app| shell.read(app).name_prompt_entity_for_test())
        .expect("STOP-3: prompt must be open");
    assert!(
        vcx.update(|window, app| prompt.read(app).input_focused_for_test(window, app)),
        "STOP-3: the prompt field must be focused on open"
    );
    let plog: std::rc::Rc<std::cell::RefCell<Vec<NamePromptEvent>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let plog2 = plog.clone();
    let psub = vcx.cx.update(|app| {
        app.subscribe(&prompt, move |_p, ev: &NamePromptEvent, _app| {
            plog2.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(psub);
    vcx.update(|window, app| {
        prompt.update(app, |p, cx| p.seed_value_for_test("hello", window, cx))
    });
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Enter { secondary: false });
    vcx.run_until_parked();
    assert!(
        plog.borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Confirm(v) if v == "hello")),
        "STOP-3: Enter must emit Confirm(value); got {:?}",
        plog.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "STOP-3: Confirm must dismiss the overlay"
    );

    // ── Probe 4: re-open, Escape cancels; OK/Cancel Tab-reachable ────────────
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.open_name_prompt_for_test(window, cx)));
    vcx.run_until_parked();
    let prompt2 = vcx
        .update(|_w, app| shell.read(app).name_prompt_entity_for_test())
        .expect("prompt re-open");
    let plog3 = plog.clone();
    let psub2 = vcx.cx.update(|app| {
        app.subscribe(&prompt2, move |_p, ev: &NamePromptEvent, _app| {
            plog3.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(psub2);
    let seen = tab_labels(vcx, 30);
    assert!(
        seen.contains(&"Save".to_string()) && seen.contains(&"Cancel".to_string()),
        "STOP-4: OK/Cancel must be Tab stops; visited {seen:?}"
    );
    // This slice does not add a Tab focus-trap to the modal (out of scope), so
    // the 30-hop walk above can wander focus into the background shell once it
    // walks past the modal's 3 stops. Land focus back INSIDE the modal before
    // proving Escape-cancels-from-within-the-modal — otherwise the dispatch
    // bubbles from whatever background element Tab last landed on, which is
    // not a descendant of the modal's `on_action` handler.
    let prompt2_input_fh = vcx.update(|_w, app| prompt2.read(app).input_focus_handle_for_test(app));
    vcx.update(|window, _| window.focus(&prompt2_input_fh));
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        plog.borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Cancel)),
        "STOP-4: Escape must emit Cancel; got {:?}",
        plog.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "STOP-4: Cancel must dismiss the overlay"
    );
    drop(state);
}

/// Escape from the focused editor lands focus on Run, then Tab/Shift-Tab resume
/// normal navigation (proves the trap is genuinely broken open).
#[gpui::test]
#[serial]
fn editor_escape_exits_to_run_then_tab_resumes(cx: &mut TestAppContext) {
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

    let editor_fh = vcx.update(|_w, app| console.read(app).editor_focus_handle_for_test(app));
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    let run = dat0_i18n::t("sql.run");
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(run.as_str()),
        "Escape lands on Run"
    );
    // Focus can now leave Run by Tab (no longer trapped): a subsequent Tab
    // reaching another stop proves nav resumed.
    press_tab(vcx);
    assert!(
        A11ySnapshot::capture(vcx).focused_label().is_some(),
        "Tab from Run reaches another stop (nav resumed, not trapped)"
    );
    drop(state);
}

/// Escape does nothing observable when the editor is NOT focused (the guard):
/// focus a toolbar button, Escape, and the focus label is unchanged (Run is not
/// force-grabbed).
#[gpui::test]
#[serial]
fn editor_escape_guarded_to_editor_focus(cx: &mut TestAppContext) {
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
    tab_until(vcx, &dat0_i18n::t("sql.history"));
    let before = A11ySnapshot::capture(vcx)
        .focused_label()
        .map(str::to_string);
    assert_eq!(
        before.as_deref(),
        Some(dat0_i18n::t("sql.history").as_str())
    );
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx)
            .focused_label()
            .map(str::to_string),
        before,
        "Escape while a non-editor stop is focused must not hijack focus to Run"
    );
    drop(state);
}

/// The run shortcut (`SqlRun` menu action) starts a run while the editor is
/// focused. Mirrors `t0_input_nav_gate`'s Probe 2: `SqlRun`'s handler calls
/// `spawn_sql_run` directly (never emits `SqlConsoleEvent::Run`, which is a
/// separate path fired only by the toolbar Run button's own handlers) and
/// bails out on an empty statement-under-cursor, so real SQL is seeded first
/// via the already-`pub` `tabs`/`input` fields and the observable asserted is
/// `SqlConsole.running`.
#[gpui::test]
#[serial]
fn run_shortcut_works_from_editor(cx: &mut TestAppContext) {
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

    let editor_fh = vcx.update(|_w, app| console.read(app).editor_focus_handle_for_test(app));
    vcx.update(|window, app| {
        console.update(app, |c, cx| {
            let input = c.tabs[c.active].input.clone();
            input.update(cx, |s, cx| s.set_value("SELECT 1", window, cx));
        })
    });
    vcx.run_until_parked();
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "precondition: editor focused before dispatching the run action"
    );
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.running),
        "precondition: not running before dispatch"
    );
    vcx.dispatch_action(dat0_app::menu_macos::SqlRun);
    vcx.run_until_parked();
    assert!(
        console.read_with(&vcx.cx, |c, _| c.running),
        "run action from the editor must start a run (SqlConsole.running) while the editor is focused"
    );
    drop(state);
}

/// Open a NamePrompt and subscribe to its events. Returns (prompt, log).
fn open_prompt_with_log(
    shell: &Entity<WorkspaceShell>,
    vcx: &mut VisualTestContext,
) -> (
    Entity<NamePrompt>,
    std::rc::Rc<std::cell::RefCell<Vec<NamePromptEvent>>>,
) {
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.open_name_prompt_for_test(window, cx)));
    vcx.run_until_parked();
    let prompt = vcx
        .update(|_w, app| shell.read(app).name_prompt_entity_for_test())
        .expect("prompt open");
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log2 = log.clone();
    let sub = vcx.cx.update(|app| {
        app.subscribe(&prompt, move |_p, ev: &NamePromptEvent, _app| {
            log2.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(sub);
    (prompt, log)
}

/// The prompt field is focused on open (no click needed).
#[gpui::test]
#[serial]
fn prompt_focused_on_open(cx: &mut TestAppContext) {
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

    let (prompt, _plog) = open_prompt_with_log(&shell, vcx);
    assert!(
        vcx.update(|window, app| prompt.read(app).input_focused_for_test(window, app)),
        "the prompt field is focused on open"
    );
    drop(state);
}

/// Enter submits the typed value; the overlay dismisses.
#[gpui::test]
#[serial]
fn prompt_enter_confirms_value(cx: &mut TestAppContext) {
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

    let (prompt, plog) = open_prompt_with_log(&shell, vcx);
    vcx.update(|window, app| {
        prompt.update(app, |p, cx| p.seed_value_for_test("my_query", window, cx))
    });
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Enter { secondary: false });
    vcx.run_until_parked();
    assert!(
        plog.borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Confirm(v) if v == "my_query")),
        "Enter emits Confirm(value); got {:?}",
        plog.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "Confirm dismisses the overlay"
    );
    drop(state);
}

/// Escape cancels and dismisses; OK/Cancel are keyboard-reachable + operable
/// (Enter on the focused Cancel button emits Cancel).
#[gpui::test]
#[serial]
fn prompt_escape_cancels_and_buttons_operable(cx: &mut TestAppContext) {
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

    // Escape → Cancel.
    let (_p1, plog1) = open_prompt_with_log(&shell, vcx);
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        plog1
            .borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Cancel)),
        "Escape emits Cancel; got {:?}",
        plog1.borrow()
    );
    assert!(!vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()));

    // Buttons reachable + operable: Tab to Cancel, Enter → Cancel.
    let (_p2, plog2) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        plog2
            .borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Cancel)),
        "Enter on the focused Cancel button emits Cancel; got {:?}",
        plog2.borrow()
    );
    drop(state);
}
