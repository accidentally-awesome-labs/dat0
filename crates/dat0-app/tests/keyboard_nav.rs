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
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use parking_lot::Mutex;
use serial_test::serial;

use support::{A11ySnapshot, press_shift_tab, press_tab};

use dat0_app::grid::GridDataSource;
use dat0_app::grid::selection::CellCoord;
use dat0_app::main_bridge::{MainLoop, MainThreadDispatcher};
use dat0_app::session::Session;
use dat0_app::settings::store::SettingsStore;
use dat0_app::settings_ui::panel::SettingsPanel;
use dat0_app::window::WorkspaceShell;
use dat0_engine::QueryEngine as _;

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

// ----------------------------------------------------------------------------
// Task 1b — `hero-open-file-recents` reachability (returning users, recents
// branch). User-approved scope addition mirroring exactly how Task 1 wired
// the sibling `hero-open-file-samples` button in `sample_column`.
// ----------------------------------------------------------------------------

/// Seed one recent entry into `cfg`'s `recents.json` via the production
/// `Recents::push` API (not a hand-written JSON literal) — mirrors how a real
/// "open a file" action populates the file, and exercises the same on-disk
/// shape `window.rs`'s `recents_empty` check and `recents_column`'s own read
/// both rely on. The path need not exist: `recents_column` only renders the
/// entry's `Display`ed path as a label — nothing in this test clicks the row
/// or reads the file from disk.
fn seed_one_recent(cfg: &Path) {
    let mut recents = dat0_app::recents::Recents::with_path(cfg.join("recents.json"));
    recents
        .push(dat0_app::recents::RecentEntry::Workspace {
            path: cfg.join("some-workspace.dat0"),
        })
        .expect("seed one recent entry into recents.json");
}

/// Task 1b: the "Open file…" button shown to RETURNING users
/// (`hero-open-file-recents`, rendered by `recents_column` once recents are
/// non-empty) is a real Tab stop, wired with the same `focus_stop` + `.a11y`
/// pattern Task 1 used for `hero-open-file-samples` in `sample_column`.
///
/// Seeding one recent entry flips `recents_empty` to `false`, so
/// `EmptyState::render` takes the `recents_column` branch instead of
/// `sample_column` — in that branch `hero-open-file-samples` does not render
/// at all, so the "Open file…" label captured by the oracle is unambiguous
/// (there is exactly one node with `hero.open_file`'s text on this frame).
/// The fresh config dir still leaves `first_run_done` unset, so the enriched
/// band (and its first-run auto-show) still paints — `recents_empty` and
/// `first_run_done` are independent switches — so this test follows the same
/// auto-show flush/close baseline dance as T0/T1 before walking Tab.
#[gpui::test]
#[serial]
fn hero_open_file_recents_reachable(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    seed_one_recent(cfg.path());

    ensure_dispatcher();
    init_components(cx);
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    let session = build_empty_session(state.path());
    let (_shell, cx) = open_shell_window(cx, session);
    cx.run_until_parked();

    // First-run render posted the auto-show onto the dispatcher; flush it so a
    // dialog is up, then close it so focus is not trapped in the overlay
    // (mirrors T0's / T1's baseline — recents_empty does not suppress this).
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

    let want = dat0_app::dat0_i18n::t("hero.open_file");
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
        "Tab did not reach hero-open-file-recents (or the focus oracle failed \
         to name it) within 40 Tab presses — is recents_empty actually false, \
         and is the button wired with `focus_stop` + `.a11y`?"
    );
    eprintln!("hero_open_file_recents_reachable: reached after {steps} Tab press(es)");

    drop(state);
}

// ----------------------------------------------------------------------------
// T2 — Settings DIY toggle rows keyboard-operable + reachability tests.
// ----------------------------------------------------------------------------
//
// Mount helper + `fresh_store_path` copied from `tests/settings_window.rs`
// (per-binary-copy convention, same as the hero machinery above). Unlike the
// hero tests, these do NOT need `DAT0_CONFIG_DIR`/`#[serial]`: `toggle_row`'s
// `set` functions (`set_crash_submission_enabled` etc.) operate on the
// `SettingsStore` INJECTED into `SettingsPanel::new` (built directly from
// `fresh_store_path`'s tempdir path), not on `crate::platform::config_dir()`.
// `settings_window.rs`'s own module doc makes the same point: only the
// `adv-reset` tests need the `config_dir()` seam, because `open_reset_confirm`
// is the one path that builds its own store from `config_dir()` rather than
// reusing `self.store`. So these tests use independent tempdirs and can run
// unserialized, exactly like `telemetry_toggle_click_persists` in that file.

