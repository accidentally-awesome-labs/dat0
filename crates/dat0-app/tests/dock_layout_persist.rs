//! B9: dock layout persistence — the windowed round-trip.
//!
//! Every assertion reads the layout back off DISK rather than out of the live
//! session. Re-reading a value the test just wrote would prove only that
//! assignment works (B6's rule); going through the file proves the whole chain,
//! including the serializer.
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
use dat0_app::session::dock_layout::{DockLayout, mirror_from_dump};
use dat0_app::window::{LeftPanel, WorkspaceShell};

const BUDGET: u64 = 128 * 1024 * 1024;

fn set_config_dir(dir: &Path) {
    // SAFETY: `#[serial]` — no other thread races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(dat0_app::panels::register_panels);
}

/// A tokio runtime kept alive for the whole test — `toggle_sql_console` reaches
/// `refresh_completion_snapshot`, which `tokio::spawn`s off-thread work.
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    /// ⚠ Holding the runtime is not enough — it must be ENTERED, or
    /// `tokio::spawn` still panics with "there is no reactor running".
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

fn settle(vcx: &mut VisualTestContext) {
    vcx.update(|window, _app| window.refresh());
    vcx.run_until_parked();
}

fn toggle_console(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) {
    vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.toggle_sql_console_for_test(window, cx));
    });
    settle(vcx);
}

/// Read the layout back off DISK — never out of the live session.
fn layout_on_disk(session: &Arc<Mutex<Session>>) -> DockLayout {
    let path = session.lock().home.root_dir().join("session.json");
    let raw = std::fs::read_to_string(path).expect("session.json exists");
    let state = dat0_app::session::migrate::load_str(&raw).expect("session.json parses");
    state.dock_layout.expect("a layout was persisted")
}

/// A booted window over a fresh session — no persisted layout.
fn boot(
    cx: &mut TestAppContext,
) -> (
    Entity<WorkspaceShell>,
    &mut VisualTestContext,
    Arc<Mutex<Session>>,
) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());
    vcx.run_until_parked();
    // The tempdir must outlive the window; leak it rather than let the session
    // read a deleted path mid-test.
    std::mem::forget(tmp);
    (shell, vcx, session)
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

#[gpui::test]
#[serial]
fn activating_a_rail_panel_persists_it(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);

    vcx.update(|_window, app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Ai, cx));
    });
    settle(vcx);

    let layout = layout_on_disk(&session);
    assert_eq!(layout.left_panel, Some(LeftPanel::Ai));
    assert!(layout.left_open());
}

#[gpui::test]
#[serial]
fn switching_rail_panels_replaces_rather_than_accumulates(cx: &mut TestAppContext) {
    // The at-most-one invariant is structural on the wire: `left_panel` is a
    // single Option, so a second activation cannot leave the first one set.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);

    vcx.update(|_window, app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
    });
    settle(vcx);
    vcx.update(|_window, app| {
        shell.update(app, |ws, cx| {
            ws.activate_left_panel(LeftPanel::Connections, cx)
        });
    });
    settle(vcx);

    assert_eq!(
        layout_on_disk(&session).left_panel,
        Some(LeftPanel::Connections)
    );
}

/// ⚠ MEASURABLY WEAKER THAN ITS SIBLINGS, recorded rather than trusted (B7's
/// precedent). Under a non-vacuity probe that made `current_dock_layout` return
/// `DockLayout::default()`, six tests in this file went red and this one stayed
/// green — because "closed" and "default" are the same value, so it cannot tell
/// a working capture from a broken one. It still earns its place as the
/// round-trip of the CLOSE direction; just do not read it as evidence that
/// capture works.
#[gpui::test]
#[serial]
fn closing_the_rail_panel_persists_the_closed_state(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);

    // Activating the open panel again closes it (B7's toggle semantics).
    for _ in 0..2 {
        vcx.update(|_window, app| {
            shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
        });
        settle(vcx);
    }

    let layout = layout_on_disk(&session);
    assert_eq!(layout.left_panel, None);
    assert!(!layout.left_open(), "a closed rail persists as closed");
}

#[gpui::test]
#[serial]
fn opening_the_console_persists_it_with_its_height(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);

    toggle_console(&shell, vcx);

    let layout = layout_on_disk(&session);
    assert!(layout.console_open, "an open console is persisted");
    assert_eq!(
        layout.bottom_size,
        Some(320),
        "the console's height comes from the live dump, so it is known as soon \
         as the dock exists"
    );
}

