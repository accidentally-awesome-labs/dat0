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
use dat0_app::view::sql_console::SqlConsoleEvent;
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
