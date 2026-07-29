//! Export-modal and saved-query-picker keyboard coverage (UI redesign B2).
//!
//! Drives Tab / Shift-Tab / arrows / Escape through the REAL keymap with
//! `simulate_keystrokes`. Never `dispatch_action` — that bypasses the keymap,
//! and the keymap is the mechanism under test (B1 design §1: gpui dispatches
//! action bindings BEFORE `on_key_down`, so the trap is built from actions
//! bound under a deeper key context).
//!
//! Task 0's `gate_*` tests characterize the two mechanisms B2 cannot work
//! without, neither previously exercised in this codebase:
//!
//! - **Gate A** — focus set from inside `render` sticks. This is what lets the
//!   export dialog take focus at all: its only production open path
//!   (`view_actions::dispatch_export`) reaches the shell from a bare `&mut App`
//!   with no `Window`, and B1 measured that with NOTHING focused the dispatch
//!   path is the window root alone, so Tab is completely inert.
//! - **Gate B** — one dat0 `focus_stop` wrapping a `RadioGroup` whose children
//!   carry `.tab_stop(false)` really is a SINGLE tab stop.
//!
//! Harness helpers are copied per-binary from `tests/modal_trap_nav.rs` (the
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
use dat0_app::view::export_dialog::{ExportDialog, ExportEvent, ExportScope};
use dat0_app::window::WorkspaceShell;
use dat0_engine::types::ExportFormat;

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
/// button (copied from `keyboard_nav.rs` / `input_nav.rs` / `modal_trap_nav.rs`).
///
/// LOAD-BEARING for any Tab-driven test: with NOTHING focused, the dispatch
/// path is the window root alone, so not even `Root`'s own "tab" binding
/// matches and Tab is completely inert.
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

// ---------------------------------------------------------------------------
// Task 0 gates
// ---------------------------------------------------------------------------

/// GATE A — a modal opened with NO `&mut Window` (the real `open_export_dialog`
/// path) must still end up focused. The mechanism under test is the
/// render-drain: the open path sets a flag and `WorkspaceShell::render` — which
/// does hold a `Window` — focuses the modal's first stop.
#[gpui::test]
#[serial]
fn gate_a_render_drain_focuses_the_export_dialog(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    // Deliberately windowless — mirrors `view_actions::dispatch_export`.
    vcx.update(|_w, app| shell.update(app, |ws, cx| ws.open_export_dialog_for_test(cx)));
    vcx.run_until_parked();

    let first_stop = vcx
        .update(|_w, app| {
            shell
                .read(app)
                .export_dialog_entity_for_test()
                .map(|d| d.read(app).format_focus_handle())
        })
        .expect("dialog mounted");
    assert!(
        vcx.update(|window, _app| first_stop.is_focused(window)),
        "the render-drain must move focus into the modal; without it nothing is \
         focused and Tab is completely inert"
    );
    drop(state);
}

/// GATE B — one dat0 `focus_stop` wrapping a `RadioGroup` whose children are
/// built with `.tab_stop(false)` is a SINGLE tab stop: one Tab leaves the group
/// entirely and lands on the NEXT dat0 stop, rather than stepping between the
/// radios painted inside it. `RadioGroup::render` rewrites each child's id but
/// leaves `tab_stop` alone (gpui-component `radio.rs:333`), which is what makes
/// this possible.
#[gpui::test]
#[serial]
fn gate_b_radio_group_is_one_tab_stop(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    vcx.update(|_w, app| shell.update(app, |ws, cx| ws.open_export_dialog_for_test(cx)));
    vcx.run_until_parked();

    let (format_fh, scope_fh) = vcx
        .update(|_w, app| {
            shell.read(app).export_dialog_entity_for_test().map(|d| {
                let d = d.read(app);
                (d.format_focus_handle(), d.scope_focus_handle())
            })
        })
        .expect("dialog mounted");

    assert!(
        vcx.update(|window, _app| format_fh.is_focused(window)),
        "gate A precondition: the drain focuses the format group"
    );

    press_tab(vcx);
    vcx.run_until_parked();

    assert!(
        vcx.update(|window, _app| scope_fh.is_focused(window)),
        "one Tab must jump the WHOLE radio group and land on the scope group; \
         landing anywhere else means the radios are still tab stops of their own \
         (they are painted inside the group, so they would come first)"
    );
    drop(state);
}

// ---------------------------------------------------------------------------
// Task 2 — the export dialog's own keyboard behaviour
// ---------------------------------------------------------------------------

/// Open the export dialog the windowless way and subscribe to its events.
/// Returns (dialog, log). The `Subscription` is deliberately leaked for the
/// test's life (the process is short-lived) — dropping it deregisters silently.
fn open_export_with_log(
    shell: &Entity<WorkspaceShell>,
    vcx: &mut VisualTestContext,
) -> (Entity<ExportDialog>, Rc<RefCell<Vec<ExportEvent>>>) {
    vcx.update(|_w, app| shell.update(app, |ws, cx| ws.open_export_dialog_for_test(cx)));
    vcx.run_until_parked();
    let dialog = vcx
        .update(|_w, app| shell.read(app).export_dialog_entity_for_test())
        .expect("dialog mounted");
    let log: Rc<RefCell<Vec<ExportEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let log2 = log.clone();
    let sub = vcx.cx.update(|app| {
        app.subscribe(&dialog, move |_d, ev: &ExportEvent, _app| {
            log2.borrow_mut().push(ev.clone())
        })
    });
    std::mem::forget(sub);
    vcx.run_until_parked(); // flush the deferred subscription activation
    (dialog, log)
}