#[gpui::test]
#[serial]
fn closing_the_console_persists_the_closed_state(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);

    toggle_console(&shell, vcx);
    assert!(layout_on_disk(&session).console_open);
    toggle_console(&shell, vcx);

    assert!(
        !layout_on_disk(&session).console_open,
        "console_open is read from the DOCK, which upstream can also toggle"
    );
}

#[gpui::test]
#[serial]
fn toggling_the_charts_panel_persists_it(cx: &mut TestAppContext) {
    // v11 is the first schema to persist chart visibility at all — v10's `ui`
    // carried only the catalog and inspector bools.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);

    vcx.update(|_window, app| {
        shell.update(app, |ws, cx| ws.toggle_chart_panel_for_test(cx));
    });
    settle(vcx);

    let layout = layout_on_disk(&session);
    assert!(layout.charts_visible);
    assert!(layout.right_open(), "charts alone opens the right dock");
}

#[gpui::test]
#[serial]
fn the_persisted_layout_never_contains_the_centre(cx: &mut TestAppContext) {
    // `DockArea::dump()` carries a `center` whose panel_name is "GridPanel"
    // (B9 T0 probe 2). Restoring it would re-wrap the grid in a TabPanel and
    // bring back the 30px title bar B5 removed, so it must not reach disk.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);
    toggle_console(&shell, vcx);

    let path = session.lock().home.root_dir().join("session.json");
    let raw = std::fs::read_to_string(path).expect("session.json exists");
    assert!(
        !raw.contains("GridPanel") && !raw.contains("\"center\""),
        "session.json must carry docks only, never the centre: {raw}"
    );
}

#[gpui::test]
#[serial]
fn the_captured_sizes_match_the_live_dock(cx: &mut TestAppContext) {
    // Ties the persisted numbers to the dock they came from, rather than to the
    // mount constants — so this keeps holding if a default ever changes.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = boot(cx);
    toggle_console(&shell, vcx);

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    let live = vcx.cx.update(|app| {
        let v = serde_json::to_value(dock.read(app).dump(app)).expect("dump serialises");
        mirror_from_dump(&v)
    });

    let layout = layout_on_disk(&session);
    assert_eq!(
        layout.left_size.map(|s| s as f32),
        live.left_dock.map(|d| d.size)
    );
    assert_eq!(
        layout.right_size.map(|s| s as f32),
        live.right_dock.map(|d| d.size)
    );
    assert_eq!(
        layout.bottom_size.map(|s| s as f32),
        live.bottom_dock.map(|d| d.size)
    );
}

#[gpui::test]
#[serial]
fn a_fresh_session_starts_with_every_dock_closed(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = boot(cx);

    assert_eq!(shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()), None);
    let dock = shell.read_with(&vcx.cx, |ws, _| ws.dock_area_for_test());
    if let Some(dock) = dock {
        assert!(
            !dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Left, app)),
            "no left dock open on a fresh session"
        );
        assert!(
            !dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Right, app)),
            "no right dock open on a fresh session"
        );
    }
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/// Seed a session's layout on disk, then open a shell over that same session —
/// the shape of reopening a workspace or recovering an orphaned session.
fn reopen_with_layout(
    cx: &mut TestAppContext,
    layout: DockLayout,
) -> (
    Entity<WorkspaceShell>,
    &mut VisualTestContext,
    Arc<Mutex<Session>>,
) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    session
        .lock()
        .set_dock_layout(Some(layout))
        .expect("seed the layout");
    let (shell, vcx) = open_shell_window(cx, session.clone());
    vcx.run_until_parked();
    std::mem::forget(tmp);
    (shell, vcx, session)
}

/// The size the live dock actually mounted at, on the given placement.
fn live_size(
    shell: &Entity<WorkspaceShell>,
    vcx: &mut VisualTestContext,
    pick: fn(&dat0_app::session::dock_layout::DumpMirror) -> Option<f32>,
) -> f32 {
    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    vcx.cx.update(|app| {
        let v = serde_json::to_value(dock.read(app).dump(app)).expect("dump serialises");
        pick(&mirror_from_dump(&v)).expect("dock present in the dump")
    })
}

#[gpui::test]
#[serial]
fn a_persisted_rail_panel_comes_back_open(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = reopen_with_layout(
        cx,
        DockLayout {
            left_panel: Some(LeftPanel::Connections),
            ..DockLayout::default()
        },
    );

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    assert!(
        dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Left, app)),
        "the left dock reopened"
    );
    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        Some(LeftPanel::Connections)
    );
}

