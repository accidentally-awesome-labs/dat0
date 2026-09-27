//! Opening SQLite files (step 5.4e).
//!
//! A SQLite file dropped, opened, passed on the command line or chosen as the
//! Chinook sample was refused as a file type dat0 did not know, and nothing
//! attached one (PD-023). These tests open real SQLite files in a real shell:
//! the first table opens in a tab and the rest are listed under CONNECTIONS,
//! a session opened again attaches its file before its tabs come back, a file
//! that is gone is reported, and Save Workspace keeps the tab readable.

mod support;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::connections::sqlite;
use dat0_core::events::Opening;
use dat0_core::session::{PersistedAttachment, PersistedAttachmentKind, Session, Tab};
use dat0_engine::QueryEngine as _;
use dat0_i18n::t;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::Surface;
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

const BUDGET: u64 = 256 * 1024 * 1024;

/// Process-global state root, config dir and recents store, leaked so a
/// session never reads a deleted directory mid-test.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    dat0_engine::extension_bootstrap::__test_install_sqlite_scanner().expect("sqlite_scanner");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("state");
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&root).expect("mkdir state");
    std::fs::create_dir_all(&cfg).expect("mkdir cfg");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", &cfg) };
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(cfg.join("settings.toml")),
        true,
    )
    .expect("seed first_run_done");
    dat0_core::globals::install_state_root(root.clone());
    dat0_core::globals::install_recents(std::sync::Arc::new(std::sync::Mutex::new(
        dat0_core::recents::Recents::with_path(tmp.path().join("recents.json")),
    )));
    std::mem::forget(tmp);
    root
});

#[derive(Clone, Props)]
struct HostProps {
    opening: Opening,
    /// Where the `save` button saves the window as a workspace.
    folder: Option<PathBuf>,
    boot: Boot,
}

/// Props never change after mount.
impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        (&self.opening, &self.folder) == (&other.opening, &other.folder)
    }
}

/// `App`'s wiring, minus what needs a desktop window.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let ws = Workspace::provide_for(&props.opening);
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_context_provider(|| props.boot.clone());
    use_context_provider(|| boot.registry.clone());
    session_boot::use_session_on(ws, props.opening.clone());
    let _events = use_window_bus(boot, ws, surface);
    let folder = props.folder.clone();
    rsx! {
        button {
            "data-a11y-id": "save",
            onclick: move |_| {
                if let Some(folder) = folder.clone() {
                    spawn(async move { dat0_ui::workspace_save::save(ws, folder).await });
                }
            },
        }
        Shell {}
    }
}

