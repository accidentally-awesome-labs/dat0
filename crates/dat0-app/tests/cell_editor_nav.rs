//! Cell-editor inline-edit behavioral coverage (UAT — PD-013 / P4b–P4c T15).
//!
//! Windowed tests that drive the SHIPPED inline cell editor through the real
//! grid keystroke path. Harness helpers are copied per-binary from
//! `tests/keyboard_nav.rs` (this crate's per-binary-copy precedent).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::grid::GridDataSource;
use dat0_app::grid::selection::CellCoord;
use dat0_app::main_bridge::{MainLoop, MainThreadDispatcher};
use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;
use dat0_engine::QueryEngine as _;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Point `config_dir()` at `dir` for the rest of this (serial) test.
fn set_config_dir(dir: &Path) {
    // SAFETY: tests are `#[serial]`, so no other thread races this process-global
    // write; each test sets it before doing anything that reads `config_dir()`.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

/// A tokio runtime kept alive for the whole test so the foreground-polled
/// `cx.spawn` futures (and any direct `Handle::block_on` production code,
/// e.g. `open_demo_workspace`/`spawn_workspace_window`) can find a reactor.
/// Copied per-binary from `tests/onboarding_gpui.rs` (T1: the Enter-activation
/// test needs the same async harness the sample-click test uses).
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.rt.enter()
    }
    /// Block on a future using the harness's own runtime (Task 3: needed to
    /// resolve `engine.get_tables()` / `GridDataSource::new` the same way
    /// `tests/a11y_content.rs`'s `AsyncHarness::block_on` does).
    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }
}

/// Build the async harness and allow the executor to park (see
/// `onboarding_gpui.rs::enter_async_harness` for the full mechanism note).
fn enter_async_harness(cx: &mut TestAppContext) -> AsyncHarness {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    cx.executor().allow_parking();
    AsyncHarness { rt }
}

/// Like `build_empty_session`, but constructs the session on the harness
/// runtime so its engine shares the runtime the test has entered.
fn build_empty_session_in(h: &AsyncHarness, state_root: &Path) -> Arc<Mutex<Session>> {
    let sess =
        h.rt.block_on(Session::new(state_root, BUDGET))
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

/// True iff a dialog is currently on the window's `Root` stack (the onboarding
/// suite's canonical "tour is showing" observable).
fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, app| window.has_active_dialog(app))
}

/// Process-global `MainLoop` (receiver half of the one dispatcher we install).
/// The dispatcher lives in `window_registry`'s single-shot `OnceCell`; its
/// receiver must outlive every test in this binary. Mirrors `onboarding_gpui.rs`.
static MAIN_LOOP: OnceLock<Mutex<Option<MainLoop>>> = OnceLock::new();

/// Install the process-global `MainThreadDispatcher` exactly once and keep its
/// `MainLoop` so any (serial) test can drain it. The hero take-tour handler
/// (`open_deferred`) POSTS `onboarding::open` onto this dispatcher rather than
/// re-entering the active window; the matching drain runs those queued closures.
fn ensure_dispatcher() {
    let slot = MAIN_LOOP.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock();
    if guard.is_none() {
        let (dispatcher, main_loop) = MainThreadDispatcher::new();
        dat0_app::window_registry::install_dispatcher(dispatcher);
        *guard = Some(main_loop);
    }
}

/// Run every closure the production code posted to the dispatcher (e.g. the
/// deferred `onboarding::open`) against the current `App` — synchronously, as
/// the production consume-loop does.
fn drain_dispatcher(cx: &mut TestAppContext) {
    let Some(slot) = MAIN_LOOP.get() else {
        return;
    };
    let mut guard = slot.lock();
    if let Some(main_loop) = guard.as_mut() {
        cx.update(|app| main_loop.drain_for_test(app));
    }
}

