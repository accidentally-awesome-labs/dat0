//! B7: the left dock — Catalog, Connections and AI as one `DockItem::tabs`,
//! selected by the activity rail.
//!
//! These assert through the a11y capture and through the DOCK's own open flag
//! rather than through the bools the test just wrote: re-reading a bool would
//! prove only that assignment works, not that `sync_left_dock` ran (B6's rule).
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
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::session::Session;
use dat0_app::window::{LeftPanel, WorkspaceShell};
use support::A11ySnapshot;

const BUDGET: u64 = 128 * 1024 * 1024;

fn set_config_dir(dir: &Path) {
    // SAFETY: `#[serial]` — no other thread races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    // The panel registry is prod-only otherwise, and a registration absent
    // under test is silently absent (the `register_modal_keys` lesson).
    cx.update(dat0_app::panels::register_panels);
}

/// A tokio runtime kept alive for the whole test.
///
/// ⚠ B7 made this necessary where `right_dock.rs` needed none: the Catalog arm
/// of `activate_left_panel` calls `refresh_catalog`, which `tokio::spawn`s the
/// off-thread `get_tables` (`window.rs:3263`). Without an ambient runtime that
/// spawn panics with "there is no reactor running". Copied from
/// `tests/ai_nav.rs:95-124`, the per-binary-copy precedent.
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
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

/// Boot a shell over an empty session with a fresh config dir.
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

/// Force one frame so `sync_left_dock` — which runs inside `render` — gets a
/// chance to act on whatever the test just wrote.
fn settle(vcx: &mut VisualTestContext) {
    vcx.update(|window, _app| window.refresh());
    vcx.run_until_parked();
}

/// How many left panels claim to be visible. The B7 invariant is that this is
/// never above 1: two visible would make upstream paint a horizontal tab bar
/// beside the rail (`tab_panel.rs:623-625`) — two selectors for one choice.
fn open_count(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> usize {
    vcx.cx.update(|app| {
        let ws = shell.read(app);
        LeftPanel::ALL
            .iter()
            .filter(|p| ws.left_panel_visible(**p))
            .count()
    })
}

fn activate(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext, target: LeftPanel) {
    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(target, cx));
    });
    settle(vcx);
}

#[gpui::test]
#[serial]
fn activating_a_left_panel_closes_the_others(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    for target in *LeftPanel::ALL {
        activate(&shell, vcx, target);
        assert_eq!(
            open_count(&shell, vcx),
            1,
            "exactly one panel may be open after activating {target:?}"
        );
        assert!(
            vcx.cx
                .update(|app| shell.read(app).left_panel_visible(target)),
            "the activated panel must be the one that is open"
        );
    }
}

#[gpui::test]
#[serial]
fn activating_the_open_panel_collapses_everything(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    activate(&shell, vcx, LeftPanel::Catalog);
    assert_eq!(open_count(&shell, vcx), 1);

    activate(&shell, vcx, LeftPanel::Catalog);
    assert_eq!(
        open_count(&shell, vcx),
        0,
        "activating the panel that is already open collapses it"
    );
    assert_eq!(vcx.cx.update(|app| shell.read(app).open_left_panel()), None);
}

/// The three a11y shims used to write left-panel bools directly, which would
/// violate the invariant one test at a time. They must route through the single
/// writer instead.
#[gpui::test]
#[serial]
fn the_test_shims_respect_the_invariant(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(Vec::new());
            let _ = ws.open_connections_for_test();
        });
    });
    settle(vcx);

    assert_eq!(
        open_count(&shell, vcx),
        1,
        "two shims in a row must not leave two panels visible"
    );
    assert_eq!(
        vcx.cx.update(|app| shell.read(app).open_left_panel()),
        Some(LeftPanel::Connections),
        "the last shim called wins"
    );
}

/// A11y shims that are only compiled under the capture feature get their own
/// test, so the one above still runs in a plain `cargo test -p dat0-app`.
#[cfg(feature = "a11y-capture")]
#[gpui::test]
#[serial]
fn the_ai_shim_respects_the_invariant(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(Vec::new());
            ws.seed_ai_panel_for_test(dat0_app::ai::panel::AiPanel::default());
        });
    });
    settle(vcx);

    assert_eq!(open_count(&shell, vcx), 1);
    assert_eq!(
        vcx.cx.update(|app| shell.read(app).open_left_panel()),
        Some(LeftPanel::Ai)
    );
}

