//! P11a T14 — behavioral UAT for first-run onboarding, driven headlessly.
//!
//! These are real windowed `#[gpui::test]`s: each opens a `TestPlatform` window
//! whose root view is a `gpui_component::Root` wrapping a real
//! [`WorkspaceShell`] over an EMPTY session (so the shell renders its
//! empty-state hero), then drives the production onboarding code paths and
//! asserts observable state changes (an actual dialog on the window;
//! `settings.toml` contents). The spike `.superpowers/sdd/spike-gpui-uat-report.md`
//! proved the mechanism (`add_window_view` → `VisualTestContext` →
//! `run_until_parked` → `simulate_click`/`dispatch_action`).
//!
//! ## Hermeticity
//! Every test points `DAT0_CONFIG_DIR` (the Part-A seam) at a fresh temp dir, so
//! NOTHING here ever reads or writes the real `~/Library/Application Support/dat0`.
//! `std::env::set_var` is `unsafe` + process-global and `#[gpui::test]` runs
//! multithreaded, so every test is `#[serial]`.
//!
//! ## What is and isn't asserted (honesty notes)
//! - `WorkspaceShell::tour_auto_shown` is a PRIVATE field of the `window`
//!   module → unreadable from this external test crate. The auto-show guard is
//!   therefore asserted via its *observable* contract: a dialog appears exactly
//!   once and a forced re-render does not stack a second one. That is a stronger
//!   (behavioral) check than reading the bool.
//! - Both the auto-show AND the manual tour re-entry points (Help → Take a
//!   Tour, the hero "Take a tour" button) hop the process-global
//!   `window_registry::dispatcher()` (`OnceCell`, settable once per process) to
//!   escape the window-update re-entrancy that would otherwise no-op the open.
//!   Because that dispatcher is single-shot AND its receiver must outlive every
//!   test, the dispatcher-driven tests share ONE install via [`ensure_dispatcher`]
//!   and drain its queue synchronously with [`drain_dispatcher`] (the
//!   app-agnostic `MainLoop::drain_for_test`, run from a plain `App` context —
//!   exactly what the production consume-loop does, minus the async loop). Each
//!   such test drains once up front (clearing any closure a prior serial test
//!   left queued, which no-ops while no window is active) so it starts clean.

mod support;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use parking_lot::Mutex;
use serial_test::serial;

use support::A11ySnapshot;

use dat0_app::main_bridge::{MainLoop, MainThreadDispatcher};
use dat0_app::session::Session;
use dat0_app::settings::set_first_run_done;
use dat0_app::settings::store::SettingsStore;
use dat0_app::window::WorkspaceShell;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Point `config_dir()` at `dir` for the rest of this (serial) test.
fn set_config_dir(dir: &Path) {
    // SAFETY: tests are `#[serial]`, so no other thread races this process-global
    // write; each test sets it before doing anything that reads `config_dir()`.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

/// Build a real, EMPTY in-memory session (no tabs/recents → the shell renders
/// its empty-state hero, which is what the onboarding paths key off).
///
/// `Session::new` is async and the engine uses `tokio::task::spawn_blocking`
/// internally, so it must run inside a tokio runtime — which the gpui test
/// executor is not. We block on it with a dedicated multi-thread runtime BEFORE
/// the window is opened. `Session::new` awaits all of its own spawn_blocking work
/// (migrations etc.) before returning, and the engine holds only a synchronous
/// `duckdb::Connection` (no Drop needs a runtime), so the runtime can be dropped
/// once construction completes. This resolves the brief's risky-unknown (a):
/// blocking on async `Session::new` under the gpui test executor.
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

/// A tokio runtime kept alive for the whole test so the foreground-polled
/// `cx.spawn` futures can call `tokio::task::spawn_blocking` (the engine and
/// file-import paths are built on it). Mirrors production `run_app`
/// (window.rs:1308/1377), which holds `runtime.enter()` for all of
/// `Application::run`. The caller MUST bind `enter()`'s guard to a `_guard`
/// held to end-of-test, and the harness MUST outlive every spawned task.
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

/// Like `build_empty_session`, but constructs the session on the harness
/// runtime so its engine shares the runtime the test has entered.
fn build_empty_session_in(h: &AsyncHarness, state_root: &Path) -> Arc<Mutex<Session>> {
    let sess = h
        .block_on(Session::new(state_root, BUDGET))
        .expect("Session::new");
    Arc::new(Mutex::new(sess))
}

/// Open a real window whose root view is a `gpui_component::Root` wrapping a
/// fresh `WorkspaceShell` over `session` — exactly mirroring production
/// (`window.rs::open_window_view`). The window is ACTIVATED inside the build
/// closure so `cx.active_window()` (which `onboarding::open` relies on) resolves
/// to it before the first render frame runs. Returns the live shell entity plus
/// the windowed test context.
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

/// True iff a dialog is currently on the window's `Root` stack.
fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, app| window.has_active_dialog(app))
}