fn mount(opening: Opening, folder: Option<PathBuf>) -> Harness {
    let _ = STATE_ROOT.as_path();
    let reg = ActionRegistry::new();
    register_all(&reg).expect("built-in actions register");
    let boot = Boot::new(reg, Vec::new());
    Harness::new(
        Host,
        HostProps {
            opening,
            folder,
            boot,
        },
    )
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Settle, then sleep, until `done` or two minutes.
fn pump(h: &mut Harness, done: impl Fn(&Harness) -> bool) -> bool {
    for _ in 0..4800 {
        h.settle();
        if done(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

fn column(h: &Harness, col: usize) -> Vec<String> {
    (0..10)
        .map(|r| text(h, &format!("cell-{r}-{col}")))
        .take_while(|v| !v.is_empty())
        .collect()
}

fn banners(h: &Harness) -> String {
    text(h, "banner-host")
}

/// The CONNECTIONS rows, in order: each database with its count of tables,
/// then its tables.
fn connections(h: &Harness) -> Vec<String> {
    (0..20)
        .map(|i| text(h, &format!("row-connections-{i}")))
        .take_while(|v| !v.is_empty())
        .collect()
}

/// A copy of `tests/fixtures/small/simple.sqlite` — `items(id, name)`, rows
/// 1 `a`, 2 `b`, 3 `c` — at `name` under the state root.
fn shop(name: &str) -> PathBuf {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/small/simple.sqlite");
    let dir = STATE_ROOT.join(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("shop.sqlite");
    std::fs::copy(fixture, &path).expect("copy the fixture");
    std::fs::canonicalize(path).expect("canonical")
}

/// A closed scratch session holding `shop.items` in a tab, with the file
/// attached as `shop`, and a table `t` of its own in a second tab. Returns
/// the session's directory.
fn session_with_shop(rt: &tokio::runtime::Runtime, file: &Path) -> PathBuf {
    rt.block_on(async {
        let mut sess = Session::new(&STATE_ROOT.join("sessions"), BUDGET)
            .await
            .expect("session");
        sqlite::attach(sess.engine.as_ref(), file, "shop")
            .await
            .expect("attach");
        let view = sqlite::open_table(sess.engine.as_ref(), "shop", "items")
            .await
            .expect("open items");
        sess.engine
            .execute("CREATE TABLE t AS SELECT * FROM (VALUES (7), (8)) v(x)")
            .await
            .expect("t");
        sess.set_attachments(vec![PersistedAttachment {
            alias: "shop".into(),
            kind: PersistedAttachmentKind::Sqlite {
                path: file.display().to_string(),
            },
        }])
        .expect("attachments");
        let tab = |table: &str| Tab {
            table_name: table.into(),
            source_path: None,
            transform_stack: Vec::new(),
            undo_cursor: 0,
            extra: Default::default(),
        };
        sess.set_tabs(vec![tab(&view), tab("t")], Some(0))
            .expect("tabs");
        let dir = sess.home.root_dir().to_path_buf();
        sess.engine.close().await.expect("close");
        dir
    })
}

#[test]
#[serial]
fn a_sqlite_file_opens_its_first_table_and_lists_the_rest() {
    let rt = runtime();
    let _guard = rt.enter();
    let file = shop("opened");
    let mut h = mount(Opening::files(vec![file]), None);

    assert!(
        pump(&mut h, |h| column(h, 0) == ["1", "2", "3"]),
        "items in a tab: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );
    assert_eq!(column(&h, 1), ["a", "b", "c"]);
    assert!(
        h.html().contains("shop_items"),
        "the tab names the file's table"
    );
    assert_eq!(connections(&h), ["shop 1", "items"]);
}

#[test]
#[serial]
fn the_chinook_sample_opens_and_its_tables_open_from_the_sidebar() {
    let rt = runtime();
    let _guard = rt.enter();
    // The hero offers the samples only while nothing is recent, and a
    // workspace saved by another test here is remembered in the same store.
    let _ = STATE_ROOT.as_path();
    if let Some(recents) = dat0_core::globals::recents() {
        *recents.lock().expect("recents") =
            dat0_core::recents::Recents::with_path(STATE_ROOT.join("no-recents.json"));
    }
    let mut h = mount(Opening::files(Vec::new()), None);
    assert!(pump(&mut h, |h| h
        .by_a11y_id("hero-sample-chinook")
        .is_some()));

    h.click("hero-sample-chinook");
    assert!(
        pump(&mut h, |h| connections(h).len() == 12),
        "chinook and its 11 tables: {:?}; banners: {:?}",
        connections(&h),
        banners(&h)
    );
    assert_eq!(connections(&h)[..3], ["chinook 11", "Album", "Artist"]);
    assert!(
        pump(&mut h, |h| {
            column(h, 1)
                .first()
                .is_some_and(|v| v == "For Those About To Rock We Salute You")
        }),
        "Album in a tab: {:?}",
        column(&h, 1)
    );

    // A table's row opens it; the database's row folds its tables.
    h.click("row-connections-2");
    assert!(
        pump(&mut h, |h| column(h, 1)
            .first()
            .is_some_and(|v| v == "AC/DC")),
        "Artist in a tab: {:?}",
        column(&h, 1)
    );
    h.click("row-connections-0");
    h.settle();
    assert_eq!(connections(&h), ["chinook 11"]);
}

#[test]
#[serial]
fn a_session_opened_again_attaches_its_file_before_its_tabs_come_back() {
    let rt = runtime();
    let _guard = rt.enter();
    let file = shop("reopened");
    let dir = session_with_shop(&rt, &file);

    let mut h = mount(Opening::Recover { dir }, None);

    assert!(
        pump(&mut h, |h| column(h, 1) == ["a", "b", "c"]),
        "the file's tab, read again: {:?}; banners: {:?}",
        column(&h, 1),
        banners(&h)
    );
    assert_eq!(connections(&h), ["shop 1", "items"]);
}

#[test]
#[serial]
fn a_file_that_is_gone_is_reported_and_the_other_tabs_come_back() {
    let rt = runtime();
    let _guard = rt.enter();
    let file = shop("gone");
    let dir = session_with_shop(&rt, &file);
    std::fs::remove_file(&file).expect("the file goes");

    let mut h = mount(Opening::Recover { dir }, None);

    let reported = t("sqlite.reattach.failed.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&reported)
            && column(h, 0) == ["7", "8"]),
        "banners: {:?}; column: {:?}",
        banners(&h),
        column(&h, 0)
    );
    assert!(connections(&h).is_empty(), "{:?}", connections(&h));
}

#[test]
#[serial]
fn a_window_saved_as_a_workspace_still_reads_its_file() {
    let rt = runtime();
    let _guard = rt.enter();
    let file = shop("saved");
    let folder = STATE_ROOT.join("saved-ws");
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = mount(Opening::files(vec![file]), Some(folder));
    assert!(pump(&mut h, |h| column(h, 0) == ["1", "2", "3"]));

    h.click("save");
    let done = t("workspace.save.done.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&done)),
        "{:?}",
        banners(&h)
    );
    assert!(
        pump(&mut h, |h| column(h, 1) == ["a", "b", "c"]),
        "the tab, on the moved database: {:?}; banners: {:?}",
        column(&h, 1),
        banners(&h)
    );
    assert_eq!(connections(&h), ["shop 1", "items"]);
}