/// CTAS a small TYPED table on the session engine (numeric `n` + bool `flag`,
/// 3 rows), bind it as a `GridDataSource`, mount it as the active grid, and pump
/// until page 0 is resident (PD-018) so the lazily-built `SelectionModel` exists.
/// CTAS (not CSV) guarantees `n`→Numeric and `flag`→Bool column types. Returns
/// the mounted data source (keep it bound for the test — `cell_render` residence
/// checks read it).
fn seed_typed_grid(
    cx: &mut VisualTestContext,
    harness: &AsyncHarness,
    session: &Arc<Mutex<Session>>,
    shell: &Entity<WorkspaceShell>,
) -> Arc<GridDataSource> {
    const SQL: &str = "SELECT * FROM (VALUES (1, true), (2, false), (3, true)) v(n, flag)";
    let engine = session.lock().engine.clone();
    harness.block_on(async {
        engine
            .create_table("cells", SQL, dat0_engine::DerivedOrigin::Sql(SQL.into()))
            .await
            .expect("create_table cells");
    });
    let ds = harness
        .block_on(async { GridDataSource::new(Arc::clone(&engine), "cells".to_string()).await })
        .expect("GridDataSource::new");
    let ds = Arc::new(ds);

    let ds_mount = Arc::clone(&ds);
    shell.update(cx, |view, cx| {
        // Deviation from the brief (see `seed_view_model_for_test`'s doc comment
        // in `window.rs`): `set_data_source` alone leaves `view_model` `None`,
        // which silently no-ops `commit_cell_edit`'s guard — a gap the brief's
        // plan didn't anticipate (its own `seed_typed_grid` transcription only
        // called `set_data_source`, mirroring `keyboard_nav.rs`'s nav-only grid
        // test, which never commits an edit so never needed a `ViewModel`).
        view.seed_view_model_for_test("cells");
        view.set_data_source(ds_mount);
        cx.notify();
    });

    // Pump until page 0 is resident so `render` builds the SelectionModel.
    let mut ready = false;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        if ds.cell_render(0, 0).is_some() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ready,
        "page 0 must load into the grid LRU before interacting"
    );
    ds
}

/// Open a fresh window over an empty session, flush + close the first-run
/// auto-show tour dialog (mandatory baseline), then seed the typed grid and
/// focus the grid shell (no click — `focus_next` leaves the active cell at the
/// origin). Returns `(shell, vcx, ds, state_dir)`; keep `ds` + `state_dir` alive.
fn mount_grid_ready<'a>(
    cx: &'a mut TestAppContext,
    harness: &AsyncHarness,
) -> (
    Entity<WorkspaceShell>,
    &'a mut VisualTestContext,
    Arc<GridDataSource>,
    tempfile::TempDir,
) {
    let state = tempfile::tempdir().unwrap();
    let session = build_empty_session_in(harness, state.path());
    let (shell, vcx) = open_shell_window(cx, Arc::clone(&session));
    vcx.run_until_parked();
    drain_dispatcher(vcx);
    vcx.run_until_parked();
    if dialog_open(vcx) {
        vcx.update(|window, app| window.close_dialog(app));
        vcx.run_until_parked();
    }
    let ds = seed_typed_grid(vcx, harness, &session, &shell);
    // Focus the grid shell WITHOUT clicking a cell (keeps active at (0,0)).
    vcx.update(|window, _app| window.focus_next());
    vcx.run_until_parked();
    (shell, vcx, ds, state)
}