#[gpui::test]
#[serial]
fn a_persisted_right_dock_comes_back_open(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, session) = reopen_with_layout(
        cx,
        DockLayout {
            inspector_visible: true,
            charts_visible: true,
            ..DockLayout::default()
        },
    );

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    assert!(
        dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Right, app)),
        "the right dock reopened"
    );
    // Close the loop rather than reach for a private getter: capture reads the
    // shell's OWN bools, so re-persisting and re-reading the file proves the
    // restored values actually landed on the shell — not merely that the dock
    // opened, which either panel alone would achieve.
    vcx.update(|_window, app| {
        shell.update(app, |ws, cx| ws.toggle_chart_panel_for_test(cx));
        shell.update(app, |ws, cx| ws.toggle_chart_panel_for_test(cx));
    });
    settle(vcx);
    let round_tripped = layout_on_disk(&session);
    assert!(
        round_tripped.inspector_visible,
        "the restored inspector bool survived a capture round-trip"
    );
    assert!(round_tripped.charts_visible);
}

#[gpui::test]
#[serial]
fn a_persisted_size_is_honoured_at_mount(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = reopen_with_layout(
        cx,
        DockLayout {
            left_panel: Some(LeftPanel::Catalog),
            left_size: Some(500),
            ..DockLayout::default()
        },
    );

    assert_eq!(
        live_size(&shell, vcx, |m| m.left_dock.map(|d| d.size)),
        500.0,
        "the persisted width won over LEFT_DOCK_WIDTH"
    );
}

#[gpui::test]
#[serial]
fn an_absurd_persisted_size_is_clamped_not_obeyed(cx: &mut TestAppContext) {
    // A layout saved on a 4K display, restored on a laptop. Without the clamp
    // this restores a window whose centre is entirely off screen and whose
    // resize handle is unreachable — with no in-app way back.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = reopen_with_layout(
        cx,
        DockLayout {
            left_panel: Some(LeftPanel::Catalog),
            left_size: Some(30_000),
            ..DockLayout::default()
        },
    );

    let size = live_size(&shell, vcx, |m| m.left_dock.map(|d| d.size));
    assert!(
        size < 30_000.0,
        "an oversized dock must be clamped; got {size}"
    );
}

#[gpui::test]
#[serial]
fn no_persisted_layout_means_the_mount_constants(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = boot(cx);
    toggle_console(&shell, vcx);

    assert_eq!(
        live_size(&shell, vcx, |m| m.left_dock.map(|d| d.size)),
        384.0
    );
    assert_eq!(
        live_size(&shell, vcx, |m| m.right_dock.map(|d| d.size)),
        848.0
    );
}

#[gpui::test]
#[serial]
fn a_persisted_open_console_comes_back_at_its_height(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = reopen_with_layout(
        cx,
        DockLayout {
            console_open: true,
            bottom_size: Some(420),
            ..DockLayout::default()
        },
    );

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    assert!(
        dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Bottom, app)),
        "the console dock reopened"
    );
    assert_eq!(
        live_size(&shell, vcx, |m| m.bottom_dock.map(|d| d.size)),
        420.0,
        "the persisted console height was honoured, not SQL_CONSOLE_DOCK_HEIGHT"
    );
}

#[gpui::test]
#[serial]
fn a_restored_console_is_still_toggleable(cx: &mut TestAppContext) {
    // The restore path mounts the console outside toggle_sql_console, so the
    // toggle must still find it built and take the open/close branch rather
    // than trying to mount a second one.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = reopen_with_layout(
        cx,
        DockLayout {
            console_open: true,
            ..DockLayout::default()
        },
    );

    toggle_console(&shell, vcx);
    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    assert!(
        !dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Bottom, app)),
        "the first toggle after a restore CLOSES rather than re-mounting"
    );

    toggle_console(&shell, vcx);
    assert!(
        dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Bottom, app)),
        "and it reopens"
    );
}

#[gpui::test]
#[serial]
fn a_fresh_session_mounts_no_bottom_dock_at_all(cx: &mut TestAppContext) {
    // B8 mounts the bottom dock lazily so a user who never opens the console
    // never sees upstream's 29px collapsed title bar, and so the first-run hero
    // is untouched. Restoring must not cost that.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _session) = boot(cx);

    if let Some(dock) = shell.read_with(&vcx.cx, |ws, _| ws.dock_area_for_test()) {
        assert!(
            !dock.read_with(&vcx.cx, |d, _app| d.has_dock(DockPlacement::Bottom)),
            "no bottom dock exists at all on a fresh session"
        );
    }
}

// ---------------------------------------------------------------------------
// Precedence: session over settings seed
// ---------------------------------------------------------------------------