/// Open a real, ACTIVATED window whose root is a `gpui_component::Root`
/// wrapping a fresh standalone [`SettingsPanel`] — mirrors
/// `settings_window.rs::open_settings_window`.
fn open_settings_panel(
    cx: &mut TestAppContext,
    settings_path: PathBuf,
) -> (Entity<SettingsPanel>, &mut VisualTestContext) {
    cx.update(gpui_component::init);
    let slot: Rc<RefCell<Option<Entity<SettingsPanel>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let store = SettingsStore::with_path(settings_path.clone());
        let panel = cx.new(|c| SettingsPanel::new(store, window, c));
        *slot2.borrow_mut() = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = slot.borrow().clone().expect("panel captured");
    (panel, vcx)
}

/// A fresh backing store path for one test — mirrors
/// `settings_window.rs::fresh_store_path`.
fn fresh_store_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    (dir, path)
}

/// Reload the on-disk `crash_submission_enabled` flag from `path` — mirrors
/// `settings_window.rs::telemetry_toggle_click_persists`'s inline
/// `reload_flag` closure, hoisted to a named fn since two tests below need it.
fn telemetry_enabled(path: &Path) -> bool {
    SettingsStore::with_path(path.to_path_buf())
        .load_or_default()
        .expect("load settings")
        .telemetry
        .crash_submission_enabled
}

// ## Why there is no `focus_shell_neutrally`-style baseline click here
//
// The hero suite establishes a keyboard-focus baseline by clicking a NEUTRAL
// spot that lands on `WorkspaceShell`'s own root `track_focus` handle (see
// `focus_shell_neutrally` above) before Tab can route at all — this file's T0
// spike found that gpui-component `Root`'s "tab" binding only dispatches when
// SOMETHING inside the window subtree is already focused (confirmed again
// below: a stale/absent `window.focus` both fall back to
// `dispatch_tree.root_node_id()`, which sits ABOVE the `Root`-key-context
// node, so neither state reaches the "Root" context's Tab binding).
// `SettingsPanel` has no equivalent root-level click-to-focus of its own
// (unlike `WorkspaceShell`), so a DIFFERENT baseline is needed here — and
// empirically, two further gpui-component facts rule out the obvious options:
//
// 1. Focus does NOT survive a section switch. `window.focused(app).is_some()`
//    stays trivially `true` after navigating away from whatever was focused
//    (the STALE `FocusId` is still registered globally), but that stale id is
//    no longer part of the newly-rendered frame's dispatch tree, so it hits
//    the SAME `root_node_id()` fallback as a totally blank window — Tab
//    silently no-ops. So a baseline established in one section (e.g. an
//    `Input` in Profile) cannot carry over to prove reachability in Telemetry.
// 2. gpui-component `Button` explicitly suppresses click-to-focus:
//    `on_mouse_down` calls `window.prevent_default()` ("Avoid focus on mouse
//    down" — `gpui-component/ui/src/button/button.rs`). So EVERY button in
//    this panel (Learn More, the Advanced pane's 4 buttons, MD/AI opens) is a
//    real Tab stop but can NEVER be click-focused — ruling out "click a safe
//    Button in the target pane" as a baseline.
//
// The only elements in the WHOLE panel a mouse click can focus are: the two
// Profile `Input`s, and — now — the 3 DIY toggle rows themselves (`focus_stop`
// chains `.track_focus`, which does NOT call `prevent_default`). Given (1),
// the baseline must be established INSIDE the SAME section as the assertion.
// So each test below establishes its baseline by clicking the toggle under
// test — this does focus it directly, but the reachability proof is NOT the
// click: it is the subsequent Shift-Tab (moves away, landing on the section's
// other tab stop) followed by a forward Tab (which must land BACK on the
// toggle) — a genuine round trip through gpui-component `Root`'s real "tab"/
// "shift-tab" keybindings and `Window::focus_next`/`focus_prev`, exactly the
// mechanism a keyboard-only user relies on, without ever touching a
// side-effecting control (Learn More's `on_click` shells out to
// `crate::platform::open_url` — confirmed by reading `render_telemetry` — so
// it is deliberately never clicked by this harness).

