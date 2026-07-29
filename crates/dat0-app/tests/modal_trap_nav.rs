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

/// Focus the shell the way a real keyboard user does: click a neutral spot in
/// the enriched band's top row (60px left of the take-tour button, which sits
/// top-RIGHT after a flex-grow tagline with no click handler of its own), then
/// PROVE the click landed on the shell's own focus handle and NOT a wired hero
/// button (copied from `keyboard_nav.rs` / `input_nav.rs`).
///
/// LOAD-BEARING for any Tab-driven test: with NOTHING focused, the dispatch
/// path is the window root alone, so not even `Root`'s own "tab" binding
/// matches and Tab is completely inert. Measured here — eight `press_tab` hops
/// from a fresh window moved focus not at all.
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
         hero button (focus oracle named {:?})",
        snap.focused_label()
    );
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
// The trap. The first two tests started life as Task 0's `gate_*` probes,
// which asserted the PRE-B1 behaviour and both failed the moment T3 mounted
// `modal_host` — see the T0 and T3 commits for the red→green transition.
// ---------------------------------------------------------------------------

/// Tab past the last stop wraps to the first instead of escaping into the
/// obscured shell — the WCAG 2.4.3 gap deferred out of kbd-nav carve-out #6.
///
/// Two hops past Cancel, not one: the first lands on the modal's own UNLABELLED
/// text field (which reads as `None`), so only the second produces a label that
/// distinguishes "wrapped inside" from "escaped outside".
#[gpui::test]
#[serial]
fn tab_wraps_from_last_stop_to_first(cx: &mut TestAppContext) {
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
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some("Save"),
        "Tab past Cancel wraps to the field, then to Save — focus never leaves \
         the modal"
    );
    drop(state);
}

/// Escape cancels from ANY stop, not just the text field. Upstream binds
/// `escape` only under key context "Input" (gpui-component
/// `input/state.rs:120`), so before B1 this was dead once focus reached
/// OK/Cancel; the `Dat0Modal`-scoped binding fixes it.
#[gpui::test]
#[serial]
fn escape_from_cancel_dismisses(cx: &mut TestAppContext) {
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
        log.borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Cancel)),
        "Escape from Cancel emits Cancel; got {:?}",
        log.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "Escape from Cancel dismisses the modal"
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

/// Shift-Tab from the first stop wraps to the last.
#[gpui::test]
#[serial]
fn shift_tab_wraps_from_first_stop_to_last(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // The field holds focus on open, so one Shift-Tab wraps straight to Cancel.
    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    press_shift_tab(vcx);
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some("Cancel"),
        "Shift-Tab from the field wraps to the last stop"
    );
    drop(state);
}

/// Focus that has ESCAPED to a background element is pulled back in by the next
/// Tab — the `None` arm of `overlay::next_index`, and the difference between a
/// trap and a mere wrap.
///
/// The escape is staged with direct `window.focus_next()` calls, which bypass
/// the keymap entirely and so model the realistic hazard: async code that
/// focuses something while a modal is up. This is what forced the `Dat0Modal`
/// key context onto the shell ROOT rather than only the scrim — with it on the
/// scrim alone, focus was measured walking from one background hero button to
/// the next with the modal still open.
///
/// Not covered, and not coverable by an element-scoped key context: focus set to
/// NOTHING via `window.blur()`. The dispatch path is then the window root alone,
/// no element context is in scope, and `Root`'s Tab binding is the only match.
#[gpui::test]
#[serial]
fn tab_snaps_focus_back_into_the_modal(cx: &mut TestAppContext) {
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
    // Force focus out of the modal WITHOUT going through the trap: `focus_next`
    // is a direct API call, so no keystroke and no binding is involved. Three
    // hops clears the modal's own three stops.
    for _ in 0..3 {
        vcx.update(|window, _app| window.focus_next());
        vcx.run_until_parked();
    }
    let escaped = A11ySnapshot::capture(vcx)
        .focused_label()
        .map(str::to_string);
    assert!(
        !matches!(escaped.as_deref(), Some("Save") | Some("Cancel")),
        "precondition: focus really did escape to the background; sits on {escaped:?}"
    );

    // Now a REAL Tab keystroke, which must route through the trap.
    press_tab(vcx);
    vcx.run_until_parked();
    let landed = vcx.update(|window, app| {
        prompt
            .read(app)
            .focus_order(app)
            .first()
            .map(|h| h.is_focused(window))
            .unwrap_or(false)
    });
    assert!(
        landed,
        "Tab with focus outside the modal re-enters at the first stop"
    );
    drop(state);
}