/// T0 HARD GATE — proves the four drive mechanisms the slice rests on:
///   1. grid `Enter` (keystroke on the focused shell) mounts the editor.
///   2. a SECOND real `Enter` keystroke (into the now-focused inline `Input`)
///      commits the typed value AND advances the active cell by EXACTLY one
///      row, re-opening the editor on the new cell — the genuine keyboard
///      path, end to end.
///   3. the committed numeric value round-trips and reads back off the live source.
///   4. a Bool column mounts the `Select` path (not a text `Input`).
///
/// # History — this spike found two real production bugs (now fixed)
///
/// The first run of this spike drove step (2) with a second
/// `cx.simulate_keystrokes("enter")` and it FAILED: the commit was silently
/// swallowed and the active cell never advanced. Root-caused to two bugs
/// (full analysis in
/// `docs/plans/2026-07-18-dat0-uat-cell-editor-nav-fix-amendment.md`):
///
/// - **Bug A** (`window.rs`, the grid key handler): gpui-component's
///   `Input::enter()` emits `InputEvent::PressEnter` and then deliberately
///   `cx.propagate()`s (by design, so an enclosing dialog can also react). The
///   raw `KeyDownEvent` therefore bubbles to the shell's own `.on_key_down`,
///   which called `begin_cell_edit` UNCONDITIONALLY — replacing
///   `self.cell_editor` / `self.cell_editor_sub` and dropping the OLD
///   `Subscription` (the P4a T10b trap) before the just-queued `PressEnter`
///   reached it. Fixed by guarding the branch on `ws.cell_editor.is_none()`
///   — the open editor now owns Enter; the shell no longer re-mounts over it.
/// - **Bug B** (`edit_ops.rs` / `window.rs`): even with Bug A fixed, the
///   synchronous `sel.move_active(1, 0)` in `commit_cell_edit_and_advance` was
///   getting discarded by the ASYNC engine round-trip's completion callback
///   (`apply_view_change`), which unconditionally clears `self.selection` —
///   the next render rebuilt a fresh `SelectionModel` at the origin. Fixed by
///   carrying the intended cursor across the rebind in a new
///   `pending_active_cell` field, consumed once by the render-time selection
///   rebuild via the existing (clamped) `SelectionModel::move_active_to`.
///
/// With both fixes in place this spike now drives the commit with the REAL
/// second `Enter` keystroke (not a direct `CommitAndMove` emit) — the
/// strongest available proof that a genuine keyboard user's Enter-to-advance
/// works end to end. A ~100-iteration `run_until_parked` + `drain_dispatcher`
/// settle loop follows before the final read, since the advance must survive
/// the async rebind (the whole point of the Bug B fix) — reading immediately
/// after the keystroke would race that completion non-deterministically.
#[gpui::test]
#[serial]
fn t0_drive_ladder(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);

    // (1) grid Enter → editor mounts.
    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()),
        CellCoord { row: 0, col: 0 },
        "sanity: fresh SelectionModel starts at origin"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "STOP-1: grid Enter must mount the cell editor (begin_cell_edit wiring)"
    );

    // (2) set value (proves `set_text_value_for_test` — used by Tasks 2–3 for
    // Blur-commit / typed-value coverage), then drive the commit+advance with
    // the REAL second `Enter` keystroke. The editor's inner `Input` already has
    // real window focus (set during `ensure_widgets` on the render after step
    // (1)'s mount), so this keystroke routes through gpui-component's
    // `Input::enter()` exactly as a genuine keyboard user's second Enter would:
    // PressEnter → `CellEditor`'s own translation → `CommitAndMove` →
    // `begin_cell_edit`'s stored subscription → `commit_cell_edit_and_advance`.
    // With Bug A's guard in place, the raw KeyDownEvent that bubbles after
    // gpui-component's `cx.propagate()` no longer re-mounts a fresh editor over
    // the (already reassigned, by the time it bubbles) new `self.cell_editor`.
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("42", window, ecx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();
    // Pump to the SETTLED steady state (mirrors the round-trip loop in step 3
    // below) rather than reading immediately: `commit_cell_edit`'s `spawn_rebind`
    // is asynchronous, and its completion callback (`apply_view_change`)
    // unconditionally clears + rebuilds `self.selection` at the grid's origin.
    // Reading `after` right away races that async completion (observed BOTH
    // outcomes non-deterministically across repeated runs before this loop was
    // added) — settling first reports the genuine, deterministic END STATE.
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    let after = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());
    assert_eq!(
        after,
        CellCoord { row: 1, col: 0 },
        "STOP-2: commit-and-advance must move the active cell exactly one row \
         down (a value of {:?} means the subscription didn't fire / fired \
         twice / didn't advance)",
        after
    );
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "STOP-2b: the editor must re-open on the advanced cell"
    );

    // (3) round-trip: the committed 42 reads back at (0,0).
    let mut got = None;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        got = shell.update(cx, |ws, _| ws.cell_display_for_test(0, 0));
        if got.as_deref() == Some("42") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        got.as_deref(),
        Some("42"),
        "STOP-3: the committed numeric value must round-trip through the engine"
    );

    // (4) Bool column mounts the Select path. Reached from a FRESH mount (a
    // second window, not by Escape-ing the still-open numeric editor above) to
    // sidestep a known, out-of-scope stale-focus edge case: after Escape tears
    // down the editor, a stale FocusId can make subsequent keystrokes miss the
    // shell handler until focus is re-established (mirrors a note in
    // `keyboard_nav.rs`; recorded in the amendment's "Deliberately NOT fixed
    // here" section). `mount_grid_ready` reborrows the same `TestAppContext`
    // to open an independent second window on the same test dispatcher.
    let (shell2, cx2, _ds2, _state2) = mount_grid_ready(cx, &harness);
    cx2.simulate_keystrokes("right"); // active (0,0) → (0,1) = `flag` (Bool)
    cx2.run_until_parked();
    cx2.simulate_keystrokes("enter");
    cx2.run_until_parked();
    let bool_editor = shell2
        .update(cx2, |ws, _| ws.cell_editor_for_test())
        .expect("bool editor mounted");
    let ct = bool_editor.read_with(cx2, |ed, _| ed.column_type_for_test());
    assert_eq!(
        ct,
        dat0_app::view::filter_popover::ColumnType::Bool,
        "STOP-4: the bool column must mount the Select path"
    );
}

