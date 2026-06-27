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

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use parking_lot::Mutex;
use serial_test::serial;

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

    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
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
/// buttons rendered inside a `window.open_dialog` overlay. A real pixel-click on
/// the carousel's "Skip" button dismisses the dialog AND persists
/// `first_run_done=true` (via the production `mark_first_run_done` →
/// `settings.toml` write). The click coordinates are derived from the dialog's
/// deterministic geometry on the fixed 1920×1080 `TestDisplay`
/// (`window_paddings` is 0 under `Decorations::Server`, identical on macOS and
/// Linux); the slide-down animation is settled with `advance_clock` first so the
/// hit-box is at its resting position.
#[gpui::test]
#[serial]
fn skip_click_dismisses_and_writes_flag(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
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
    // hit-box, then click the "Skip" button (bottom-left of the controls row).
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.simulate_click(point(px(777.), px(550.)), Modifiers::none());
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
// 6. Hero sample button → real click drives the wired sample-open helper.
// ----------------------------------------------------------------------------

/// Clicking the first hero sample card ("Iris", id `hero-sample-0`) drives the
/// production `open_sample_kind` handler, which SYNCHRONOUSLY extracts the
/// bundled CSV into `$state_root/samples/iris.csv` before spawning the async
/// import. We assert that extraction — a real, observable side effect of the
/// click reaching the wired helper.
///
/// HEADLESS BOUNDARY (honest note, see report): the subsequent
/// `cx.spawn(handle_drop(..))` runs the import via `tokio::task::spawn_blocking`
/// (`file_drop.rs:76`, then `duckdb_engine.rs:254`). The gpui foreground test
/// executor is NOT a tokio runtime, and entering one (`rt.enter()`) does not
/// survive across the future's `.await` points when it is the gpui executor — not
/// tokio — re-polling the task, so spawn_blocking panics with "no reactor
/// running" *inside the detached task*. Production runs under tokio so this works
/// there; here the FULL tab-open cannot execute. We therefore drive the real
/// click, catch that boundary panic, and assert the deterministic reachable
/// observable: the bundled CSV is on disk (extracted synchronously, before the
/// spawn). The async tab-open itself is human-UAT-owed.
#[gpui::test]
#[serial]
fn hero_sample_click_extracts_bundled_csv(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Plain hero (no enriched band / no auto-show attempt) keeps the sample
    // column at the deterministic top of the right column.
    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    set_first_run_done(&store, true).unwrap();
    // `open_sample_kind` early-returns with an error banner unless the state
    // root is installed; point it at the test temp so the extraction target is
    // observable.
    dat0_app::window_registry::install_state_root(state.path().to_path_buf());

    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    let iris = state.path().join("samples").join("iris.csv");
    assert!(!iris.exists(), "precondition: sample not yet extracted");

    // Click the first sample card (full-width row near the top of the 280px
    // right column on the 1920-wide TestDisplay). The detached async import then
    // hits the tokio/gpui-executor boundary and panics WITHIN the spawned task;
    // silence the default hook for that expected panic and catch it so the test
    // can assert the synchronous side effect that already happened.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.simulate_click(point(px(1700.), px(40.)), Modifiers::none());
        cx.run_until_parked();
    }));
    std::panic::set_hook(prev_hook);

    assert!(
        iris.exists(),
        "clicking hero-sample-0 must extract the bundled Iris CSV (proves the \
         wired open_sample_kind ran up to the headless tokio boundary)"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// Per-panel Next/Back navigation — human-UAT-owed (documented dead-end).
// ----------------------------------------------------------------------------

/// The brief asks to assert "Next → panel 1, Back → panel 0". That per-panel
/// index is NOT observable headlessly: the carousel is a one-shot re-presented
/// `Dialog` with no current-panel accessor, and gpui's test harness exposes no
/// rendered-text extraction to read the panel's headline/body. Worse, a fixed
/// pixel cannot reliably drive "Next" across panels: the "Back" button only
/// appears from panel 1 on, which shifts the right-hand control group, so
/// repeated clicks at one point oscillate between Next and Back instead of
/// advancing. `skip_click_dismisses_and_writes_flag` DOES prove overlay clicks
/// reach the controls row and fire a real handler (Skip → flag + dismiss); the
/// remaining per-panel forward/back *visual* sequencing is left to manual UAT.
#[gpui::test]
#[ignore = "per-panel Next/Back index is not observable headlessly — human UAT (see doc comment + report)"]
fn carousel_next_back_navigation_is_human_uat() {
    // Intentionally empty: documents what a human must still verify (the dots
    // pager advances, Back returns, "Get started" appears on the last panel).
}
