//! UAT "MotherDuck UI" slice (Slice 5). Content + state-display coverage for the
//! three P9b surfaces: the Catalog "Cloud" group, the Test-connection result
//! states, and the SQL-console routing chip. Click-free inject-and-assert — no
//! live token/keychain/engine. Production seams chain `.a11y_label` onto existing
//! elements, so release markup is byte-identical (no owed human glance).
//!
//! Harness helpers below are COPIED verbatim from `tests/chart_uat_window.rs`
//! (which copied them from `tests/onboarding_gpui.rs`) — per-binary copy, matching
//! that precedent; NOT centralized.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;
use std::cell::RefCell;
use std::rc::Rc;

use support::A11ySnapshot;

use dat0_app::connections::ConnectionStatus;
use dat0_app::connections::routing::Routing;
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
/// runtime (`Session::new` is async + uses `spawn_blocking`).
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

/// Initialise the gpui-component theme global — required before any gpui-component
/// widget renders.
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
}

/// A tokio runtime kept alive for the whole test so foreground-polled `cx.spawn`
/// futures can call `tokio::task::spawn_blocking`. Needed by the routing-chip test
/// because `toggle_sql_console` may refresh `md_databases` off the engine on open.
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

/// A fake `md:`-origin table (→ Catalog "Cloud" group via the `tree.rs` classifier).
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

/// A fake local `File`-origin table (→ Catalog "Sources" group, never "Cloud").
fn file_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::File(std::path::PathBuf::from("/data/local.csv")),
    }
}

#[gpui::test]
#[serial]
fn cloud_group_renders_md_table_not_file(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    // One md-attached table + one local file table → the classifier must route
    // the md one to "Cloud" and the file one to "Sources".
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(vec![md_tbl("md_events"), file_tbl("local_sales")]);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    // The flat a11y tree has no section structure, so the SECTION-HEADER counts are
    // the teeth: a misclassified md table would read "Cloud (0)" / "Sources (2)".
    assert!(
        snap.has_label("Cloud (1)"),
        "Cloud section holds exactly the 1 md table"
    );
    assert!(snap.has_label("md_events"), "md-attached table row renders");
    assert!(
        snap.has_label("Sources (1)"),
        "file table classified to Sources, not Cloud"
    );
}

#[gpui::test]
#[serial]
fn test_result_renders_disconnected(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            let mgr = ws.open_connections_for_test();
            mgr.set_md_status(ConnectionStatus::Disconnected);
            mgr.set_md_test_result("Connection OK".into());
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("Connect"), "Disconnected arm shows Connect");
    assert!(
        snap.has_label("Test connection"),
        "Disconnected arm shows Test"
    );
    assert!(
        snap.has_label("Connection OK"),
        "seeded test-result message renders"
    );
}

#[gpui::test]
#[serial]
fn routing_chip_shows_md_not_local(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    // toggle_sql_console may refresh md_databases off the engine on open → runtime.
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.update(|window, app| {
        shell.update(app, |ws, cx| {
            ws.seed_routing_chip_for_test(1234, Routing::Md, window, cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains("ms · md"),
        "routing chip shows the md suffix"
    );
    assert!(
        !snap.has_label_contains("· local"),
        "teeth: not the local suffix"
    );
}

#[gpui::test]
#[serial]
fn test_result_renders_connected(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            let mgr = ws.open_connections_for_test();
            mgr.set_md_status(ConnectionStatus::Connected); // keeps md_databases
            mgr.set_md_databases(vec!["sample_data".into()]);
            mgr.set_md_test_result("Connection OK".into());
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label("Disconnect"),
        "Connected arm shows Disconnect"
    );
    assert!(
        snap.has_label("Test connection"),
        "Connected arm shows Test"
    );
    assert!(snap.has_label("Forget token"), "Connected arm shows Forget");
    assert!(
        snap.has_label("sample_data"),
        "attached db name renders under Connected"
    );
    assert!(
        snap.has_label("Connection OK"),
        "test-result message renders"
    );
}