/// Baseline entry trigger: with the grid focused, `Enter` mounts the editor.
#[gpui::test]
#[serial]
fn grid_enter_mounts_editor(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    assert!(
        !shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "baseline: no editor before Enter"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "grid Enter must mount the inline cell editor"
    );
}

/// `Enter` in the editor commits and advances the active cell one row down, then
/// re-opens the editor on the new cell (spreadsheet walk-down).
///
/// Drives the commit with the REAL second `Enter` keystroke — the same path T0
/// proved out (Bug A + Bug B fixed in production) — not the `CommitAndMove`
/// emit fallback. A ~100-iteration settle loop (mirroring T0's STOP-2 loop)
/// runs before the final read, since the advance must survive the async
/// `apply_view_change` rebind; reading immediately after the keystroke would
/// race that completion non-deterministically.
#[gpui::test]
#[serial]
fn enter_commits_and_advances_down(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    let before = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());

    cx.simulate_keystrokes("enter"); // mount
    cx.run_until_parked();
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("7", window, ecx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter"); // commit + advance (real keystroke)
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();
    // Settle loop: the advance must survive the async rebind (Bug B's fix).
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }

    let after = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());
    assert_eq!(after.row, before.row + 1, "Enter must advance one row down");
    assert_eq!(
        after.col, before.col,
        "advance must stay in the same column"
    );
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "the editor must re-open on the advanced cell"
    );
}

/// `Escape` cancels the edit: the editor disappears and the cursor stays put.
#[gpui::test]
#[serial]
fn escape_cancels_and_keeps_cursor(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    let before = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());

    cx.simulate_keystrokes("enter"); // mount
    cx.run_until_parked();
    assert!(shell.update(cx, |ws, _| ws.cell_editor_open_for_test()));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        !shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "Escape must dismiss the editor"
    );
    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()),
        before,
        "Escape must leave the cursor on the cell being edited"
    );
}

