//! UAT "keyboard-nav / focus reachability" slice (Slice 6) — Task 0 HARD GATE.
//!
//! This is the load-bearing spike. It proves, in ONE windowed `#[gpui::test]`,
//! the six criteria the whole slice rests on:
//!   (1) a `focus_stop` div is Tab-reachable via gpui-component `Root`;
//!   (2) Enter on the focused control fires its `on_key_down` handler (tour opens);
//!   (3) the `.focus()` ring compiles + applies (build succeeds + no panic);
//!   (4) `window.focused()` correlates to the WorkspaceShell-owned handle;
//!   (5) the focus oracle (`focused_label`) reads the element's label back;
//!   (6) the stable handle survives the recapture re-render with the SAME
//!       identity (the forced `window.refresh()` inside `A11ySnapshot::capture`
//!       does NOT lose focus — a per-frame-minted handle would).
//!
//! Harness helpers below are COPIED per-binary from `tests/motherduck_window.rs`
//! and `tests/onboarding_gpui.rs` (mount helpers + the shared `MainThreadDispatcher`
//! machinery), matching this crate's per-binary-copy precedent; only `A11ySnapshot`
//! and the tab combinators live in `tests/support/mod.rs`.

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

use support::{A11ySnapshot, press_tab};

use dat0_app::main_bridge::{MainLoop, MainThreadDispatcher};
use dat0_app::session::Session;
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

/// Focus the shell the way a real keyboard user does: click a neutral spot in
/// the enriched band's top row (60px left of the take-tour button, which sits
/// top-RIGHT after a flex-grow tagline with no click handler of its own), then
/// PROVE the click landed on the shell's own focus handle and NOT a wired hero
/// button — the T1 review hardening of the T0 spike's precondition.
///
/// T0 originally only asserted `window.focused(app).is_some()`, which is true
/// whether the click lands on the shell OR (as the hero gets denser and this
/// fixed offset drifts under a new button) on a hero control instead — a
/// silent mis-focus that would make the subsequent Tab walk start from the
/// wrong place without failing loudly. The second assertion below closes that
/// gap: the focus oracle must name NO hero button (the shell's own handle is
/// never registered in the oracle's side-map), so any future collision fails
/// here with a clear message instead of corrupting a downstream assertion.
/// Still bounds-derived (no bare pixel constants) — reused by every test in
/// this file that needs a clean, untrapped keyboard-focus baseline.
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

// ----------------------------------------------------------------------------
// T0 — focus oracle + `focus_stop` + one hero button (the HARD GATE).
// ----------------------------------------------------------------------------

/// Proves all six spike criteria in one flow. First-run (fresh temp config →
/// `first_run_done` unset) so the enriched band paints `hero-take-tour`; that
/// same condition fires the one-shot auto-show, which we flush + close first so
/// the keyboard interaction starts from a dialog-free, focus-untrapped baseline.
#[gpui::test]
#[serial]
fn t0_focus_oracle_and_take_tour(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Install the shared dispatcher (the exact one the hero handler posts onto),
    // init gpui-component (theme + Root's tab bindings), clear any stale closure.
    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx);

    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // First-run render posted the auto-show onto the dispatcher; flush it so a
    // dialog is up, then close it so focus is not trapped in the overlay. The
    // per-shell `tour_auto_shown` guard prevents any re-fire, and the band still
    // paints (first_run_done is still false), so `hero-take-tour` stays reachable.
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "sanity: first-run auto-show opened a dialog"
    );
    cx.update(|window, app| window.close_dialog(app));
    cx.run_until_parked();
    assert!(!dialog_open(cx), "baseline: auto-show dialog closed");

    // Establish window focus the way a real keyboard user does: click into the
    // workspace. gpui-component `Root`'s "tab" binding (key_context "Root") only
    // dispatches when SOMETHING inside the window subtree is focused — a fresh
    // window with nothing focused no-ops Tab (verified empirically in this T0
    // spike). The `#workspace-shell` div's bubble-phase click handler focuses the
    // shell's handle (window.rs:6533), which puts the "Root" context in the
    // dispatch path. See `focus_shell_neutrally`'s doc comment for the T1
    // hardening of this precondition (proves the click did NOT land on a wired
    // hero button, not just that *something* got focused).
    focus_shell_neutrally(cx);

    // (1) Tab reaches the take-tour focus_stop, and (4)(5)(6) the oracle names it
    // by label through the forced recapture re-render. In the current empty-state
    // shell take-tour is the first (and only) tab stop, so one Tab suffices; the
    // bounded loop is defensive — any future intervening tab stop (e.g. a
    // gpui-component button, `tab_index` 0, whose handle is NOT in the oracle
    // side-map → `focused_label()` is `None`) is simply Tabbed past until the
    // oracle names OUR button. `A11ySnapshot::capture` forces a `window.refresh()`
    // re-render before each read; the label still resolving proves the
    // WorkspaceShell-owned handle kept its identity across the re-render (6).
    let want = dat0_app::dat0_i18n::t("hero.take_tour");
    let mut reached = false;
    let mut steps = 0;
    for _ in 0..40 {
        press_tab(cx);
        steps += 1;
        let snap = A11ySnapshot::capture(cx);
        if snap.focused_label() == Some(want.as_str()) {
            reached = true;
            break;
        }
    }
    assert!(
        reached,
        "Tab did not land on the take-tour button (or the focus oracle failed to \
         name it) within 40 Tab presses"
    );
    eprintln!("t0: focus oracle named take-tour after {steps} Tab press(es)");

    // (2) Enter on the focused button fires its `on_key_down` twin → the tour
    // handler (`open_deferred`) posts onto the dispatcher; draining opens it. The
    // dialog is cleanly attributable to Enter (auto-show already closed above).
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "Enter on the focused take-tour button did not open the tour"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// T1 — wire the remaining hero buttons + prove the full Tab cycle.
// ----------------------------------------------------------------------------