/// Initialise the gpui-component theme global (required before any view that
/// renders gpui-component widgets — the carousel uses `Button`/`Dialog`).
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
}

/// Process-global `MainLoop` (the receiver half of the one dispatcher we
/// install). The dispatcher itself lives in `window_registry`'s single-shot
/// `OnceCell`; its receiver must outlive every test in this binary, so it is
/// stashed here on first use and shared by all `#[serial]` tests.
static MAIN_LOOP: OnceLock<Mutex<Option<MainLoop>>> = OnceLock::new();

/// Install the process-global `MainThreadDispatcher` exactly once and keep its
/// `MainLoop` so any (serial) test can drain it. The auto-show render and the
/// manual tour re-entry handlers (`open_deferred`) both POST `onboarding::open`
/// onto this dispatcher rather than re-entering the active window; the matching
/// drain runs those queued closures. Idempotent: the second-and-later calls
/// reuse the already-installed dispatcher (the `OnceCell` set is a no-op).
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
/// deferred `onboarding::open`) against the current `App` — exactly what the
/// production consume-loop does, but synchronously.
///
/// Uses the App-only `TestAppContext::update`, NOT `VisualTestContext::update`:
/// the latter takes the window out of its slot for the closure, which would
/// re-create the very re-entrancy no-op the dispatcher hop exists to avoid.
/// `&mut VisualTestContext` deref-coerces to `&mut TestAppContext`, so callers
/// pass either — before a window exists (clearing stale closures, which then
/// no-op) or after (actually opening the dialog on the active window).
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
// 1. ★ auto-show-once — the single most important UAT item.
// ----------------------------------------------------------------------------

