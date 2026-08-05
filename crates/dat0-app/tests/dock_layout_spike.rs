//! B9 T0 — the dock-layout hard gate.
//!
//! Three probes that must pass before any production edit:
//!
//! 1. `DockArea::dump()` reads the LIVE `Dock`, not a construction-time copy.
//! 2. The serialized dump really carries `{size, open}` per placement, which is
//!    the shape B9's capture path parses.
//! 3. Mounting the bottom dock before the first frame settles — the shape the
//!    console restore needs — neither panics nor moves the a11y node count.
//!
//! These live in a spike file rather than in `window.rs`: B8 found that an
//! in-production probe reddens unrelated `a11y_content` tests as an artifact and
//! makes its own measurements unreadable. They stay after B9 as standing
//! regression guards, like `dock_chrome_spike.rs`.
//!
//! ⚠ What these probes deliberately do NOT prove: that a real mouse-drag resize
//! is persisted. `Dock::resize` is reachable only through `resize_handle`'s drag
//! (`dock/dock.rs:291-305`), and dat0 cannot obtain the `Entity<Dock>` to call
//! the public `set_size` — `DockArea` keeps all three private with no getter.
//! What covers that gap is structural: `Dock::resize` mutates the same
//! `self.size` field `DockState::new` reads (`dock/state.rs:34-43`), and probe 1
//! proves the dump reflects that field's live value. Writing a probe that
//! pretended to drive a drag would be worse than naming the gap — B7 recorded
//! two consecutive probes that "passed" while measuring nothing, and the more
//! convincing one was the more wrong.
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

const BUDGET: u64 = 128 * 1024 * 1024;

/// The three mount constants, mirrored from `window.rs`. Probe 1 leans on them
/// being DISTINCT: three different numbers cannot all come from one shared
/// constant, so reading them back proves the dump resolves each dock separately.
const LEFT_DOCK_WIDTH: f32 = 384.0;
const RIGHT_DOCK_WIDTH: f32 = 288.0 + 560.0;
const SQL_CONSOLE_DOCK_HEIGHT: f32 = 320.0;

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
/// spawn panics with "there is no reactor running".
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    /// ⚠ Holding the runtime is not enough — it must be ENTERED, or
    /// `tokio::spawn` still panics. Every test binds the guard, not just the
    /// harness.
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

/// Serialize the live `DockArea::dump()`.
fn dump_json(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> serde_json::Value {
    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    vcx.cx
        .update(|app| serde_json::to_value(dock.read(app).dump(app)).expect("dump serialises"))
}

// ---------------------------------------------------------------------------
// Probe 1 — dump() reads the live Dock
// ---------------------------------------------------------------------------

/// `DockArea::dump()` reports each dock's own size, resolved from the live
/// `Dock` rather than echoed from a shared constant.
///
/// The three mount sizes are deliberately distinct (384 / 848 / 320), so
/// reading all three back is a stronger claim than any single value could be:
/// one shared constant cannot produce three different numbers.
#[gpui::test]
#[serial]
fn dump_reports_each_docks_own_mount_size(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx) = boot(cx);

    // The bottom dock is mounted lazily on first console open (B8).
    toggle(&shell, vcx);

    let json = dump_json(&shell, vcx);
    eprintln!("PROBE1 dump = {json:#}");

    let size_of = |placement: &str| -> f64 {
        json.get(placement)
            .unwrap_or_else(|| panic!("{placement} present in the dump: {json:#}"))
            .get("size")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_else(|| panic!("{placement}.size is a bare number: {json:#}"))
    };

    assert_eq!(size_of("left_dock"), f64::from(LEFT_DOCK_WIDTH));
    assert_eq!(size_of("right_dock"), f64::from(RIGHT_DOCK_WIDTH));
    assert_eq!(size_of("bottom_dock"), f64::from(SQL_CONSOLE_DOCK_HEIGHT));
}

// ---------------------------------------------------------------------------
// Probe 2 — the JSON shape B9's capture path parses
// ---------------------------------------------------------------------------

/// Every placement carries `size` as a bare number and `open` as a bool.
///
/// `Pixels` is `#[repr(transparent)]` over `f32` with a derived `Serialize`
/// (`gpui-0.2.2/src/geometry.rs:2565-2573`), which is why `size` is not a
/// nested object. If that ever changes, B9's mirror struct changes with it.
#[gpui::test]
#[serial]
fn dump_json_carries_size_and_open_per_dock(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx) = boot(cx);
    toggle(&shell, vcx);

    let json = dump_json(&shell, vcx);

    for placement in ["left_dock", "right_dock", "bottom_dock"] {
        let slot = json
            .get(placement)
            .unwrap_or_else(|| panic!("{placement} present in the dump: {json:#}"));
        assert!(
            slot.get("size")
                .and_then(serde_json::Value::as_f64)
                .is_some(),
            "{placement}.size must be a bare number: {slot:#}"
        );
        assert!(
            slot.get("open")
                .and_then(serde_json::Value::as_bool)
                .is_some(),
            "{placement}.open must be a bool: {slot:#}"
        );
    }

    // The dump also carries a `center`. B9 must never restore it — a
    // `PanelInfo::Panel` comes back as `DockItem::tabs` (`dock/state.rs:227-236`)
    // and regains the 30px title bar. Asserting it is PRESENT here is what makes
    // the capture path's job clear: the centre exists in the dump and is
    // discarded on purpose, rather than being absent by luck.
    assert!(
        json.get("center").is_some(),
        "the dump carries a centre that B9 deliberately drops: {json:#}"
    );
}

// ---------------------------------------------------------------------------
// Probe 3 — mounting the bottom dock before the first frame settles
// ---------------------------------------------------------------------------

/// The console can be mounted before the first frame has settled — the earliest
/// moment the restore path could run — without tripping B7's construction-time
/// re-entrancy panic ("cannot read WorkspaceShell while it is already being
/// updated").
#[gpui::test]
#[serial]
fn console_mounts_before_the_first_frame_settles(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    // NO run_until_parked first — that is the whole point of this probe.
    vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.toggle_sql_console_for_test(window, cx));
    });
    settle(vcx);

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    assert!(
        dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Bottom, app)),
        "the bottom dock is open after an early mount"
    );
    std::mem::forget(tmp);
}
