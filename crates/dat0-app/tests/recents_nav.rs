//! UAT "recents-list keyboard nav" slice — Task 0 HARD GATE.
//!
//! This is the load-bearing spike. It proves, in ONE windowed `#[gpui::test]`,
//! the risks the whole slice rests on:
//!   R4: Tab reaches the recents-list `focus_stop` (the oracle names it by its
//!       label text);
//!   R1: a chained `on_key_down` (pushed after `focus_stop`'s own) receives
//!       Down under `TestPlatform` and moves the active-index;
//!   R2: a seeded `recents.json` renders the rows.
//!
//! Mount scaffolding below is COPIED per-binary from `tests/keyboard_nav.rs`
//! (this crate's per-binary-copy precedent) — `set_config_dir`,
//! `build_empty_session`, `open_shell_window`, `init_components`,
//! `focus_shell_neutrally` are verbatim copies. `t0_recents_list_tab_and_arrow`
//! deliberately never installs the process-global `MainThreadDispatcher`
//! (`ensure_dispatcher`/`drain_dispatcher` in `keyboard_nav.rs`): with no
//! dispatcher installed, `window_registry::dispatcher()` returns `None` and
//! `window.rs`'s one-shot auto-tour-open silently no-ops (`if let Some(dispatcher)
//! = ... dispatcher.dispatch(...)`), so the first-run auto-show dialog never
//! opens here and there is nothing to flush/close before walking Tab.

mod support;

use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;

use support::{A11ySnapshot, press_tab};

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

// ----------------------------------------------------------------------------
// T0 — full production nav + the load-bearing assertions (HARD GATE).
// ----------------------------------------------------------------------------

/// Seed a real `recents.json` under the (serial) test's config dir with `n`
/// Package entries. `push` inserts most-recent-first, so pushing p0 then p1
/// yields on-disk order [p1, p0]; we push in REVERSE so row 0 == "/recent/0".
fn seed_recents(cfg: &Path, n: usize) {
    let mut r = dat0_app::recents::Recents::with_path(cfg.join("recents.json"));
    for i in (0..n).rev() {
        r.push(dat0_app::recents::RecentEntry::Package {
            path: PathBuf::from(format!("/recent/{i}")),
        })
        .expect("push recent");
    }
}

/// Tab from the neutral shell focus until the recents list is the focused stop,
/// or panic after a bounded number of hops (mirrors keyboard_nav.rs:278).
fn tab_to_recents_list(cx: &mut VisualTestContext) {
    let want = dat0_i18n::t("hero.recent_label");
    for _ in 0..12 {
        press_tab(cx);
        let snap = A11ySnapshot::capture(cx);
        if snap.focused_label() == Some(want.as_str()) {
            return;
        }
    }
    panic!("recents list was never the focused Tab stop within 12 hops");
}

/// T0 HARD GATE. Seeds 2 recents, mounts the shell, Tabs to the list (R4 +
/// oracle), then presses Down and asserts the active-index moved (R1: the
/// chained `on_key_down` receives the arrow under TestPlatform). If the Down
/// assertion fails, STOP — switch to a single unified `on_key_down`.
#[gpui::test]
#[serial]
fn t0_recents_list_tab_and_arrow(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    seed_recents(cfg.path(), 2);

    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // R2: the seeded rows render (recents_empty == false → recents_column runs).
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label(&dat0_i18n::t("hero.recent_label")),
        "seeded recents list header must render"
    );

    // R4: Tab reaches the list; the oracle names it by its label text.
    focus_shell_neutrally(vcx);
    tab_to_recents_list(vcx);

    // R1: Down moves the active-index via the chained on_key_down.
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.recents_active_for_test()),
        0,
        "active-index starts at 0"
    );
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.recents_active_for_test()),
        1,
        "Down must move the recents active-index to 1 (R1 gate)"
    );

    drop(state);
}

/// Arrow nav moves the active-index and clamps at both ends. Seeds 2 recents
/// (indices 0..=1); Up at 0 stays 0, Down past the last row stays at len-1.
#[gpui::test]
#[serial]
fn recents_arrows_move_and_clamp(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    seed_recents(cfg.path(), 2);

    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);
    tab_to_recents_list(vcx);

    // Up at the top is a no-op (saturating).
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.recents_active_for_test()),
        0,
        "Up at index 0 clamps to 0"
    );

    // Down moves to the last row, then clamps there.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.recents_active_for_test()),
        1,
        "Down moves to index 1"
    );
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.recents_active_for_test()),
        1,
        "Down at the last row clamps to len-1"
    );

    // Up returns toward the top.
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.recents_active_for_test()),
        0,
        "Up returns to index 0"
    );

    drop(state);
}