/// Escape with the SQL console open closes the MODAL ONLY — the console behind
/// it stays. (The master plan's named B1 regression: one Escape must not walk
/// two rungs of the ladder.)
#[gpui::test]
#[serial]
fn escape_over_console_closes_only_the_modal(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, _clog) = open_console_with_log(&shell, vcx);

    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "the modal closed"
    );
    // The console has no `_for_test` visibility shim and this slice must not add
    // one; assert on what it PAINTS instead. `sql-run` renders "Run" while idle
    // (`sql_console.rs:826-830`), so the button's presence proves the console is
    // still mounted behind the dismissed modal.
    assert!(
        A11ySnapshot::capture(vcx)
            .query_by_role(dat0_app::a11y::AccessRole::Button, &dat0_i18n::t("sql.run"),),
        "the console behind it did NOT close"
    );
    drop(state);
}

/// The modal card emits a real `Dialog` node named by the prompt title —
/// `AccessRole::Dialog` had no production consumer before B1.
#[gpui::test]
#[serial]
fn modal_emits_a_named_dialog_node(cx: &mut TestAppContext) {
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
    let title = vcx.update(|_w, app| prompt.read(app).title().to_string());
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.query_by_role(dat0_app::a11y::AccessRole::Dialog, &title),
        "the modal card emits a Dialog node named {title:?}"
    );
    drop(state);
}

/// At most one modal is ever mounted. The three prompt fields are independent
/// `Option`s, so this invariant is representable-but-forbidden; a debug_assert
/// in each open path fails loudly if a future flow breaks it.
///
/// Load-bearing rather than cosmetic: `render` selects the trapped focus order
/// with an `or` chain over the same three fields, so a second mounted modal
/// would silently be the one NOT trapped.
#[gpui::test]
#[serial]
fn at_most_one_modal_is_open(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert_eq!(
        vcx.update(|_w, app| shell.read(app).open_modal_count_for_test(app)),
        0,
        "no modal before opening one"
    );
    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    assert_eq!(
        vcx.update(|_w, app| shell.read(app).open_modal_count_for_test(app)),
        1,
        "exactly one modal while a prompt is up"
    );
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| shell.read(app).open_modal_count_for_test(app)),
        0,
        "back to zero after dismiss"
    );
    drop(state);
}

/// Dismissing a modal returns focus to whatever held it before the modal opened.
/// Without this, closing a prompt strands focus and the next Tab restarts from
/// the top of the shell.
#[gpui::test]
#[serial]
fn dismiss_restores_focus_to_the_pre_open_stop(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // Land focus on a known shell stop and record its label. A single Tab from
    // a fresh window lands on the shell's own UNLABELLED focus handle, so walk
    // to a named hero button instead. The label is then read back rather than
    // hardcoded into the final assertion, so a harness change surfaces as a
    // clear failure rather than a silent tautology.
    focus_shell_neutrally(vcx);
    tab_until(vcx, "Take a tour");
    let before = A11ySnapshot::capture(vcx)
        .focused_label()
        .map(str::to_string);
    assert_eq!(
        before.as_deref(),
        Some("Take a tour"),
        "precondition: focus is parked on a labelled shell stop"
    );

    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();

    let after = A11ySnapshot::capture(vcx)
        .focused_label()
        .map(str::to_string);
    assert_eq!(after, before, "focus returned to the pre-open stop");
    drop(state);
}
