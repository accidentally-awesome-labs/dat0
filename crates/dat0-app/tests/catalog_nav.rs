//! UAT "catalog-tree" slice — T0 HARD GATE + the Task-6 behavioral suite.
//!
//! `t0_catalog_tab_and_arrow` is the load-bearing spike. It proves, in ONE
//! windowed `#[gpui::test]`, the risks the whole slice rests on:
//!   R6: Tab reaches the catalog container's `focus_stop` (the oracle names it
//!       by its label text);
//!   R1: a chained `on_key_down` (pushed after `focus_stop`'s own) receives
//!       Down under `TestPlatform` and moves the active-index — re-proven for
//!       THIS surface (recents R1 precedent);
//!   plus the container's a11y twin renders when the panel is visible.
//!
//! Mount scaffolding below is COPIED per-binary from `tests/recents_nav.rs`
//! (this crate's per-binary-copy precedent) — `set_config_dir`,
//! `build_empty_session`, `open_shell_window`, `init_components`,
//! `focus_shell_neutrally` are verbatim copies. `t0_catalog_tab_and_arrow`
//! deliberately never installs the process-global `MainThreadDispatcher`
//! (`ensure_dispatcher`/`drain_dispatcher` in `keyboard_nav.rs`): with no
//! dispatcher installed, `window_registry::dispatcher()` returns `None` and
//! `window.rs`'s one-shot auto-tour-open silently no-ops (`if let Some(dispatcher)
//! = ... dispatcher.dispatch(...)`), so the first-run auto-show dialog never
//! opens here and there is nothing to flush/close before walking Tab.

mod support;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
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

/// A fake `md:`-origin table (alias "sample_data") and a sqlite-attached table
/// (alias "sq") — copied shapes from tests/motherduck_window.rs:104-126.
fn md_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::Attached {
            alias: "sample_data".into(),
            source: "md:sample_data".into(),
        },
    }
}
fn sqlite_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::Attached {
            alias: "sq".into(),
            source: "/tmp/x.db".into(),
        },
    }
}
fn file_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::File(std::path::PathBuf::from("/data/local.csv")),
    }
}

/// Seed the depth-2 tree every Task-6 test walks (3 top-level nodes → 6 visible
/// rows expanded).
///
/// Visible rows, paint order (sections: Sources sorted [local_sales, sq],
/// Cloud, Tables, Derived):
///   0 L0:local_sales   (Sources — file leaf; "local_sales" < "sq")
///   1 P :sq            (Sources — sqlite attach parent)
///   2 L1:alpha
///   3 L1:zeta
///   4 P :sample_data   (Cloud — md attach parent)
///   5 L1:md_events
fn seed_tree(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) {
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(vec![
                sqlite_tbl("alpha"),
                sqlite_tbl("zeta"),
                md_tbl("md_events"),
                file_tbl("local_sales"),
            ]);
        });
    });
    vcx.run_until_parked();
}

/// Tab from the neutral shell focus until the catalog container is the focused
/// stop, or panic after a bounded number of hops (recents_nav.rs:137 idiom;
/// the hero paints several stops before the dock, hence the larger bound).
fn tab_to_catalog(cx: &mut VisualTestContext) {
    let want = dat0_i18n::t("catalog.title");
    for _ in 0..20 {
        press_tab(cx);
        let snap = A11ySnapshot::capture(cx);
        if snap.focused_label() == Some(want.as_str()) {
            return;
        }
    }
    panic!("catalog container was never the focused Tab stop within 20 hops");
}

/// T0 HARD GATE. Seeds a flat 2-table catalog, Tabs to the container (R6 +
/// oracle twin), presses Down and asserts the active-index moved (R1: the
/// chained on_key_down receives arrows on THIS surface). If the Down assertion
/// fails, STOP — switch to the single-unified-on_key_down fallback (design R1).
#[gpui::test]
#[serial]
fn t0_catalog_tab_and_arrow(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(vec![md_tbl("md_events"), file_tbl("local_sales")]);
        });
    });
    vcx.run_until_parked();

    // The container's oracle twin renders (label = the panel title).
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label(&dat0_i18n::t("catalog.title")),
        "catalog container a11y twin must render when the panel is visible"
    );

    // R6: Tab reaches the container; the oracle names it by its label text.
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    // R1: Down moves the active-index via the chained on_key_down.
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        0,
        "active-index starts at 0"
    );
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        1,
        "Down must move the catalog active-index to 1 (R1 hard gate)"
    );

    drop(state);
}

// ----------------------------------------------------------------------------
// Task 6 — the behavioral UAT suite over the seeded depth-2 tree.
// ----------------------------------------------------------------------------

#[gpui::test]
#[serial]
fn arrows_walk_visible_rows_and_clamp(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    let active = |vcx: &mut VisualTestContext, shell: &Entity<WorkspaceShell>| {
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test())
    };
    assert_eq!(active(vcx, &shell), 0);
    vcx.simulate_keystrokes("up"); // clamp at top
    vcx.run_until_parked();
    assert_eq!(active(vcx, &shell), 0, "Up at row 0 clamps");
    for _ in 0..7 {
        vcx.simulate_keystrokes("down"); // 6 rows: 5 moves + 2 clamped
    }
    vcx.run_until_parked();
    assert_eq!(
        active(vcx, &shell),
        5,
        "Down clamps at the last visible row"
    );
    drop(state);
}