/// Task 2 (Step 1/failing -> Step 4/passing): a genuine Tab round trip reaches
/// the telemetry DIY toggle, and Space flips it — proving both keyboard
/// reachability AND operability in one flow.
#[gpui::test]
fn settings_toggle_keyboard_reachable_and_operable(cx: &mut gpui::TestAppContext) {
    use gpui::Modifiers;

    let (_dir, path) = fresh_store_path();
    let (_panel, vcx) = open_settings_panel(cx, path.clone());

    // Navigate to Telemetry — `tg-telemetry` only renders inside
    // `render_telemetry`, and the default section is Profile.
    let telemetry_row_label = dat0_i18n::t("settings.telemetry");
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, &telemetry_row_label);
    vcx.run_until_parked();

    let toggle_label = dat0_i18n::t("settings.telemetry.toggle");
    let before = telemetry_enabled(&path);

    // Establish the in-frame focus baseline by clicking the toggle itself
    // (see the module note above for why this is unavoidable) — this ALSO
    // flips it once, which the operability assertions below account for.
    let bounds = vcx
        .debug_bounds("tg-telemetry")
        .expect("tg-telemetry must have painted bounds in the Telemetry pane");
    vcx.simulate_click(bounds.center(), Modifiers::none());
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.focused_label(),
        Some(toggle_label.as_str()),
        "clicking the telemetry toggle must focus it (precondition for the Tab \
         round trip below)"
    );
    assert_ne!(
        telemetry_enabled(&path),
        before,
        "sanity: clicking the toggle must flip it (same on_click as the mouse path)"
    );

    // Shift-Tab moves focus AWAY from the toggle (to the pane's only other tab
    // stop, "Learn More" — a gpui-component `Button`, invisible to the oracle,
    // so `focused_label()` reads `None` here; the meaningful assertion is that
    // it is no longer the toggle's label).
    press_shift_tab(vcx);
    let snap = A11ySnapshot::capture(vcx);
    assert_ne!(
        snap.focused_label(),
        Some(toggle_label.as_str()),
        "Shift-Tab must move focus away from the telemetry toggle"
    );

    // Forward Tab must land BACK on the toggle — the reachability proof this
    // task requires: a real "tab" keystroke, dispatched through
    // gpui-component `Root`'s real keybinding, reaches the DIY toggle.
    press_tab(vcx);
    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.focused_label(),
        Some(toggle_label.as_str()),
        "Tab did not bring focus back to the telemetry DIY toggle"
    );

    // Operability: Space on the now-refocused toggle flips it again.
    vcx.simulate_keystrokes("space");
    vcx.run_until_parked();
    assert_eq!(
        telemetry_enabled(&path),
        before,
        "Space on the focused telemetry toggle did not flip it back"
    );

    // Teeth: flip it again and confirm it moves once more — proves the
    // assertion above reads real toggle state, not a one-shot fluke.
    vcx.simulate_keystrokes("space");
    vcx.run_until_parked();
    assert_ne!(
        telemetry_enabled(&path),
        before,
        "a second Space press must flip the telemetry toggle again"
    );
}