/// Every hero button in the enriched band + the sample-picker column is a
/// real Tab stop, reached in DOM order: take-tour → open-demo → 3 sample
/// cards (in `sample_data::entries()` order) → open-file. All are
/// `tab_index` 0, so DOM order IS tab order (Slice 6 design invariant).
///
/// Sample titles are pulled from the real `sample_data::entries()` catalog
/// (not hardcoded), so this test tracks the catalog rather than duplicating
/// its contents — `empty_state`'s own `sample_buttons_cover_all_entries` unit
/// test already gates the catalog's shape (exactly 3 entries, in
/// Csv/Sqlite/Remote order), so relying on that order here is safe.
#[gpui::test]
#[serial]
fn hero_tab_cycle_visits_every_button(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // First-run render posted the auto-show onto the dispatcher; flush it so a
    // dialog is up, then close it so focus is not trapped in the overlay
    // (mirrors T0's baseline).
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "sanity: first-run auto-show opened a dialog"
    );
    cx.update(|window, app| window.close_dialog(app));
    cx.run_until_parked();
    assert!(!dialog_open(cx), "baseline: auto-show dialog closed");

    focus_shell_neutrally(cx);

    let samples = dat0_app::sample_data::entries();
    assert_eq!(samples.len(), 3, "precondition: exactly 3 sample entries");
    let expected: Vec<String> = vec![
        dat0_app::dat0_i18n::t("hero.take_tour"),
        dat0_app::dat0_i18n::t("hero.demo.cta"),
        samples[0].title.to_string(),
        samples[1].title.to_string(),
        samples[2].title.to_string(),
        dat0_app::dat0_i18n::t("hero.open_file"),
    ];

    for want in &expected {
        press_tab(cx);
        let snap = A11ySnapshot::capture(cx);
        assert_eq!(
            snap.focused_label(),
            Some(want.as_str()),
            "tab order mismatch at {want:?}"
        );
    }

    drop(state);
}

// ----------------------------------------------------------------------------
// T1 — Enter-activation breadth: a second button besides take-tour.
// ----------------------------------------------------------------------------

/// Enter on the focused `hero-open-demo` button drives the SAME production
/// path the mouse click reaches (`crate::window::open_demo_workspace`) all
/// the way to completion: `unpack_package_into` runs SYNCHRONOUSLY (it
/// blocks on the entered tokio runtime, unlike the sample-card import's
/// detached `cx.spawn`), materializes the demo workspace, then
/// `open_workspace_at` → `spawn_workspace_window` opens a REAL second window
/// and registers it — mirrors `onboarding_gpui.rs`'s
/// `hero_sample_click_imports_bundled_csv`, which drives a real click to a
/// real production completion (an imported table) rather than stopping at
/// the synchronous half of the flow.
///
/// Both `install_state_root` (or `open_demo_workspace` early-returns) and
/// `install_window_registry` (or `spawn_workspace_window` early-returns) are
/// required preconditions — without them the click/Enter would silently
/// no-op past the unpack, which is why the registry-length assertion below is
/// the meaningful teeth: it can only go from 0 to 1 if the ENTIRE chain
/// (unpack → recover_workspace → open_window) ran to completion.
#[gpui::test]
#[serial]
fn hero_enter_activates_open_demo(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    dat0_app::window_registry::install_state_root(state.path().to_path_buf());
    let registry = std::sync::Arc::new(parking_lot::Mutex::new(
        dat0_app::window_registry::WindowRegistry::new(),
    ));
    dat0_app::window_registry::install_window_registry(registry.clone());

    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test: `open_demo_workspace`
    // and `spawn_workspace_window` both need `tokio::runtime::Handle::try_current()`.

    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    let session = build_empty_session_in(&harness, state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // Flush + close the first-run auto-show so focus is not trapped in the
    // overlay (mirrors T0 / the tab-cycle test's baseline).
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "sanity: first-run auto-show opened a dialog"
    );
    cx.update(|window, app| window.close_dialog(app));
    cx.run_until_parked();
    assert!(!dialog_open(cx), "baseline: auto-show dialog closed");

    focus_shell_neutrally(cx);

    press_tab(cx); // hero-take-tour
    press_tab(cx); // hero-open-demo
    let snap = A11ySnapshot::capture(cx);
    assert_eq!(
        snap.focused_label(),
        Some(dat0_app::dat0_i18n::t("hero.demo.cta").as_str()),
        "second Tab must land on the open-demo button"
    );

    assert_eq!(
        registry.lock().len(),
        0,
        "precondition: no window registered before Enter"
    );

    // `open_demo_workspace` runs synchronously inside the `on_key_down` twin
    // (no dispatcher hop needed — see the empty_state.rs comment), so by the
    // time this returns the demo workspace has been unpacked, reopened, and
    // its window registered; `run_until_parked` settles the frame.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    assert_eq!(
        registry.lock().len(),
        1,
        "Enter on the focused open-demo button must drive open_demo_workspace \
         to completion — a second real window registered for the unpacked \
         demo workspace"
    );

    drop(state);
}
