//! B8: the SQL console as the bottom dock.
//!
//! These assert through the DOCK's own open flag and through the a11y capture,
//! never by re-reading a value the test just wrote — re-reading would prove
//! only that assignment works (B6's rule).
//!
//! The console's visibility has no shell bool left to re-read in any case: B8
//! deleted it and derived `sql_console_visible` from
//! `DockArea::is_dock_open(Bottom)`, precisely because upstream owns two toggle
//! paths dat0 does not.
//!
//! Hermeticity: `DAT0_CONFIG_DIR` points at a fresh temp dir; `#[serial]`
//! because both `set_var` and the capture collector are process-global.

mod support;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use gpui_component::dock::DockPlacement;
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;
use support::A11ySnapshot;

const BUDGET: u64 = 128 * 1024 * 1024;

fn set_config_dir(dir: &Path) {
    // SAFETY: `#[serial]` — no other thread races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    // Prod-only otherwise, and a registration absent under test is silently
    // absent (the `register_modal_keys` lesson from B1/B2).
    cx.update(dat0_app::panels::register_panels);
}

/// A tokio runtime kept alive for the whole test.
///
/// `toggle_sql_console` reaches `refresh_completion_snapshot`, which
/// `tokio::spawn`s the off-thread `get_tables`; without an ambient runtime that
/// spawn panics with "there is no reactor running". Same reason `left_dock.rs`
/// needs one and `right_dock.rs` does not.
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    /// ⚠ Holding the runtime is not enough — it must be ENTERED, or
    /// `tokio::spawn` still panics with "there is no reactor running". Every
    /// test below binds the guard, not just the harness.
    fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.rt.enter()
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

/// Open a real window whose root is a `Root` wrapping a live `WorkspaceShell`,
/// mirroring production (`window.rs::open_window_view`).
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

fn boot(cx: &mut TestAppContext) -> (Entity<WorkspaceShell>, &mut VisualTestContext) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // The tempdir must outlive the window; leak it rather than let the session
    // read a deleted path mid-test.
    std::mem::forget(tmp);
    (shell, vcx)
}

fn settle(vcx: &mut VisualTestContext) {
    vcx.update(|window, _app| window.refresh());
    vcx.run_until_parked();
}

/// Toggle through the SHELL — the ⌘⇧C / menu / palette path.
fn toggle(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) {
    vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.toggle_sql_console_for_test(window, cx));
    });
    settle(vcx);
}

/// Toggle through the DOCK — byte-for-byte what upstream's title-bar chevron
/// and its click-a-tab-while-collapsed handler call (`tab_panel.rs:746-751`).
/// This is the writer dat0 does not own.
fn toggle_externally(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) {
    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists once the console has been opened");
    vcx.update(|window, app| {
        dock.update(app, |d, cx| {
            d.toggle_dock(DockPlacement::Bottom, window, cx);
        });
    });
    settle(vcx);
}