/// First run (no `settings.toml` → `first_run_done` defaults false): the
/// empty-state render schedules the tour via the dispatcher, it opens exactly
/// once, and a forced re-render does NOT stack a second dialog (the dual-guard).
#[gpui::test]
#[serial]
fn auto_show_opens_tour_exactly_once(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Install the shared process-global dispatcher (the exact one window.rs:6058
    // posts the auto-show onto), then clear any closure a prior serial test left
    // queued — draining now, before a window exists, makes those stale
    // `onboarding::open`s no-op (no active window) so we start clean.
    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx);

    let session = build_empty_session(state.path());
    let (shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // The first-run empty-state render POSTED the tour open onto the dispatcher
    // (window.rs:6058). Draining runs it from a plain `App` context — the hop
    // that makes the open actually land (a direct in-render open would re-enter
    // the taken window and no-op).
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "first-run empty-state must auto-open the tour dialog"
    );

    // Force another render frame. The synchronous `tour_auto_shown` guard must
    // prevent a SECOND dispatch — so the drain below finds nothing queued and
    // there is still exactly one dialog. We prove "exactly one" by popping once:
    // a second stacked dialog would survive a single close.
    shell.update(cx, |_shell, c| c.notify());
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "tour dialog must still be present after a re-render"
    );
    cx.update(|window, app| window.close_dialog(app));
    cx.run_until_parked();
    assert!(
        !dialog_open(cx),
        "re-render must NOT have stacked a second tour dialog (dual-guard)"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// 2. auto-show suppressed once the flag is set.
// ----------------------------------------------------------------------------

/// With `first_run_done=true` seeded in `settings.toml`, the empty-state render
/// must NOT auto-open the tour. (Teeth: `auto_show_opens_tour_exactly_once`
/// proves the same harness DOES open a dialog when the flag is unset, so this
/// negative is meaningful rather than vacuous.)
#[gpui::test]
#[serial]
fn auto_show_suppressed_when_first_run_done(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Seed the persisted flag.
    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    set_first_run_done(&store, true).unwrap();
    assert!(
        store.load_or_default().unwrap().first_run_done,
        "precondition: flag seeded true"
    );

    // Install the shared dispatcher and drain any stale closures a prior serial
    // test left queued (draining before a window exists no-ops them harmlessly).
    // This makes the test SYMMETRIC with `auto_show_opens_tour_exactly_once`:
    // if suppression is broken and window.rs still posts an open onto the
    // dispatcher, that closure is drained and executed below — opening a real
    // dialog that fails the assert. Without this, a broken-suppression-but-
    // dispatcher-routed regression would queue an open that is never executed,
    // giving a false green.
    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx);

    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // Drain the dispatcher: if suppression is broken, a queued open closure
    // executes now and opens a dialog — the assert below will catch it.
    // With suppression working, the queue is empty and no dialog appears.
    drain_dispatcher(cx);
    cx.run_until_parked();

    assert!(
        !dialog_open(cx),
        "tour must NOT auto-open once first_run_done=true"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// 3. carousel opens via the public entry point (panel 0).
// ----------------------------------------------------------------------------

/// `onboarding::open` (the Help-menu / hero re-entry point) opens the carousel
/// dialog on the active window. This drives the real `present_panel` →
/// `window.open_dialog` path end to end.
#[gpui::test]
#[serial]
fn onboarding_open_shows_carousel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Seed first_run_done=true so the empty-state render neither shows the
    // enriched band nor posts an auto-show — this test exercises ONLY the
    // explicit `onboarding::open` entry point, independent of whether a prior
    // serial test already installed the shared dispatcher.
    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    set_first_run_done(&store, true).unwrap();

    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // No auto-show (flag seeded true); the dialog below is solely from our
    // explicit call.
    assert!(!dialog_open(cx), "no dialog before the explicit open");

    // Reach `onboarding::open` from a plain App context (NOT nested in a window
    // update — it re-enters the active window itself).
    cx.cx.update(dat0_app::onboarding::open);
    cx.run_until_parked();

    assert!(
        dialog_open(cx),
        "onboarding::open must present the tour carousel"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// 4. TakeTour action OPENS the tour (the fix).
// ----------------------------------------------------------------------------

/// Help → "Take a Tour" (and the `menu_macos::TakeTour` action it dispatches)
/// must actually OPEN the tour.
///
/// gpui's `dispatch_action` runs the handler INSIDE a `window.update` of the
/// active window (App::dispatch_action → active_window.update → ... → the global
/// `&TakeTour` listener). The OLD wiring called `onboarding::open` directly
/// there, which re-entered that already-taken window via
/// `cx.active_window().update(..)` and silently no-op'd — so "Take a Tour" did
/// nothing. The fix routes the handler through `onboarding::open_deferred`,
/// which hops the `MainThreadDispatcher` (running the open from a plain `App`
/// context after the frame, exactly like the auto-show). This test reproduces
/// the production one-line wiring verbatim (`run_app`'s registration is not
/// separately callable) and asserts the dialog opens once the hop is drained.
///
/// Teeth: revert the handler to a direct `onboarding::open` and this fails —
/// the in-update open no-ops AND nothing is posted to drain.
#[gpui::test]
#[serial]
fn take_tour_action_opens_tour_via_dispatcher(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Seed first_run_done=true so the empty-state render does NOT also auto-show
    // — the only dialog this test can open is the one from the TakeTour action.
    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    set_first_run_done(&store, true).unwrap();

    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    // Production wiring, verbatim (window.rs:1614).
    cx.update(|app| {
        app.on_action(|_: &dat0_app::menu_macos::TakeTour, app: &mut gpui::App| {
            dat0_app::onboarding::open_deferred(app);
        });
    });

    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();
    assert!(
        !dialog_open(cx),
        "precondition: no auto-show (first_run_done=true) → clean baseline"
    );

    // Dispatch the real action. The handler runs inside the active window's
    // update; `open_deferred` therefore POSTS onto the dispatcher instead of
    // re-entering. Draining runs it from a plain App context → the tour opens.
    cx.dispatch_action(dat0_app::menu_macos::TakeTour);
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();

    assert!(
        dialog_open(cx),
        "TakeTour must open the tour dialog (open_deferred hops the dispatcher \
         out of the window-update re-entry that used to silently no-op)"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// 4b. Hero "Take a tour" button OPENS the tour (the fix, via a real click).
// ----------------------------------------------------------------------------

/// The first-run hero band's "Take a tour" button (`id`/`debug_selector`
/// `hero-take-tour`) must open the tour. Its click handler fires inside a
/// `window.update`, so (like the menu action) a synchronous `onboarding::open`
/// would re-enter the taken window and no-op; the fix routes it through
/// `open_deferred`. We click the REAL button — located by its painted bounds via
/// the release-no-op `debug_selector`, not a fragile hard-coded pixel — then
/// drain the dispatcher hop and assert the dialog opens.
///
/// The enriched band only renders when `first_run_done=false`, which ALSO fires
/// the auto-show. We flush + close that auto-show dialog first so the
/// post-click dialog is cleanly attributable to the button click alone.
///
/// Teeth: revert the hero handler to a direct `onboarding::open` and this fails
/// — the in-update open no-ops and nothing is posted to drain after the click.
#[gpui::test]
#[serial]
fn hero_take_tour_button_opens_tour(cx: &mut TestAppContext) {
    use gpui::Modifiers;

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    // first_run_done unset (false) → Enriched band (hero-take-tour painted).

    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // Flush + dismiss the first-run auto-show so the baseline is dialog-free.
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "sanity: first-run auto-show opened a dialog"
    );
    cx.update(|window, app| window.close_dialog(app));
    cx.run_until_parked();
    assert!(!dialog_open(cx), "baseline: auto-show dialog closed");

    // Locate + click the REAL hero button by its painted bounds. The band still
    // renders (first_run_done is still false) and the auto-show will NOT re-fire
    // (the per-shell `tour_auto_shown` guard is now set).
    let bounds = cx
        .debug_bounds("hero-take-tour")
        .expect("hero-take-tour button must be painted in the enriched band");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();

    // The click handler posted `open_deferred` onto the dispatcher; drain it.
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "clicking the hero 'Take a tour' button must open the tour dialog"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// 5. Carousel "Skip" click → real overlay-button click dismisses + writes flag.
// ----------------------------------------------------------------------------

/// Resolves the brief's risky-unknown (b): `simulate_click` DOES hit-test
/// buttons rendered inside a `window.open_dialog` overlay. A real click on the
/// carousel's "Skip" button dismisses the dialog AND persists
/// `first_run_done=true` (via the production `mark_first_run_done` →
/// `settings.toml` write).
///
/// UAT Gap 2: the click is now located BY LABEL, not by a hand-tuned pixel. The
/// Skip `Button` carries a test-only `.a11y("tour-skip", …)` annotation, so
/// `A11ySnapshot::capture` finds it by its rendered label, recovers its static id,
/// and resolves painted bounds via `debug_bounds` — proving the AccessKit node and
/// the gpui hitbox stay in lockstep and retiring the fragile (777,550) constant.
/// The slide-down animation is settled with `advance_clock` first so the hit-box
/// is at its resting position when captured.
#[gpui::test]
#[serial]
fn skip_click_dismisses_and_writes_flag(cx: &mut TestAppContext) {
    use std::time::Duration;

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    // Precondition: flag unset (defaults false), no dialog yet.
    assert!(
        !store
            .load_or_default()
            .map(|s| s.first_run_done)
            .unwrap_or(true),
        "precondition: first_run_done must start false"
    );

    cx.cx.update(dat0_app::onboarding::open);
    cx.run_until_parked();
    assert!(dialog_open(cx), "carousel must be open at panel 0");

    // Settle the dialog slide-down animation so the button is at its resting
    // hit-box, then click the "Skip" button BY LABEL. `A11ySnapshot::capture`
    // reads the AccessKit tree the render emitted; `click` recovers Skip's static
    // id (`tour-skip`) from the label, resolves its painted bounds via
    // `debug_bounds`, and fires a real `simulate_click` at the centre — no
    // hand-tuned (777,550) pixel constant, and it self-corrects if the layout
    // moves.
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let snap = A11ySnapshot::capture(cx);
    snap.click(cx, &dat0_app::dat0_i18n::t("onboarding.tour.skip"));
    cx.run_until_parked();

    // The overlay click really fired the Skip handler: dialog dismissed AND the
    // persisted flag flipped to true.
    assert!(
        !dialog_open(cx),
        "Skip must dismiss the tour dialog (overlay click reached the button)"
    );
    assert!(
        store
            .load_or_default()
            .map(|s| s.first_run_done)
            .unwrap_or(false),
        "Skip must persist first_run_done=true (real settings.toml write)"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// 6. Hero sample button → real click drives the full async import to completion.
// ----------------------------------------------------------------------------

/// The first hero sample card ("Iris", id `hero-sample-0`) drives the production
/// `open_sample_kind` → `cx.spawn(handle_drop → route_drop_outcomes)` flow to
/// COMPLETION: the bundled CSV is extracted AND imported, leaving the `iris`
/// table registered in the engine. (Pre-Gap-3 this test could only assert the
/// synchronous CSV extraction because the async import's `spawn_blocking`
/// panicked under the gpui test executor; the async harness now drives it home.)
///
/// Drive strategy: `open_sample_kind` uses `cx.spawn(...).detach()`, so the
/// Task cannot be captured for `block_test`. Instead we pump the gpui foreground
/// queue with repeated `run_until_parked` calls interleaved with real
/// `thread::sleep` pauses. Each pump drives the detached task to its next
/// `spawn_blocking` park (sniff on iteration 1, CTAS on iteration 2); the
/// harness runtime's blocking pool runs the DuckDB work during the sleep; the
/// waker re-enqueues the continuation for the next pump. We check the engine
/// for the `iris` table after each cycle and break early once the import
/// completes (~2-3 iterations in practice, 100 × 20 ms = 2 s budget).
#[gpui::test]
#[serial]
fn hero_sample_click_imports_bundled_csv(cx: &mut TestAppContext) {
    use dat0_engine::QueryEngine as _;
    use gpui::{Modifiers, point, px};

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Plain hero (no enriched band / auto-show) keeps the sample column at the
    // deterministic top of the right column.
    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    set_first_run_done(&store, true).unwrap();
    // open_sample_kind early-returns unless the state root is installed.
    dat0_app::window_registry::install_state_root(state.path().to_path_buf());

    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test

    init_components(cx);
    let session = build_empty_session_in(&harness, state.path());
    let (_shell, cx) = open_shell_window(cx, Arc::clone(&session));
    cx.run_until_parked();

    let iris_csv = state.path().join("samples").join("iris.csv");
    assert!(!iris_csv.exists(), "precondition: sample not yet extracted");

    // Click the first sample card.
    // NOTE: (1700, 40) is empirically tuned for the fixed 1920×1080 TestDisplay
    // (first sample card at the top of the 280 px right column; deterministic
    // on both platforms).
    cx.simulate_click(point(px(1700.), px(40.)), Modifiers::none());

    // Sync side-effect is immediate (inside open_sample_kind before the spawn):
    // bundled CSV extracted to samples/iris.csv.
    assert!(
        iris_csv.exists(),
        "hero-sample-0 must extract the bundled Iris CSV"
    );

    // Async tail: pump the gpui foreground queue until the `iris` table appears
    // in the engine, or 2 s elapses (~100 × 20 ms). Each cycle:
    //   1. run_until_parked  — drives the detached task to its next park point
    //                          (spawn_blocking for sniff, then CTAS, then done).
    //   2. get_tables        — check whether the CTAS has committed.
    //   3. thread::sleep     — let the blocking pool finish the pending work so
    //                          the waker re-enqueues the task continuation.
    let engine = session.lock().engine.clone();
    let mut imported = false;
    for _ in 0..100 {
        cx.run_until_parked();
        let tables = harness
            .block_on(async { engine.get_tables().await })
            .unwrap_or_default();
        if tables.iter().any(|t| t.name == "iris") {
            imported = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        imported,
        "hero-sample-0 must import iris to completion (tables: {:?})",
        harness
            .block_on(async { engine.get_tables().await })
            .unwrap_or_default()
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    // Settle: drain any remaining detached-task work (add_tab → persist and
    // route_drop_outcomes → GridDataSource::new) while `state` is still alive.
    // The `#[gpui::test]` macro injects `dispatcher.run_until_parked()` AFTER
    // the test function returns (i.e., after local variables including `state`
    // are dropped). If pending task work remains at that point, `persist()`
    // hits ENOENT because the temp dir has already been cleaned up. Two settle
    // rounds are sufficient (one for add_tab, one for GridDataSource::new).
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        cx.run_until_parked();
    }

    drop(state);
}

// ----------------------------------------------------------------------------
// Per-panel Next navigation — CONTENT assertion via AccessKit (UAT Gap 2).
// ----------------------------------------------------------------------------

/// Un-ignores the former dead-end (`carousel_next_back_navigation_is_human_uat`):
/// the per-panel headline IS now observable headlessly. The carousel titles/bodies
/// carry test-only `.a11y_label` annotations, so `A11ySnapshot::capture` reads the
/// AccessKit tree the render emitted and asserts WHICH panel headline is on screen
/// — the rendered-text extraction gpui itself cannot do.
///
/// Mechanism (why capture sees the current panel): the tour is a re-presented
/// gpui-component `Dialog`; `WorkspaceShell::render` mounts `Root::render_dialog_layer`,
/// which re-invokes the stored dialog builder on every frame. `capture`'s forced
/// `window.refresh()` therefore re-runs `present_panel`'s body, re-firing the
/// `.a11y_label`/`.a11y` calls for the CURRENT panel. Clicking "Next" BY LABEL
/// (no pixel constant) closes-then-re-presents at panel 1; a fresh capture then
/// finds panel 1's headline and no longer finds panel 0's.
///
/// The dialog's 0.25 s slide/fade animation is settled with `advance_clock`
/// before each capture so `run_until_parked` yields a single stable frame (a
/// mid-animation frame would re-request renders and duplicate the captured nodes,
/// which would make the unique-match `has_label` panic).
///
/// Teeth: asserting `p2_title` present (or `p1_title` absent) BEFORE the click
/// fails — proving the assertion is bound to the actual on-screen panel.
#[gpui::test]
#[serial]
fn carousel_next_advances_panel_text(cx: &mut TestAppContext) {
    use std::time::Duration;

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Seed first_run_done=true so the empty-state renders the PLAIN hero (the
    // enriched band's a11y nodes render only when the flag is false) and posts no
    // auto-show. The only captured nodes are then the carousel's own, keeping the
    // by-label lookups unambiguous (unique-match — `has_label` panics on dups).
    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    set_first_run_done(&store, true).unwrap();

    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();
    assert!(!dialog_open(cx), "no dialog before the explicit open");

    // Open the carousel at panel 0 (plain App context — `open` re-enters the
    // active window itself, so it must NOT be nested in a window update).
    cx.cx.update(dat0_app::onboarding::open);
    cx.run_until_parked();
    assert!(dialog_open(cx), "carousel must be open at panel 0");

    let p1_title = dat0_app::dat0_i18n::t(dat0_app::onboarding::panels::PANELS[0].title_key);
    let p2_title = dat0_app::dat0_i18n::t(dat0_app::onboarding::panels::PANELS[1].title_key);

    // Settle the open animation, then read the emitted tree.
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let snap = A11ySnapshot::capture(cx);
    assert!(
        snap.has_label(&p1_title),
        "panel 0 headline ({p1_title:?}) must render at open"
    );
    assert!(
        !snap.has_label(&p2_title),
        "panel 1 headline ({p2_title:?}) must NOT render before Next"
    );

    // Click Next BY LABEL — resolves id `tour-next` → `debug_bounds` → real click
    // at the button's painted centre (retires the old (777,550) constant).
    snap.click(cx, &dat0_app::dat0_i18n::t("onboarding.tour.next"));
    cx.run_until_parked();
    // Settle the newly-presented panel's animation before the second capture.
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    let snap2 = A11ySnapshot::capture(cx);
    assert!(
        snap2.has_label(&p2_title),
        "Next must advance to panel 1 headline ({p2_title:?})"
    );
    assert!(
        !snap2.has_label(&p1_title),
        "panel 0 headline ({p1_title:?}) must be gone after advancing"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// 7. insta proof slice — gate the SERIALIZED state a behavioral flow produces.
// ----------------------------------------------------------------------------

/// PROOF SLICE for the "do-now" tier of the 2026-06-29 UAT-automation research:
/// `insta` snapshot-gates the *serialized* state that a real production code
/// path writes — here the `settings.toml` produced by the first-run-done persist
/// path (`set_first_run_done` → `SettingsStore::save` → `toml::to_string_pretty`),
/// the exact path the carousel "Skip" / "Get started" handlers reach via
/// `mark_first_run_done`.
///
/// Why this beats the hand-rolled `first_run_done == true` assert in
/// `skip_click_dismisses_and_writes_flag`: the snapshot captures the ENTIRE
/// serialized file, so any drift in the settings schema, a field default, or the
/// TOML formatting fails the gate — not just the one boolean. The committed
/// `.snap` is the regression baseline; insta never auto-creates snapshots under
/// `CI`, so a changed serialization reddens the build (run `cargo insta review`
/// locally to accept an intended change).
///
/// Deterministic + cross-platform: `Settings` carries no paths/timestamps/random
/// in its defaults, so the bytes are byte-identical on macOS and Linux CI. Plain
/// `#[test]` — it writes to an explicit `with_path` store and never touches the
/// process-global `DAT0_CONFIG_DIR`, so it needs no `#[serial]`. The value is in
/// gating serialized output; the same one-liner extends to session.json,
/// generated SQL, and `.dat0` export manifests.
#[test]
fn persisted_settings_toml_is_snapshot_gated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());

    // Drive the real production persist path (load defaults → set flag → atomic
    // save), exactly what the onboarding Skip / Get-started handlers invoke.
    set_first_run_done(&store, true).unwrap();

    // Gate the exact bytes on disk, not a reconstructed struct.
    let toml = std::fs::read_to_string(&path).unwrap();
    insta::assert_snapshot!("persisted_settings_toml", toml);
}

// ----------------------------------------------------------------------------
// Gap 3 — async-flow support: canonical "engine op completes in-harness" test.
// ----------------------------------------------------------------------------

/// T0 spike (now a permanent regression test): with an entered tokio runtime +
/// `allow_parking`, a `cx.spawn`ed flow that hits `tokio::task::spawn_blocking`
/// (here a real CSV import via the production `handle_drop`) runs to COMPLETION
/// under `#[gpui::test]` — no "no reactor running" panic, and the table is
/// registered in the engine afterwards.
///
/// ## Which path worked: brief fallback 1 (capture the task + drive it).
///
/// The brief's PRIMARY path (`cx.spawn(..).detach(); cx.run_until_parked()`) does
/// NOT complete the import — it leaves the engine empty (no panic, but no table).
/// Root cause (verified against gpui 0.2.2): `run_until_parked` is literally
/// `while dispatcher.tick(false) {}`; it drains the gpui executor's *current*
/// queue and returns the instant the foreground task parks on the cross-thread
/// tokio `JoinHandle` (the `spawn_blocking` inside `handle_drop`). It does NOT
/// wait for the off-thread completion to re-enqueue the task. `allow_parking`
/// only affects the `block`/`block_test` path, not `run_until_parked`.
///
/// So we apply fallback 1: capture the `Task` and drive it with a primitive that
/// parks for the cross-thread wake — `BackgroundExecutor::block_test`
/// (`block_internal(background_only = false, .., timeout = None)`). It ticks the
/// FOREGROUND queue (where `cx.spawn` lands the task — `block()` uses
/// `background_only = true` and would skip it) and, with `allow_parking` set,
/// parks on its own unparker instead of panicking; the dispatcher's
/// `dispatch`/`dispatch_on_main_thread` call `unpark_last()` when the tokio
/// completion re-enqueues the runnable, so it re-ticks and runs the import to the
/// end. (`harness.block_on(task)` would NOT work: tokio's `block_on` polls the
/// gpui `Task` handle but never ticks the gpui dispatcher, so the foreground
/// import body never runs.) The runtime is still entered for all of this so the
/// `spawn_blocking` inside the import finds a reactor — that part of the brief's
/// hypothesis (mirror `run_app`'s `runtime.enter()`) holds; only the *driver* had
/// to change from `run_until_parked` to `block_test`.
#[gpui::test]
#[serial]
fn engine_backed_async_flow_completes_in_harness(cx: &mut TestAppContext) {
    use dat0_engine::QueryEngine as _;

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test

    init_components(cx);
    let session = build_empty_session_in(&harness, state.path());

    // A tiny CSV on disk; importing it exercises the spawn_blocking engine path.
    let csv = state.path().join("spike.csv");
    std::fs::write(&csv, "a,b\n1,2\n3,4\n").unwrap();

    let (_shell, cx) = open_shell_window(cx, Arc::clone(&session));
    cx.run_until_parked();

    // Spawn the real import flow exactly like the UI does (foreground `cx.spawn`),
    // then DRIVE the captured task to completion with `block_test` (fallback 1) so
    // the cross-thread `spawn_blocking` wake is actually awaited.
    let sess = Arc::clone(&session);
    let csv2 = csv.clone();
    let task = cx.cx.spawn(async move |_app| {
        let _ = dat0_app::file_drop::handle_drop(vec![csv2], sess).await;
    });
    cx.executor().block_test(task);

    // The import's spawn_blocking completed → the engine has the `spike` table.
    let engine = session.lock().engine.clone();
    let tables = harness
        .block_on(async move { engine.get_tables().await })
        .expect("get_tables");
    assert!(
        tables.iter().any(|t| t.name == "spike"),
        "engine-backed async import must complete in-harness (tables: {:?})",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    drop(state);
}