/// Left/Right cycle the format radio group while it holds focus — the WAI-ARIA
/// radiogroup pattern: the group is one tab stop and arrows move the selection.
/// Selection WRAPS here, deliberately unlike the list surfaces, whose arrows
/// clamp (`empty_state.rs:436-439`).
#[gpui::test]
#[serial]
fn arrows_change_the_export_format(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    let (dialog, _log) = open_export_with_log(&shell, vcx);

    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).format_for_test()),
        ExportFormat::Csv,
        "defaults to CSV"
    );
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).format_for_test()),
        ExportFormat::Json,
        "Right moves to the next format"
    );
    vcx.simulate_keystrokes("left left");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).format_for_test()),
        ExportFormat::Parquet,
        "Left wraps past the first entry to the last"
    );
    drop(state);
}

/// Up/Down cycle the scope group. Same pattern, vertical layout.
#[gpui::test]
#[serial]
fn arrows_change_the_export_scope(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    let (dialog, _log) = open_export_with_log(&shell, vcx);
    vcx.update(|window, app| window.focus(&dialog.read(app).scope_focus_handle()));
    vcx.run_until_parked();

    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).scope_for_test()),
        ExportScope::CurrentView
    );
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).scope_for_test()),
        ExportScope::FullTable,
        "Down moves to the next scope"
    );
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).scope_for_test()),
        ExportScope::CurrentView,
        "Down from the last entry wraps to the first"
    );
    drop(state);
}

/// Enter on the Export stop emits the ARROW-SELECTED scope and format, not the
/// defaults — proves the keyboard path reaches the same state the mouse does.
#[gpui::test]
#[serial]
fn enter_on_export_emits_the_selected_scope_and_format(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    let (dialog, log) = open_export_with_log(&shell, vcx);

    vcx.simulate_keystrokes("right"); // format → Json (drain left focus here)
    vcx.run_until_parked();
    vcx.update(|window, app| window.focus(&dialog.read(app).scope_focus_handle()));
    vcx.simulate_keystrokes("down"); // scope → FullTable
    vcx.run_until_parked();
    vcx.update(|window, app| window.focus(&dialog.read(app).run_focus_handle()));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();

    assert!(
        log.borrow().iter().any(|e| matches!(
            e,
            ExportEvent::Export {
                scope: ExportScope::FullTable,
                format: ExportFormat::Json
            }
        )),
        "Enter on Export must carry the arrow-selected values, got {:?}",
        log.borrow()
    );
    drop(state);
}

// ---------------------------------------------------------------------------
// Task 3 — the export dialog inside the trap
// ---------------------------------------------------------------------------

/// The export modal traps Tab: four stops, wrapping, never escaping into the
/// obscured shell. This is the WCAG 2.4.3 fix for the export dialog — B1 closed
/// it for the three `NamePrompt` modals only.
#[gpui::test]
#[serial]
fn export_modal_tab_cycles_four_stops(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    let (dialog, _log) = open_export_with_log(&shell, vcx);
    let (fmt, scope, run, cancel) = vcx.update(|_w, app| {
        let d = dialog.read(app);
        (
            d.format_focus_handle(),
            d.scope_focus_handle(),
            d.run_focus_handle(),
            d.cancel_focus_handle(),
        )
    });

    assert!(
        vcx.update(|window, _app| fmt.is_focused(window)),
        "the drain opens the modal on its first stop"
    );
    for (i, want) in [&scope, &run, &cancel, &fmt].iter().enumerate() {
        press_tab(vcx);
        vcx.run_until_parked();
        assert!(
            vcx.update(|window, _app| want.is_focused(window)),
            "Tab hop {} must stay inside the modal and wrap at the end",
            i + 1
        );
    }
    press_shift_tab(vcx);
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, _app| cancel.is_focused(window)),
        "Shift-Tab from the first stop wraps to the last"
    );
    drop(state);
}

/// Escape from a non-field stop emits exactly ONE Cancel. Two bindings match
/// while a modal is up; `on_action` handlers consume by default, but this is
/// the cell-editor double-fire class and is asserted, not assumed.
#[gpui::test]
#[serial]
fn escape_from_export_emits_exactly_one_cancel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    let (dialog, log) = open_export_with_log(&shell, vcx);
    vcx.update(|window, app| window.focus(&dialog.read(app).cancel_focus_handle()));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();

    assert_eq!(
        log.borrow()
            .iter()
            .filter(|e| matches!(e, ExportEvent::Cancel))
            .count(),
        1,
        "exactly one Cancel per Escape, got {:?}",
        log.borrow()
    );
    assert!(
        vcx.update(|_w, app| shell.read(app).export_dialog_entity_for_test().is_none()),
        "Escape dismisses the modal"
    );
    drop(state);
}

/// The modal paints a named `Dialog` a11y node, so `modal_host` is genuinely in
/// the tree rather than the dialog being mounted bare.
#[gpui::test]
#[serial]
fn export_modal_emits_a_named_dialog_node(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    let (_dialog, _log) = open_export_with_log(&shell, vcx);
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.query_by_role(dat0_app::a11y::AccessRole::Dialog, "Export"),
        "modal_host must paint a Dialog node named by the modal's title"
    );
    drop(state);
}