/// Task 2 (Step 5, adjusted per empirical finding — see the module note
/// above): reachability for the Settings window's OTHER focusable controls.
///
/// `gpui_component::input::Input`'s `FocusHandle` is internal to the widget —
/// it is never passed through `.focus_stop`/recorded in the oracle's
/// side-map (`a11y/mod.rs`'s `record_focus_id` side-map), so
/// `A11ySnapshot::focused_label()` returns `None` whenever an `Input` holds
/// focus. Confirmed non-vacuously below: `window.focused(app)` DOES change
/// across the two Tab presses (real focus movement is happening — Name then
/// Email, the Profile pane's first two tab stops), while `focused_label()`
/// stays `None` throughout — the oracle is blind to `Input` focus, but Tab
/// itself is genuinely moving it. A bare `is_none()` assertion alone would be
/// vacuous (it would also pass if Tab silently did nothing); pairing it with
/// the real `window.focused()` identity check makes it a real test of the
/// documented limitation, not a tautology.
///
/// The brief's fallback ("assert Tab reaches the Reset button") is NOT
/// reachable via this harness: every gpui-component `Button` in this panel
/// (`adv-reset` included) suppresses click-to-focus (see the module note), so
/// there is no way to establish an in-frame baseline in the Advanced pane
/// without clicking a Button first — and a stale baseline from another
/// section does not carry over (also documented above). So this test instead
/// proves the OTHER two DIY toggles (`tg-workspace`, `tg-updates`) are
/// keyboard-operable via the SAME `focus_stop` wiring as telemetry, closing
/// out per-instance coverage of all 3 `toggle_row` call sites (the telemetry
/// test above already proves the shared `focus_stop`/`Root` Tab-routing
/// mechanism end to end, including the round trip; these two reuse that same
/// code path, so a click-then-Space check is sufficient here without
/// repeating the full round trip 3 times).
#[gpui::test]
fn settings_other_toggles_reachable_and_operable(cx: &mut gpui::TestAppContext) {
    use gpui::Modifiers;

    let (_dir, path) = fresh_store_path();
    let (_panel, vcx) = open_settings_panel(cx, path.clone());

    // Confirm the Input-invisibility limitation empirically, non-vacuously.
    // Establish a real baseline by clicking the Name input directly (Profile
    // is the default section; Name/Email are its only two tab stops, and
    // unlike the toggle tests above there is no OTHER click-focusable,
    // non-Button element in this pane to seed focus from instead — Input is
    // the thing under test here).
    let name_bounds = vcx
        .debug_bounds("settings-name-input")
        .expect("settings-name-input must have painted bounds in the default Profile pane");
    vcx.simulate_click(name_bounds.center(), Modifiers::none());
    vcx.run_until_parked();
    let mut prior_focus = vcx.update(|window, app| window.focused(app));
    assert!(
        prior_focus.is_some(),
        "clicking the Name input must establish window focus"
    );
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.focused_label().is_none(),
        "gpui-component Input focus must be invisible to the oracle \
         (documented limitation) — if this ever becomes visible, replace \
         this test with a direct Input-label reachability assertion"
    );

    // Now Tab across Name -> Email (and back), checking real focus movement
    // (`window.focused()` changes) while `focused_label()` stays `None`
    // throughout — proves Tab IS moving focus, the oracle just cannot name
    // an `Input`.
    let mut moved = false;
    for _ in 0..2 {
        press_tab(vcx);
        let snap = A11ySnapshot::capture(vcx);
        let now_focus = vcx.update(|window, app| window.focused(app));
        assert!(
            snap.focused_label().is_none(),
            "gpui-component Input focus must be invisible to the oracle \
             (documented limitation) — if this ever becomes visible, replace \
             this test with a direct Input-label reachability assertion"
        );
        if now_focus != prior_focus {
            moved = true;
        }
        prior_focus = now_focus;
    }
    assert!(
        moved,
        "sanity: Tab must genuinely move window focus across the two Profile \
         Inputs even though the oracle cannot name either — otherwise the \
         `is_none()` checks above would be vacuously true"
    );

    // Workspace toggle: click-focus + Space-flip round trip.
    let workspace_row_label = dat0_i18n::t("settings.workspace");
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, &workspace_row_label);
    vcx.run_until_parked();

    let workspace_toggle_label = dat0_i18n::t("settings.workspace.toggle");
    let ws_before = SettingsStore::with_path(path.clone())
        .load_or_default()
        .expect("load settings")
        .workspace
        .treat_all_as_networked;
    let bounds = vcx
        .debug_bounds("tg-workspace")
        .expect("tg-workspace must have painted bounds in the Workspace pane");
    vcx.simulate_click(bounds.center(), Modifiers::none());
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.focused_label(),
        Some(workspace_toggle_label.as_str()),
        "clicking the workspace toggle must focus it"
    );
    vcx.simulate_keystrokes("space");
    vcx.run_until_parked();
    let ws_after_two_flips = SettingsStore::with_path(path.clone())
        .load_or_default()
        .expect("load settings")
        .workspace
        .treat_all_as_networked;
    assert_eq!(
        ws_after_two_flips, ws_before,
        "Space on the focused workspace toggle (after the click already \
         flipped it once) must flip it back to its original value"
    );

    // Updates toggle: same round trip.
    let updates_row_label = dat0_i18n::t("settings.updates");
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, &updates_row_label);
    vcx.run_until_parked();

    let updates_toggle_label = dat0_i18n::t("settings.updates.toggle");
    let up_before = SettingsStore::with_path(path.clone())
        .load_or_default()
        .expect("load settings")
        .update_auto_check;
    let bounds = vcx
        .debug_bounds("tg-updates")
        .expect("tg-updates must have painted bounds in the Updates pane");
    vcx.simulate_click(bounds.center(), Modifiers::none());
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.focused_label(),
        Some(updates_toggle_label.as_str()),
        "clicking the updates toggle must focus it"
    );
    vcx.simulate_keystrokes("space");
    vcx.run_until_parked();
    let up_after_two_flips = SettingsStore::with_path(path.clone())
        .load_or_default()
        .expect("load settings")
        .update_auto_check;
    assert_eq!(
        up_after_two_flips, up_before,
        "Space on the focused updates toggle (after the click already flipped \
         it once) must flip it back to its original value"
    );
}

