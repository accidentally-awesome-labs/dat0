//! Modal focus-trap coverage (UI redesign B1).
//!
//! Drives Tab / Shift-Tab / Escape through the REAL keymap with
//! `simulate_keystrokes`. Never `dispatch_action` — that bypasses the keymap,
//! and the keymap is the mechanism under test (design doc §1: gpui dispatches
//! action bindings BEFORE `on_key_down`, so the trap has to be built from
//! actions bound under a deeper key context).
//!
//! Task 0's `gate_*` tests characterize the PRE-B1 behaviour and are inverted
//! into the real assertions by Task 3. They exist so the defect is proven to be
//! real before any fix is written.
//!
//! Harness helpers are copied per-binary from `tests/input_nav.rs` (the
//! per-binary-copy convention; see `tests/support/mod.rs` for the rationale).
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

use support::{A11ySnapshot, press_shift_tab, press_tab};

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
/// runtime (`Session::new` is async + uses `spawn_blocking`).
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
    // The harness calls only `gpui_component::init`, so the modal-scoped
    // bindings production registers in `run_app` are absent unless we add them
    // here (the carve-out #7 lesson: a green test over a dead key path).
    cx.update(dat0_app::overlay::register_modal_keys);
}

/// A tokio runtime kept alive for the whole test so foreground-polled `cx.spawn`
/// futures can call `tokio::task::spawn_blocking`.
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

/// Tab from the current focus until `want` is the focused stop, or panic after
/// 20 hops (catalog_nav.rs `tab_to_catalog` idiom).
fn tab_until(cx: &mut VisualTestContext, want: &str) {
    for _ in 0..20 {
        press_tab(cx);
        if A11ySnapshot::capture(cx).focused_label() == Some(want) {
            return;
        }
    }
    panic!("`{want}` was never the focused Tab stop within 20 hops");
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

/// Open a `NamePrompt` and subscribe to its events. Returns (prompt, log).
fn open_prompt_with_log(
    shell: &Entity<WorkspaceShell>,
    vcx: &mut VisualTestContext,
) -> (Entity<NamePrompt>, Rc<RefCell<Vec<NamePromptEvent>>>) {
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.open_name_prompt_for_test(window, cx)));
    vcx.run_until_parked();
    let prompt = vcx
        .update(|_w, app| shell.read(app).name_prompt_entity_for_test())
        .expect("prompt open");
    let log = Rc::new(RefCell::new(Vec::new()));
    let log2 = log.clone();
    let sub = vcx.cx.update(|app| {
        app.subscribe(&prompt, move |_p, ev: &NamePromptEvent, _app| {
            log2.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(sub);
    (prompt, log)
}

// ---------------------------------------------------------------------------
// Task 0 — hard gate. These characterize TODAY's behaviour; Task 3 inverts the
// two `gate_*` tests into the real assertions.
// ---------------------------------------------------------------------------

/// PRE-B1: Tab past Cancel leaves the modal entirely — the WCAG 2.4.3 gap
/// deferred out of kbd-nav carve-out #6 (`name_prompt.rs` doc comment).
/// Task 3 inverts this into `tab_wraps_from_last_stop_to_first`.
///
/// Two hops past Cancel, not one: a single hop could land on the modal's own
/// unlabelled text field and read as `None` for the wrong reason.
#[gpui::test]
#[serial]
fn gate_tab_escapes_the_modal_today(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    press_tab(vcx);
    press_tab(vcx);
    let after = A11ySnapshot::capture(vcx)
        .focused_label()
        .map(str::to_string);
    // Two assertions, because "not Save/Cancel" alone is vacuous: it would also
    // pass if focus had merely wrapped onto the modal's own UNLABELLED text
    // field. Requiring a `Some` label proves focus reached a real labelled stop
    // in the obscured background shell.
    assert!(
        after.is_some(),
        "PRE-B1 premise: Tab past Cancel reaches a labelled BACKGROUND stop, \
         not the modal's unlabelled field"
    );
    assert!(
        !matches!(after.as_deref(), Some("Save") | Some("Cancel")),
        "PRE-B1 premise: Tab past Cancel escapes the modal; landed on {after:?}"
    );
    drop(state);
}

/// PRE-B1: Escape does nothing once focus leaves the text field, because
/// `escape` is bound only under key context "Input"
/// (gpui-component `input/state.rs:120`). Task 3 inverts this into
/// `escape_from_cancel_dismisses`.
#[gpui::test]
#[serial]
fn gate_escape_from_cancel_is_dead_today(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        log.borrow().is_empty(),
        "PRE-B1 premise: Escape from Cancel emits nothing; got {:?}",
        log.borrow()
    );
    assert!(
        vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "PRE-B1 premise: the modal is still open after Escape from Cancel"
    );
    drop(state);
}

/// Control that must hold BEFORE and AFTER B1: one Escape with the text field
/// focused emits exactly ONE Cancel. Task 1 adds a second `escape` binding, so
/// two bindings then match; this guards against the cell-editor slice's
/// Enter-double-fire failure mode.
#[gpui::test]
#[serial]
fn escape_with_field_focused_emits_exactly_one_cancel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, log) = open_prompt_with_log(&shell, vcx);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    let cancels = log
        .borrow()
        .iter()
        .filter(|e| matches!(e, NamePromptEvent::Cancel))
        .count();
    assert_eq!(cancels, 1, "exactly one Cancel per Escape; got {cancels}");
    drop(state);
}

// ---------------------------------------------------------------------------
// Task 2 — the prompt declares its own focus order.
// ---------------------------------------------------------------------------

/// The prompt's declared focus order is exactly [field, Save, Cancel] — the
/// trap's only source of truth, so a render reorder must break this test.
#[gpui::test]
#[serial]
fn prompt_focus_order_is_field_ok_cancel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (prompt, _log) = open_prompt_with_log(&shell, vcx);
    let (order, field) = vcx.update(|_w, app| {
        let p = prompt.read(app);
        (p.focus_order(app), p.input_focus_handle_for_test(app))
    });
    assert_eq!(order.len(), 3, "field + Save + Cancel");
    assert_eq!(order[0], field, "the text field is first");
    drop(state);
}