fn is_open(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> bool {
    vcx.cx
        .update(|app| shell.read(app).bottom_dock_open_for_test(app))
}

fn has_dock(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> bool {
    vcx.cx.update(|app| {
        shell
            .read(app)
            .dock_area_for_test()
            .is_some_and(|d| d.read(app).has_dock(DockPlacement::Bottom))
    })
}

/// The bottom dock does not exist until the console is first opened.
///
/// This is the whole reason B8 mounts lazily instead of alongside the left and
/// right docks: upstream keeps a CLOSED bottom dock on screen at `h(px(29.))`
/// so its title bar stays clickable (`dock.rs:372-380`). Mounting eagerly would
/// put that bar under the first-run hero for every user who never opens a SQL
/// console at all.
#[gpui::test]
#[serial]
fn no_bottom_dock_until_the_console_is_first_opened(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    assert!(
        !has_dock(&shell, vcx),
        "a freshly booted shell must have no bottom dock at all"
    );
    assert!(
        !is_open(&shell, vcx),
        "and it must therefore report the console as not visible"
    );

    toggle(&shell, vcx);

    assert!(has_dock(&shell, vcx), "the first toggle mounts the dock");
    assert!(is_open(&shell, vcx), "and opens it");
}

/// A second toggle closes it, and the derived getter follows the DOCK.
#[gpui::test]
#[serial]
fn toggling_twice_closes_the_dock_and_visibility_follows(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    toggle(&shell, vcx);
    assert!(is_open(&shell, vcx), "open after one toggle");

    toggle(&shell, vcx);
    assert!(!is_open(&shell, vcx), "closed after two");

    toggle(&shell, vcx);
    assert!(is_open(&shell, vcx), "open again after three");
}

/// ⚠ The desync test, and the reason `sql_console_visible` is derived at all.
///
/// Upstream's chevron flips `Dock::open` behind the shell's back. With a cached
/// bool the shell would still believe the console was open, and the NEXT
/// ⌘⇧C would toggle that stale value — closing an already-closed dock, i.e.
/// reopening it, exactly backwards from what the user asked for.
#[gpui::test]
#[serial]
fn an_external_toggle_does_not_reverse_the_next_shell_toggle(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    toggle(&shell, vcx);
    assert!(is_open(&shell, vcx), "seed: open");

    // The chevron.
    toggle_externally(&shell, vcx);
    assert!(
        !is_open(&shell, vcx),
        "the derived getter must track a toggle dat0 did not perform"
    );

    // Now the user presses the shortcut. It must OPEN, not close-again.
    toggle(&shell, vcx);
    assert!(
        is_open(&shell, vcx),
        "after an external close, the next shell toggle must OPEN — a cached \
         bool would have gone the other way"
    );
}

/// `set_bottom_dock` leaks subscriptions per call (`dock/mod.rs:955-963`), so
/// it must run exactly once no matter how often the console is toggled. A
/// regression here is unbounded growth rather than a wrong pixel, which is why
/// it gets a test of its own: the dock entity's identity must never change.
#[gpui::test]
#[serial]
fn repeated_toggles_never_remount_the_dock(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    toggle(&shell, vcx);
    let first = vcx.cx.update(|app| {
        shell
            .read(app)
            .dock_area_for_test()
            .map(|d| d.entity_id())
            .expect("dock")
    });

    for _ in 0..4 {
        toggle(&shell, vcx);
    }

    let after = vcx.cx.update(|app| {
        shell
            .read(app)
            .dock_area_for_test()
            .map(|d| d.entity_id())
            .expect("dock")
    });
    assert_eq!(
        first, after,
        "the DockArea must be the same entity after repeated toggles"
    );
    assert!(
        has_dock(&shell, vcx),
        "and the bottom dock must still be attached"
    );
}

/// The console's controls stay reachable and uniquely named inside the dock.
///
/// ⚠ This is the regression guard for B8's worst bug. `TabPanel` renders
/// `track_focus(&self.focus_handle(cx))` and delegates that to the ACTIVE
/// PANEL's handle, so returning any handle that is also a live stop inside the
/// console registers one `FocusId` twice in a frame — once outside the
/// `.tab_group()` and once inside — and EVERY console focus stop silently
/// leaves the Tab ring. `SqlConsole::focus_handle` returns a dedicated,
/// non-tab-stop root handle for exactly this reason.
///
/// ⚠⚠ This MUST walk Tab, not count nodes. The first version of this test
/// counted a11y nodes and was measurably VACUOUS: reverting `focus_handle` to
/// the editor handle reddened eight tests in `sql_console_nav` while all seven
/// tests here stayed green. Node presence was never what the bug destroyed —
/// the nodes are captured either way; it is the TAB RING that loses them.
#[gpui::test]
#[serial]
fn console_controls_stay_tab_reachable_inside_the_dock(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    toggle(&shell, vcx);

    // Stage focus in the shell — with nothing focused the dispatch path is the
    // window root alone and Tab is completely inert (B1), so a walk that skips
    // this measures nothing.
    let tt = vcx
        .debug_bounds("hero-take-tour")
        .expect("hero-take-tour painted");
    vcx.simulate_click(
        gpui::point(tt.origin.x - gpui::px(60.), tt.center().y),
        gpui::Modifiers::none(),
    );
    vcx.run_until_parked();

    let run = dat0_i18n::t("sql.run");
    let mut reached = false;
    for _ in 0..60 {
        support::press_tab(vcx);
        if A11ySnapshot::capture(vcx).focused_label() == Some(run.as_str()) {
            reached = true;
            break;
        }
    }
    assert!(
        reached,
        "`{run}` must be reachable by Tab across the shell → dock → console \
         boundary; if this fails, check that SqlConsole::focus_handle still \
         returns the dedicated root handle and not a live stop"
    );

    // A duplicate accessible name would make `query_by_role` PANIC on a
    // duplicate match (`tests/support/mod.rs:139`), taking whole suites down
    // rather than failing one assertion.
    assert_eq!(
        A11ySnapshot::capture(vcx).count_label(&run),
        1,
        "`{run}` must appear exactly once in the docked console"
    );
}

/// Closing the dock takes the console's nodes out of the tree entirely.
///
/// Measured at T0 against a synthetic panel (a collapsed bottom dock
/// contributes 0 nodes); this is the same claim for the real console, and it
/// is what keeps a collapsed console from leaving ~18 phantom controls behind
/// for a screen reader.
#[gpui::test]
#[serial]
fn a_closed_console_leaves_no_controls_in_the_tree(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);
    let run = dat0_i18n::t("sql.run");

    toggle(&shell, vcx);
    assert_eq!(
        A11ySnapshot::capture(vcx).count_label(&run),
        1,
        "seed: Run is present while open"
    );

    toggle(&shell, vcx);
    assert_eq!(
        A11ySnapshot::capture(vcx).count_label(&run),
        0,
        "a collapsed bottom dock must not leave the console's controls in the \
         accessibility tree"
    );
}

/// B9 will resolve this name through the global `PanelRegistry`.
#[gpui::test]
#[serial]
fn the_console_panel_is_registered_under_its_frozen_name(cx: &mut TestAppContext) {
    init_components(cx);
    assert_eq!(
        dat0_app::view::sql_console::SqlConsole::PANEL_NAME,
        "SqlConsolePanel"
    );
}
