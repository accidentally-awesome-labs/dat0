//! UAT "AI config-panel" slice — T0 HARD GATE.
//!
//! `t0_ai_nav_gate` is the load-bearing spike. It proves, in ONE windowed
//! `#[gpui::test]`, the four risks the whole slice rests on:
//!   Probe 1: Tab reaches the AI dock's `ai-toggle-enabled` `focus_stop` (the
//!            oracle names it by its label text);
//!   Probe 2: Enter on the focused button flips the draft `enabled` flag
//!            (keyboard operability through `handle_ai_panel_event`);
//!   Probe 3: the SQL-Console `nl2sql-chip` is Tab-reachable across the
//!            shell→console-view boundary;
//!   Probe 4: Enter on the focused chip emits `OpenNl2SqlPrompt`.
//!
//! Mount scaffolding below is COPIED per-binary from `tests/catalog_nav.rs`
//! (this crate's per-binary-copy precedent) — `set_config_dir`,
//! `build_empty_session`, `open_shell_window`, `init_components`,
//! `focus_shell_neutrally` are verbatim copies. `t0_ai_nav_gate` deliberately
//! never installs the process-global `MainThreadDispatcher`
//! (`ensure_dispatcher`/`drain_dispatcher` in `keyboard_nav.rs`): with no
//! dispatcher installed, `window_registry::dispatcher()` returns `None` and
//! `window.rs`'s one-shot auto-tour-open silently no-ops (`if let Some(dispatcher)
//! = ... dispatcher.dispatch(...)`), so the first-run auto-show dialog never
//! opens here and there is nothing to flush/close before walking Tab.

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

use dat0_app::ai::Provider;
use dat0_app::ai::panel::AiPanel;
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

/// T0 HARD GATE — four probes in one windowed test. Any failure → STOP/re-scope.
#[gpui::test]
#[serial]
fn t0_ai_nav_gate(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    // Probe 3's console-open path `tokio::spawn`s (`refresh_completion_snapshot`)
    // → needs an ambient tokio runtime for the whole test (motherduck_window.rs
    // routing-chip precedent — entered BEFORE the window opens, like there).
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // Seed the AI dock open with a known draft (bypasses keychain/settings).
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_ai_panel_for_test(AiPanel {
                provider: Some(Provider::Anthropic),
                key_set: false,
                model: String::new(),
                enabled: false,
                advanced_override: false,
                include_sample_rows: false,
                test_result: None,
            });
        });
    });
    vcx.run_until_parked();

    // Probe 1: Tab reaches ai-toggle-enabled; the oracle names it by its label.
    let enabled_label = dat0_i18n::t("ai.enabled.off");
    focus_shell_neutrally(vcx);
    tab_until(vcx, &enabled_label);

    // Probe 2: Enter flips the draft `enabled` flag (operability). The handler
    // also best-effort writes settings.toml → sandboxed by set_config_dir above.
    assert!(
        !shell.update(vcx, |ws, _cx| ws.ai_panel_enabled_for_test()),
        "enabled starts false"
    );
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        shell.update(vcx, |ws, _cx| ws.ai_panel_enabled_for_test()),
        "Enter on ai-toggle-enabled must flip the draft enabled flag (probe 2)"
    );

    // Probe 3: the console chip is Tab-reachable across the shell→console-view
    // boundary. Open the console ready + capture the entity.
    let console = vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.open_console_ready_for_test(window, cx))
    });
    vcx.run_until_parked();
    let chip_label = dat0_i18n::t("sql.nl2sql.chip");
    assert!(
        A11ySnapshot::capture(vcx).has_label(&chip_label),
        "nl2sql-chip a11y twin must render when ai_ready"
    );
    tab_until(vcx, &chip_label);

    // Probe 4: Enter on the focused chip emits OpenNl2SqlPrompt (observed via a
    // subscription BEFORE the shell's own downstream handler runs).
    let events: Rc<RefCell<Vec<SqlConsoleEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let ev2 = events.clone();
    let _sub = vcx.cx.update(|app| {
        app.subscribe(&console, move |_c, ev: &SqlConsoleEvent, _app| {
            ev2.borrow_mut().push(ev.clone());
        })
    });
    vcx.run_until_parked(); // subscription activation is deferred — flush it first
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        events
            .borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::OpenNl2SqlPrompt)),
        "Enter on nl2sql-chip must emit OpenNl2SqlPrompt (probe 4)"
    );

    drop(state);
}

// ─── Task 1: AI-dock reachability suite ─────────────────────────────────────
// Breadth over the dock Task 0 wired: all 7 always-rendered buttons are Tab
// stops in paint order, the conditional `ai-key-forget` joins them when
// `key_set`, and none of them are Tab stops while the dock is closed. These
// tests never open the SQL console, so (per the catalog_nav.rs precedent) no
// ambient tokio runtime is needed.