// ----------------------------------------------------------------------------
// Task 3 — Grid Tab-reachability + arrow-nav via SelectionModel.
// ----------------------------------------------------------------------------
//
// The grid does NOT drive cell navigation through the gpui tab-focus chain
// the way the hero buttons / Settings DIY toggles do (Tasks 0-2 above).
// Instead: Tab must reach the grid SHELL — the ONE `track_focus` handle on
// the `#workspace-shell` root div — and from there ARROW keys drive the
// `SelectionModel` via `grid/keymap.rs` (`key_from_event` -> `apply_key`),
// entirely independent of gpui focus. The grid's visible ring
// (`GridTableDelegate::render_td`'s `is_active` cell) tracks
// `SelectionModel::active()`, not `window.focused()`. So this test asserts
// arrow-nav via `grid_active_cell_for_test()` (the `SelectionModel`), not the
// `A11ySnapshot` focus oracle Tasks 0-2 use (grid cells are `.a11y_label`
// content nodes, not focusable — there is nothing for that oracle to name
// here).
//
// ## Why this test calls `window.focus_next()` directly instead of only
// `press_tab` (empirically derived — read before changing this test)
//
// Every hero/Settings test above establishes a baseline with a REAL click
// before ever pressing Tab, because gpui-component `Root`'s "tab" binding
// only dispatches once SOMETHING in the window is already focused (T0's
// finding, `focus_shell_neutrally`'s doc comment). In the grid's minimal
// mounted scene there is no OTHER focusable element to click that is NOT
// the shell itself — every click bubbles (nothing intercepts propagation)
// to `#workspace-shell`'s own `on_click(click_to_focus)`, which calls
// `self.focus_handle.focus(window)` DIRECTLY. That direct-focus path
// predates this task (T11) and is unaffected by `tab_stop`/`tab_index`, so
// a click-then-Tab-then-arrow flow passes identically whether or not the
// shell is a registered tab stop — verified empirically by temporarily
// forcing `tab_stop(false)` and re-running: click+Tab+Down still moved the
// active cell. That flow therefore cannot serve as this task's RED/GREEN
// proof; it is exercised separately below as a realistic-UX regression
// guard instead.
//
// The property Task 3 actually changes is `Window::focus_next()`'s tab-order
// walk (`TabStopMap::next`): with NOTHING focused, `next(None)` returns the
// first REGISTERED tab stop, or `None` if no node has `tab_stop == true`
// anywhere in the frame (verified by reading `gpui-0.2.2/src/tab_stop.rs`).
// Before this task neither the shell (`track_focus`'d, `tab_stop` never
// set) nor gpui-component `Table`'s own internal handle (also `tab_stop:
// false` by construction) satisfies that, so `focus_next()` is a genuine
// no-op from a clean window — confirmed empirically: forcing
// `tab_stop(false)` and calling `window.focus_next()` from a freshly
// dialog-flushed, nothing-focused window left `window.focused()` at `None`
// and a subsequent Down keystroke left the active cell at its starting
// `(0, 0)`. `window.focus_next()` is exactly the function
// `gpui_component::Root::on_action_tab` invokes for a real "tab" keystroke
// once dispatch reaches it (`root.rs`); calling it directly here tests the
// SAME production mechanism the fix touches, while sidestepping the
// separate, pre-existing, already-documented "Root dispatch needs a prior
// focus" precondition — an orthogonal harness/gpui-component limitation
// this task neither introduces nor is responsible for fixing.