fn seed_settings(cfg: &Path, layout: DockLayout) {
    std::fs::create_dir_all(cfg).expect("config dir");
    let store = dat0_app::settings::store::SettingsStore::with_path(cfg.join("settings.toml"));
    let mut settings = store.load_or_default().expect("load settings");
    settings.ui.dock_layout = Some(layout);
    store.save(&settings).expect("save settings");
}

#[gpui::test]
#[serial]
fn a_fresh_session_inherits_the_settings_seed(cx: &mut TestAppContext) {
    // A plain launch calls Session::new, which leaves session.json with no
    // layout. Without the seed the layout would never come back outside a
    // workspace or a recovered session.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("cfg");
    set_config_dir(&cfg);
    init_components(cx);
    seed_settings(
        &cfg,
        DockLayout {
            left_panel: Some(LeftPanel::Ai),
            ..DockLayout::default()
        },
    );

    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        Some(LeftPanel::Ai),
        "a brand-new scratch session starts from the last-used layout"
    );
    std::mem::forget(tmp);
}

#[gpui::test]
#[serial]
fn the_session_layout_wins_over_the_settings_seed(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("cfg");
    set_config_dir(&cfg);
    init_components(cx);
    seed_settings(
        &cfg,
        DockLayout {
            left_panel: Some(LeftPanel::Ai),
            ..DockLayout::default()
        },
    );

    let session = build_empty_session(&tmp.path().join("state"));
    session
        .lock()
        .set_dock_layout(Some(DockLayout {
            left_panel: Some(LeftPanel::Catalog),
            ..DockLayout::default()
        }))
        .expect("seed the session");
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        Some(LeftPanel::Catalog),
        "the session is authoritative when it carries a layout of its own"
    );
    std::mem::forget(tmp);
}

#[gpui::test]
#[serial]
fn a_dock_toggle_does_not_write_settings_toml(cx: &mut TestAppContext) {
    // The seed is written ONLY on the close/quit flush. SettingsWatcher re-reads
    // the file on every write, and settings.toml is otherwise written only on
    // deliberate user action -- writing it per toggle would widen the window in
    // which a load-mutate-save clobbers a hand-edit in flight.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("cfg");
    set_config_dir(&cfg);
    init_components(cx);
    seed_settings(&cfg, DockLayout::default());
    let before = std::fs::read_to_string(cfg.join("settings.toml")).expect("settings written");

    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    vcx.update(|_window, app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Ai, cx));
    });
    settle(vcx);

    let after = std::fs::read_to_string(cfg.join("settings.toml")).expect("settings still there");
    assert_eq!(
        before, after,
        "a dock toggle must leave settings.toml untouched"
    );
    std::mem::forget(tmp);
}

// ---------------------------------------------------------------------------
// Restored panels must also run their SIDE EFFECTS
// ---------------------------------------------------------------------------

/// A restored left panel must run the side effects `activate_left_panel`
/// centralises, or it comes back visible-but-empty.
///
/// ⚠ This is the bug the whole-branch pass found and no per-task test could:
/// every other test in this file asserts dock or panel VISIBILITY, and the
/// docks opened perfectly while the AI panel behind one of them was never
/// hydrated. B7 folded these side effects into `activate_left_panel` so no
/// entry point could lose them; B9's restore is a second entry point that
/// seeds the visibility bools directly and skipped every one.
///
/// ⚠⚠ SAFETY: the seeded AI settings deliberately set NO provider.
/// `hydrate_ai_panel` only reaches the OS keychain when a provider is set, and
/// a test must never drive the real keychain (the standing rule from the
/// AI-config slice, where an Enter press would have DELETED the developer's
/// stored API key).
#[gpui::test]
#[serial]
fn a_restored_ai_panel_is_hydrated_not_merely_visible(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    set_config_dir(&cfg);
    init_components(cx);

    // `enabled = true` with no provider: hydration is observable, the keychain
    // is not touched.
    let store = dat0_app::settings::store::SettingsStore::with_path(cfg.join("settings.toml"));
    let mut settings = store.load_or_default().unwrap();
    settings.ai.enabled = true;
    settings.ai.provider = None;
    settings.ui.dock_layout = Some(DockLayout {
        left_panel: Some(LeftPanel::Ai),
        ..DockLayout::default()
    });
    store.save(&settings).unwrap();

    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        Some(LeftPanel::Ai),
        "precondition: the AI panel restored visible"
    );
    assert!(
        shell.read_with(&vcx.cx, |ws, _| ws.ai_panel_enabled_for_test()),
        "a restored AI panel must be HYDRATED from settings, not left at its \
         default -- visible-but-empty is the failure this guards"
    );
    std::mem::forget(tmp);
}