#[gpui::test]
#[serial]
fn left_jumps_to_parent_then_collapses_children_vanish(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    // Walk to row 3 (child "zeta" of parent "sq").
    for _ in 0..3 {
        vcx.simulate_keystrokes("down");
    }
    vcx.run_until_parked();
    assert_eq!(shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()), 3);

    // ← on a child jumps to ITS parent (row 1), not any earlier row.
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        1,
        "Left on a child moves to its parent"
    );

    // Expanded children are painted before the collapse…
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("alpha"), "child renders while expanded");
    assert!(snap.has_label("zeta"));

    // ← on the (expanded) parent collapses it: children VANISH from the a11y
    // tree (absence teeth — render-conditioned seams, R2).
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        !snap.has_label("alpha") && !snap.has_label("zeta"),
        "collapsed children must not render"
    );
    assert!(
        snap.has_label("md_events"),
        "the OTHER parent's children are untouched"
    );
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_collapsed_for_test()),
        vec!["sq".to_string()]
    );

    // → on the collapsed parent re-expands; children return.
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("alpha"), "right re-expands the parent");
    assert!(
        shell
            .update(vcx, |ws, _cx| ws.catalog_collapsed_for_test())
            .is_empty()
    );
    drop(state);
}

#[gpui::test]
#[serial]
fn enter_on_parent_toggles_collapse(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    vcx.simulate_keystrokes("down"); // row 1 = parent "sq"
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter"); // focus_stop activate → Toggle
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_collapsed_for_test()),
        vec!["sq".to_string()],
        "Enter on an expanded parent collapses it"
    );
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        !snap.has_label("alpha"),
        "children vanish on Enter-collapse"
    );
    drop(state);
}

/// Locate the session.json `Session::new` created under `state_root`. It lives
/// at `state_root/scratch/<uuid>/session.json` (session/mod.rs:228 joins
/// "scratch" + a fresh UUID), so scan the `scratch/` children.
fn find_session_json(state_root: &Path) -> PathBuf {
    let scratch = state_root.join("scratch");
    for entry in std::fs::read_dir(&scratch).expect("read scratch root") {
        let p = entry.expect("dir entry").path();
        if p.is_dir() && p.join("session.json").exists() {
            return p.join("session.json");
        }
    }
    panic!("no session.json under {scratch:?}");
}

#[gpui::test]
#[serial]
fn collapse_state_persists_to_session_json(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    // Toggle via the production single-source method `toggle_catalog_parent`
    // (mouse + kbd both route here); it calls persist_dock_ui → session.json.
    // It is `pub(crate)` (invisible to an integration test), so drive it
    // through the keyboard path: Down×4 → row 4 (parent "sample_data"), Enter.
    for _ in 0..4 {
        vcx.simulate_keystrokes("down");
    }
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_collapsed_for_test()),
        vec!["sample_data".to_string()],
        "Enter on the Cloud parent must collapse it before we probe the disk"
    );

    let raw = std::fs::read_to_string(find_session_json(state.path())).expect("session.json");
    assert!(
        raw.contains(r#""catalog_collapsed""#) && raw.contains(r#""sample_data""#),
        "collapsed alias must be persisted in the session ui block; got: {raw}"
    );
    drop(state);
}

// R5 probe OUTCOME (run once, 2026-07-11): `enter_on_leaf_reaches_open_table_tab_gracefully`
// PANICKED — Enter on a leaf routes to `open_table_tab`, whose `tokio::spawn`
// (window.rs:2946) aborts with "there is no reactor running" because the test
// holds no entered tokio runtime (production enters one for the whole
// `Application::run` closure; `build_empty_session`'s runtime is dropped on
// return). Per the sanctioned brief outcome the test is DELETED: Enter-on-leaf
// stays human; the `Open` arm is unit-covered in catalog/nav.rs
// (`enter_toggles_parents_and_opens_leaves`) — recents precedent.

/// The catalog container is a tab stop ONLY while the panel is visible (the
/// focus_stop lives inside the `catalog_panel_visible.then(..)` render branch).
/// With no seed (panel hidden), a bounded Tab walk must never land on it —
/// this guards the hero/settings Tab sequences other suites assert.
#[gpui::test]
#[serial]
fn hidden_panel_is_not_a_tab_stop(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // NO seed_catalog_tree_for_test → catalog_panel_visible stays false.
    focus_shell_neutrally(vcx);

    let title = dat0_i18n::t("catalog.title");
    for _ in 0..20 {
        press_tab(vcx);
        let snap = A11ySnapshot::capture(vcx);
        assert_ne!(
            snap.focused_label(),
            Some(title.as_str()),
            "hidden catalog panel must not be Tab-reachable"
        );
    }
    drop(state);
}