/// Import `a,b\n1,2\n3,4\n5,6\n` via the production `handle_drop` flow (the
/// Slice-3/5 grid-seeding recipe, copied from
/// `tests/a11y_content.rs::grid_renders_cell_values_as_a11y_cells`), mount it
/// as the active grid, and settle the PD-018 page-0 prefetch so the
/// `SelectionModel` is a real, non-empty (3 rows x 2 cols) grid — enough rows
/// for two separate "down" presses to each move the active cell by one row
/// without clamping at the bottom edge.
#[gpui::test]
#[serial]
fn grid_tab_reach_then_arrow_moves_active_cell(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test
    init_components(cx);
    drain_dispatcher(cx); // clear any stale queued closure (no window yet → no-op)

    let session = build_empty_session_in(&harness, state.path());

    let csv = state.path().join("cells.csv");
    std::fs::write(&csv, "a,b\n1,2\n3,4\n5,6\n").unwrap();

    let (shell, cx) = open_shell_window(cx, Arc::clone(&session));
    cx.run_until_parked();

    // MUST flush + close the first-run auto-show tour dialog before doing
    // anything else — exactly the T0/T1 baseline dance above. Skipping this
    // was an earlier iteration's bug: `Root::open_dialog` mints a FRESH focus
    // handle and calls `.focus(window)` on it immediately, which silently
    // steals + traps focus (a CHILD of `#workspace-shell`, since the dialog
    // layer is chained onto the same root div) and confounds every
    // `window.focused()` probe below with the dialog's own handle instead of
    // whatever the test is actually trying to observe.
    drain_dispatcher(cx);
    cx.run_until_parked();
    assert!(
        dialog_open(cx),
        "sanity: first-run auto-show opened a dialog"
    );
    cx.update(|window, app| window.close_dialog(app));
    cx.run_until_parked();
    assert!(!dialog_open(cx), "baseline: auto-show dialog closed");

    // Import via the production `handle_drop` flow, driven to completion with
    // `block_test` (Gap-3 fallback) so the cross-thread `spawn_blocking` wake
    // is awaited and the table is actually registered.
    let sess = Arc::clone(&session);
    let csv2 = csv.clone();
    let task = cx.cx.spawn(async move |_app| {
        let _ = dat0_app::file_drop::handle_drop(vec![csv2], sess).await;
    });
    cx.executor().block_test(task);

    let engine = session.lock().engine.clone();
    let tables = harness
        .block_on(async { engine.get_tables().await })
        .expect("get_tables");
    let table_name = tables
        .iter()
        .map(|t| t.name.clone())
        .next()
        .expect("the CSV import must register exactly one table");
    let ds = harness
        .block_on(async { GridDataSource::new(Arc::clone(&engine), table_name).await })
        .expect("GridDataSource::new");
    let ds = Arc::new(ds);

    // Mount it as the active grid view (mirrors `route_drop_outcomes`'s
    // `set_data_source` + `notify`).
    let ds_for_mount = Arc::clone(&ds);
    shell.update(cx, |view, cx| {
        view.set_data_source(ds_for_mount);
        cx.notify();
    });

    // Pump the foreground queue + drain the dispatcher until page 0 is
    // resident in the grid LRU (PD-018) — until then `render_td` paints the
    // em-dash placeholder for every cell, which is irrelevant to selection
    // movement, but settling here also gives the lazily-built
    // `SelectionModel` (constructed once `rows > 0 && cols > 0` on the same
    // render, `window.rs`) time to exist before we read it.
    let mut page_ready = false;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        if ds.cell_render(0, 0).is_some() {
            page_ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        page_ready,
        "page 0 must load into the grid LRU before the grid can be interacted with"
    );

    // ── Clean baseline: nothing focused, active cell at its fresh origin ────
    assert!(
        cx.update(|window, app| window.focused(app).is_none()),
        "clean baseline: nothing should be focused yet (dialog flushed, no click)"
    );
    let before: CellCoord = shell.update(cx, |ws, _cx| ws.grid_active_cell_for_test());
    assert_eq!(
        before,
        CellCoord { row: 0, col: 0 },
        "sanity: a freshly-mounted SelectionModel starts at the origin cell"
    );

    // ── Core RED/GREEN proof: Tab-reachability via the registered tab stop ──
    //
    // RED (pre-fix, verified manually by forcing `tab_stop(false)`):
    // `focus_next()` leaves `window.focused()` at `None` and the following
    // Down keystroke leaves `before` unchanged — Tab genuinely does not
    // reach the grid, so the arrow has nothing to drive.
    //
    // GREEN (post-fix, this assertion): `focus_next()` finds the shell as
    // the (only, but now genuinely registered) tab stop, focus becomes
    // `Some`, and the dispatch path for the following Down keystroke now
    // includes `#workspace-shell`'s `on_key_down(key_handler)` — the active
    // cell advances exactly one row.
    cx.update(|window, _app| window.focus_next());
    cx.run_until_parked();
    assert!(
        cx.update(|window, app| window.focused(app).is_some()),
        "Tab (focus_next) must reach the grid shell now that it is a \
         registered tab stop — before this task's fix, the shell's \
         `track_focus`'d handle had no `tab_stop`/`tab_index` set on it, so \
         `focus_next()` found nothing and this stayed `None`"
    );

    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    let after: CellCoord = shell.update(cx, |ws, _cx| ws.grid_active_cell_for_test());
    assert_ne!(
        before, after,
        "arrow key did not move the grid active cell — Tab must not have \
         actually reached the grid's own dispatch subtree"
    );
    assert_eq!(after.row, before.row + 1, "Down should advance one row");

    // ── Realistic-UX regression guard: a real click, then a real "tab" ──────
    // keystroke, then another arrow press. `#workspace-shell`'s own
    // `on_click(click_to_focus)` (pre-dating this task) already focuses the
    // shell directly on any unhandled click, independent of `tab_stop` — so
    // this flow is NOT itself a RED/GREEN proof (confirmed empirically: it
    // passes even with `tab_stop(false)` forced). It IS a genuine guard that
    // the Task 3 wiring does not regress the ordinary mouse-then-keyboard
    // path: a real "tab" keystroke, now satisfying the Root-dispatch
    // precondition (something is already focused, from the click), must not
    // knock focus OFF the grid, and the arrow must still drive the
    // `SelectionModel` afterward.
    let win_bounds = cx.update(|window, _app| window.bounds());
    cx.simulate_click(win_bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.update(|window, app| window.focused(app).is_some()),
        "a click into the grid must focus something (precondition for Tab)"
    );

    press_tab(cx);
    cx.run_until_parked();
    assert!(
        cx.update(|window, app| window.focused(app).is_some()),
        "a real Tab keystroke must not un-focus the grid shell — the shell \
         is the sole registered tab stop here, so Tab should cycle back onto \
         it rather than dropping focus"
    );

    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    let after2: CellCoord = shell.update(cx, |ws, _cx| ws.grid_active_cell_for_test());
    assert_eq!(
        after2.row,
        after.row + 1,
        "arrow-nav must still work after a real click + real Tab keystroke"
    );

    drop(state);
}