/// Seed the AI dock open (`seed_ai_panel_for_test` also sets
/// `ai_panel_visible`) with a known draft in the given `key_set` state.
fn seed_ai_dock(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext, key_set: bool) {
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_ai_panel_for_test(AiPanel {
                provider: Some(Provider::Anthropic),
                key_set,
                model: String::new(),
                enabled: false,
                advanced_override: false,
                include_sample_rows: false,
                test_result: None,
            });
        });
    });
    vcx.run_until_parked();
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

/// Local mirror of the PRIVATE `ai/panel.rs::provider_label` so the tests can
/// name the provider button's label. If the format drifts, update BOTH.
fn provider_label_text(p: Option<Provider>) -> String {
    match p {
        Some(p) => format!(
            "{}: {}",
            dat0_i18n::t("ai.provider"),
            dat0_i18n::t(&format!("ai.provider.{}", p.id()))
        ),
        None => dat0_i18n::t("ai.provider.unset"),
    }
}

#[gpui::test]
#[serial]
fn ai_dock_seven_buttons_reachable_in_paint_order(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_ai_dock(&shell, vcx, false);
    focus_shell_neutrally(vcx);

    let want = [
        dat0_i18n::t("ai.enabled.off"),
        provider_label_text(Some(Provider::Anthropic)),
        dat0_i18n::t("ai.key.set_button"),
        dat0_i18n::t("ai.model.set_button"),
        dat0_i18n::t("ai.advanced.off"),
        dat0_i18n::t("ai.sample_rows.off"),
        dat0_i18n::t("ai.test"),
    ];
    let seen = tab_labels(vcx, 40);
    for label in &want {
        assert!(
            seen.contains(label),
            "Tab never reached AI button {label:?}; visited {seen:?}"
        );
    }
    // Order: each expected label appears, and in the paint sequence above.
    let idxs: Vec<usize> = want
        .iter()
        .map(|l| seen.iter().position(|s| s == l).expect("present"))
        .collect();
    assert!(
        idxs.windows(2).all(|w| w[0] < w[1]),
        "AI buttons must be Tab-visited in paint order; got {seen:?}"
    );
    drop(state);
}

#[gpui::test]
#[serial]
fn ai_key_forget_is_reachable_when_key_set(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_ai_dock(&shell, vcx, true); // key_set → ai-key-forget renders
    focus_shell_neutrally(vcx);

    let forget = dat0_i18n::t("ai.key.forget");
    assert!(
        A11ySnapshot::capture(vcx).has_label(&forget),
        "ai-key-forget must render when key_set"
    );
    let seen = tab_labels(vcx, 40);
    assert!(
        seen.contains(&forget),
        "ai-key-forget must be Tab-reachable when key_set; visited {seen:?}"
    );
    drop(state);
}

#[gpui::test]
#[serial]
fn ai_dock_not_a_tab_stop_when_closed(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // Do NOT open the dock (ai_panel_visible stays false).
    focus_shell_neutrally(vcx);
    let enabled_label = dat0_i18n::t("ai.enabled.off");
    let seen = tab_labels(vcx, 40);
    assert!(
        !seen.contains(&enabled_label),
        "no AI button may be a Tab stop while the dock is closed; visited {seen:?}"
    );
    drop(state);
}

// ─── Task 2: SQL-Console AI-trigger suite ───────────────────────────────────
// Breadth over the console triggers: both `nl2sql-chip` (Task 0) and
// `sql-explain` (Task 2) are Tab-reachable when `ai_ready`, and Enter on the
// focused Explain button emits `SqlConsoleEvent::Explain`. The shell's
// downstream `spawn_ai_explain` early-returns on `provider == None` (this
// suite never seeds the AI panel), so the emit is headless-safe — observed via
// an `App::subscribe` that fires before the (no-op) downstream handler.

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

#[gpui::test]
#[serial]
fn console_ai_triggers_reachable(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    // Console-open `tokio::spawn`s (`refresh_completion_snapshot`) → ambient
    // runtime for the whole test, entered BEFORE the window opens (T0 idiom).
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, _log) = open_console_with_log(&shell, vcx);

    let chip = dat0_i18n::t("sql.nl2sql.chip");
    let explain = dat0_i18n::t("sql.explain.button");
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label(&chip), "chip twin renders when ai_ready");
    assert!(
        snap.has_label(&explain),
        "explain twin renders when ai_ready"
    );

    // Establish keyboard focus before walking Tab (nothing is focused in a
    // fresh window; T0 only tabbed after the same neutral click).
    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 40);
    assert!(
        seen.contains(&chip),
        "nl2sql-chip Tab-reachable; visited {seen:?}"
    );
    assert!(
        seen.contains(&explain),
        "sql-explain Tab-reachable; visited {seen:?}"
    );
    drop(state);
}