/// Guards the whole point of the invariant: the capture must never show two
/// panel names at once, which is what a tab bar beside the rail would look like.
///
/// ⚠ MEASURED WEAK AT T2, and deliberately kept: the T2 non-vacuity probe (an
/// additive `set_left_panel_exclusive`) turned the other four tests red and left
/// this one GREEN. The reason is that only the catalog contributes a named node
/// today — the Connections and AI panels draw their titles as bare children,
/// which contribute nothing to the capture (A5). It grows teeth at T4, when both
/// titles move into `Panel::title` with an `a11y_label`.
///
/// ✅ RE-PROBED AT T4 and it now fails under the additive writer, as intended.
#[gpui::test]
#[serial]
fn never_two_panel_names_in_the_tree_at_once(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    for target in *LeftPanel::ALL {
        activate(&shell, vcx, target);
        let snap = A11ySnapshot::capture(vcx);
        let named = [
            dat0_app::dat0_i18n::t("catalog.title"),
            dat0_app::dat0_i18n::t("connections.title"),
            dat0_app::dat0_i18n::t("ai.title"),
        ]
        .iter()
        .filter(|n| snap.count_label(n) > 0)
        .count();
        assert!(
            named <= 1,
            "with {target:?} active, {named} panel names are in the tree at once"
        );
    }
}

// ---------------------------------------------------------------------------
// T4: the dock itself
// ---------------------------------------------------------------------------

fn left_dock_open(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> bool {
    vcx.cx
        .update(|app| shell.read(app).left_dock_open_for_test(app))
}

#[gpui::test]
#[serial]
fn left_dock_is_closed_when_every_panel_is_hidden(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    assert!(
        vcx.cx.update(|app| shell.read(app).dock_mounted_for_test()),
        "the DockArea should be built on the first render"
    );
    assert!(
        !left_dock_open(&shell, vcx),
        "a fresh workspace shows no left panel, so the dock must be closed"
    );
}

#[gpui::test]
#[serial]
fn activating_a_panel_opens_the_dock_and_titles_it(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    activate(&shell, vcx, LeftPanel::Connections);

    assert!(
        left_dock_open(&shell, vcx),
        "sync_left_dock must open the dock when a panel becomes visible"
    );
    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.count_label(&dat0_app::dat0_i18n::t("connections.title")),
        1,
        "the dock's title bar names the panel exactly once"
    );
}

#[gpui::test]
#[serial]
fn collapsing_closes_the_dock_again(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    activate(&shell, vcx, LeftPanel::Catalog);
    assert!(left_dock_open(&shell, vcx));

    activate(&shell, vcx, LeftPanel::Catalog);
    assert!(
        !left_dock_open(&shell, vcx),
        "collapsing the last visible panel must close the dock"
    );
}

/// Design §7.4's collision, asserted rather than assumed. `query_by_role` panics
/// on a duplicate match, so a panel title that duplicates a body node would take
/// whole suites down rather than fail one assertion.
#[gpui::test]
#[serial]
fn each_panel_name_resolves_without_a_duplicate(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    for (panel, key) in [
        (LeftPanel::Catalog, "catalog.title"),
        (LeftPanel::Connections, "connections.title"),
        (LeftPanel::Ai, "ai.title"),
    ] {
        activate(&shell, vcx, panel);
        let snap = A11ySnapshot::capture(vcx);
        assert_eq!(
            snap.count_label(&dat0_app::dat0_i18n::t(key)),
            1,
            "{key} must name exactly one node while {panel:?} is open"
        );
    }
}

/// The catalog's own content must still reach the capture THROUGH the dock —
/// the panel is a delegate, so a broken delegation would render an empty dock
/// with a correct title bar and every title assertion above would still pass.
#[gpui::test]
#[serial]
fn catalog_body_content_reaches_the_capture_through_the_dock(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| ws.seed_catalog_tree_for_test(Vec::new()));
    });
    settle(vcx);

    let snap = A11ySnapshot::capture(vcx);
    // `visible_rows`' section headers are rendered by the BODY, not the title
    // bar, so finding one proves the delegation ran.
    assert!(
        snap.count_label("Tables (0)") > 0,
        "the catalog body's section headers must render inside the dock"
    );
}