/// Invalid input is rejected: typing non-numeric into a numeric cell + `Enter`
/// suppresses the commit — no advance, and the editor STAYS open so the user can
/// fix it.
///
/// The MOUNT keystroke (opening the editor) uses the same real
/// `cx.simulate_keystrokes("enter")` as every other test here. The
/// commit-ATTEMPT is driven with `cx.dispatch_action(input::Enter { .. })`
/// instead of a second real keystroke — see the note below for why, and why
/// this is still a faithful drive of the real production wiring, not a
/// weakened one.
///
/// # Why not a second real keystroke (investigated, not assumed)
///
/// A second `cx.simulate_keystrokes("enter")` here PANICS — reproducibly, and
/// deterministically inside the `simulate_keystrokes` call itself (confirmed
/// via `RUST_BACKTRACE=full`), with gpui's `text_system.rs` debug_assert
/// "text argument should not contain newlines". Root cause, traced through
/// gpui 0.2.2 + gpui-component 0f0ab35 source (not guessed):
///
/// `gpui::Window::dispatch_keystroke` (the function `TestAppContext`'s
/// `simulate_keystrokes` uses) unconditionally calls
/// `keystroke.with_simulated_ime()`, which fills `key_char = Some("\n")` for
/// the `"enter"` key. If the KeyDown event's propagation was never stopped —
/// true here, by design: `InputState::enter()` (`input/state.rs`) calls
/// `cx.propagate()` for single-line inputs so `WorkspaceShell`'s own
/// `.on_key_down` can also react (`window.rs`'s Bug-A comment), and the shell
/// never stops it either when `cell_editor.is_some()` — `dispatch_keystroke`
/// THEN replays that `key_char` as literal typed text via
/// `input_handler.dispatch_input("\n", ..)`, straight into whatever
/// `InputState` currently owns the focus (still the SAME, still-open
/// `CellEditor`'s numeric `Input`, since the rejected parse never replaces
/// it — unlike a successful commit, which swaps in a fresh, unpolluted
/// editor before the polluted one is ever rendered again). The next repaint
/// then tries to shape that single-line buffer (now `"abc\n"`) as one line
/// and hits the debug_assert.
///
/// This replay step is specific to `TestAppContext::dispatch_keystroke`
/// (its only caller in the whole `gpui` crate besides its own unit tests) —
/// it exists so headless tests can emulate a full hardware+IME round trip
/// for ordinary typed characters. Checked against the real macOS backend
/// (`platform/mac/window.rs`): a real physical Enter key on Cocoa is routed
/// through `NSTextInputContext.handleEvent:` → `doCommandBySelector:`
/// (`insertNewline:`), which gpui's own glue code no-ops for exactly this
/// case (`keystroke_for_do_command` is only armed when `key_char.is_none()`,
/// which isn't true for Enter) — so a real macOS user does not hit this.
///
/// `cx.dispatch_action` drives the *same* real production chain a keystroke
/// would (`Window::dispatch_action` → `dispatch_action_on_node` →
/// `InputState::enter()` → real `PressEnter` emit → `CellEditor`'s real
/// subscriber → real `parse_text` rejection) — it just skips gpui's
/// keystroke-specific raw-KeyDown normalization (and the IME-replay
/// side-effect that rides along with it), which carries no assertion this
/// test cares about. Both assertions below are unchanged and unweakened.
///
/// No settle loop is needed: `parse_text` rejects synchronously, so the
/// commit is suppressed before any async rebind is ever scheduled — a single
/// `run_until_parked` to process the action suffices.
#[gpui::test]
#[serial]
fn invalid_numeric_is_rejected_editor_stays(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    let before = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());

    cx.simulate_keystrokes("enter"); // mount over numeric col 0 (real keystroke)
    cx.run_until_parked();
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| {
            ed.set_text_value_for_test("abc", window, ecx)
        });
    });
    cx.run_until_parked();
    // Commit attempt — must be suppressed. See the doc comment above for why
    // this is `dispatch_action` rather than a second `simulate_keystrokes`.
    cx.dispatch_action(gpui_component::input::Enter { secondary: false });
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();

    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()),
        before,
        "invalid input must NOT advance the active cell"
    );
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "invalid input must leave the editor open to correct"
    );
}