#[gpui::test]
#[serial]
fn enter_on_explain_emits_explain(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    // Console-open `tokio::spawn`s (`refresh_completion_snapshot`) → ambient
    // runtime for the whole test, entered BEFORE the window opens (T0 idiom).
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, log) = open_console_with_log(&shell, vcx);

    let explain = dat0_i18n::t("sql.explain.button");
    // Establish keyboard focus before walking Tab (nothing is focused in a
    // fresh window; T0 only tabbed after the same neutral click).
    focus_shell_neutrally(vcx);
    tab_until(vcx, &explain);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Explain)),
        "Enter on sql-explain must emit Explain; got {:?}",
        log.borrow()
    );
    drop(state);
}

// ----------------------------------------------------------------------------
// Follow-ups (post-merge review findings).
// ----------------------------------------------------------------------------

/// FF-1 — the self-removing Forget button must HAND OFF focus, not orphan it.
///
/// Activating Forget sets `key_set = false`, which stops rendering the very button
/// that was focused (`ai/panel.rs` gates `ai-key-forget` on `key_set`). The element
/// tracking that focus handle disappears on the next frame, so without an explicit
/// hand-off a keyboard user is left focused on nothing and must Tab from the top
/// again. Assert focus moves to the sibling that survives the removal — "Set key…".
///
/// ★ SAFETY — `provider` is seeded `None` ON PURPOSE. `ForgetKey` wraps its keychain
/// call in `if let Some(provider) = self.ai_panel.provider`, so a `None` provider skips
/// `KeychainKeyStore::forget()` entirely. With `Some(..)` this test would DELETE THE
/// DEVELOPER'S REAL STORED API KEY from the OS keychain (CI Linux runs a live
/// gnome-keyring; macOS uses the login keychain). Nothing is lost by staying hermetic:
/// `key_set = false` and the focus hand-off both live OUTSIDE that guard, so the path
/// under test executes identically.
#[gpui::test]
#[serial]
fn forget_key_hands_focus_to_set_key(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_ai_panel_for_test(AiPanel {
                provider: None, // ★ keychain-safe — see the doc comment above
                key_set: true,  // renders `ai-key-forget` (gated on key_set alone)
                model: String::new(),
                enabled: false,
                advanced_override: false,
                include_sample_rows: false,
                test_result: None,
            });
        });
    });
    vcx.run_until_parked();

    let forget = dat0_i18n::t("ai.key.forget");
    let set_key = dat0_i18n::t("ai.key.set_button");

    focus_shell_neutrally(vcx);
    tab_until(vcx, &forget);

    // Enter activates ForgetKey → the focused button removes itself.
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    // Teeth: the button really did remove itself. Without this, the focus assertion
    // below could pass trivially with the old button still painted.
    assert!(
        !snap.has_label(&forget),
        "ai-key-forget must stop rendering once key_set is false"
    );
    assert_eq!(
        snap.focused_label(),
        Some(set_key.as_str()),
        "focus must be handed to `ai-key-set`, not orphaned on the removed button"
    );
    drop(state);
}

/// T2-m2 — the console AI triggers are tab stops ONLY when operable.
///
/// With `ai_ready = false` the chip and Explain fall into their `else` arm: a plain,
/// non-interactive div carrying neither `focus_stop` nor an `.a11y` twin. So they must
/// surface no a11y node (a bare `.child(text)` is AccessKit-invisible) AND never join
/// the Tab cycle. Mirrors `ai_dock_not_a_tab_stop_when_closed` for the console half.
#[gpui::test]
#[serial]
fn console_ai_triggers_not_tab_stops_when_not_ready(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    // Console-open `tokio::spawn`s (`refresh_completion_snapshot`) → ambient runtime
    // for the whole test, entered BEFORE the window opens (T0 idiom).
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // Console OPEN, but AI NOT ready → chip/Explain render their disabled arm.
    let _console = vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.open_console_for_test(window, cx, false))
    });
    vcx.run_until_parked();

    let chip = dat0_i18n::t("sql.nl2sql.chip");
    let explain = dat0_i18n::t("sql.explain.button");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        !snap.has_label(&chip),
        "no nl2sql-chip a11y twin when !ai_ready"
    );
    assert!(
        !snap.has_label(&explain),
        "no sql-explain a11y twin when !ai_ready"
    );

    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 40);
    assert!(
        !seen.contains(&chip),
        "nl2sql-chip must not be a tab stop when !ai_ready; visited {seen:?}"
    );
    assert!(
        !seen.contains(&explain),
        "sql-explain must not be a tab stop when !ai_ready; visited {seen:?}"
    );
    drop(state);
}