/// Positive control for the `dispatch_action(Enter)` drive that
/// `invalid_numeric_is_rejected_editor_stays` relies on for its commit attempt.
///
/// That test's two assertions (no advance + editor stays open) would ALSO pass
/// if `cx.dispatch_action(gpui_component::input::Enter { .. })` were a complete
/// no-op — nothing would have proven the action actually routes anywhere. This
/// test drives the SAME mechanism with a VALID value ("99") and asserts the
/// active cell genuinely ADVANCES: that assertion FAILS if the dispatch were a
/// no-op (the cursor would stay at `before`), so a pass here is proof the
/// action reaches the real `InputState::enter()` → `PressEnter` →
/// `CommitAndMove` → `commit_cell_edit_and_advance` chain — the same chain the
/// invalid-reject test's commit ATTEMPT rides, just with a parse that
/// succeeds instead of one that's rejected. That, in turn, is what makes the
/// invalid test's "no advance" assertion meaningful rather than vacuous.
///
/// Since 99 is valid, the commit SUCCEEDS and the editor tears down + re-mounts
/// fresh on the advanced cell — no risk of the stray-IME-"\n" panic that only
/// bites a REJECTED, still-open editor (see the doc comment on
/// `invalid_numeric_is_rejected_editor_stays` for why that one can't use a
/// second real keystroke).
#[gpui::test]
#[serial]
fn dispatch_action_enter_commits_valid_and_advances(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    let before = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());

    cx.simulate_keystrokes("enter"); // mount over (0,0) (real keystroke, as every test uses)
    cx.run_until_parked();
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("99", window, ecx));
    });
    cx.run_until_parked();
    // Commit attempt — SAME mechanism the invalid-reject test uses for its
    // commit attempt, but with a value that parses cleanly.
    cx.dispatch_action(gpui_component::input::Enter { secondary: false });
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();

    let target = CellCoord {
        row: before.row + 1,
        col: before.col,
    };
    let mut after = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());
    for _ in 0..100 {
        if after == target {
            break;
        }
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
        after = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());
    }

    assert_eq!(
        after.row,
        before.row + 1,
        "positive control: dispatch_action(Enter) with a VALID value must \
         advance the active cell one row down — a value of {:?} means the \
         dispatch_action drive did NOT reach the real commit-and-advance path \
         (i.e. it would have been a no-op), which would make the sibling \
         invalid-reject test's \"no advance\" assertion vacuous",
        after
    );
    assert_eq!(
        after.col, before.col,
        "advance must stay in the same column"
    );

    // Value round-trip: separate settle loop (the display read can lag the
    // active-cell advance briefly, same as `numeric_commit_round_trips_to_data_source`).
    let mut got = shell.update(cx, |ws, _| ws.cell_display_for_test(0, 0));
    for _ in 0..100 {
        if got.as_deref() == Some("99") {
            break;
        }
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
        got = shell.update(cx, |ws, _| ws.cell_display_for_test(0, 0));
    }
    assert_eq!(
        got.as_deref(),
        Some("99"),
        "the committed value must round-trip through the engine"
    );
}

/// The one deep proof: a typed numeric value driven through the real engine
/// round-trip reads back off the live (rebound overlay) data source. Proves
/// the UI→CellEdit→engine bridge once.
#[gpui::test]
#[serial]
fn numeric_commit_round_trips_to_data_source(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);

    // Sanity: (0,0) starts at the seeded 1.
    assert_eq!(
        shell
            .update(cx, |ws, _| ws.cell_display_for_test(0, 0))
            .as_deref(),
        Some("1"),
        "seed sanity: (0,0) is 1 before the edit"
    );

    cx.simulate_keystrokes("enter"); // mount over (0,0)
    cx.run_until_parked();
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("42", window, ecx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter"); // commit + advance
    cx.run_until_parked();

    let mut got = None;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        got = shell.update(cx, |ws, _| ws.cell_display_for_test(0, 0));
        if got.as_deref() == Some("42") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        got.as_deref(),
        Some("42"),
        "the edited numeric value must persist through the engine round-trip \
         and read back off the live overlay source"
    );
}

/// A Bool column mounts the `Select` widget path (not a text `Input`). This is
/// the meaningful, robust proof of the second widget path. Driving the actual
/// `SelectEvent::Confirm` headlessly is a known gpui-component limitation
/// (`set_selected_value` does not emit `Confirm`) — the commit itself stays
/// covered by the `parse_bool_text_path` unit in `cell_editor.rs`, so the mount
/// proof is where the new coverage lives.
#[gpui::test]
#[serial]
fn bool_column_mounts_select_path(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);

    // Move the active cell to the bool column (`flag`, screen col 1), then open.
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()).col,
        1,
        "Right must move the active cell onto the bool column"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("bool editor mounted");
    let ct = editor.read_with(cx, |ed, _| ed.column_type_for_test());
    assert_eq!(
        ct,
        dat0_app::view::filter_popover::ColumnType::Bool,
        "a bool column must mount the Select (Bool) editor path, not a text Input"
    );
}
