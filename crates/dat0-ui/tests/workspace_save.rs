//! Saving a window's work as a workspace (step 5.4c).
//!
//! Save Workspace asked for a file name and logged it (PD-023). These tests
//! mount the real `Shell` over a real scratch session, save it into a folder
//! through `workspace_save`, and check that the window went on as the
//! workspace: its tables, views and SQL moved into the folder, its grid on the
//! new engine, its name, lock and recent entry the folder's.

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::Opening;
use dat0_core::session::queries::HistoryEntry;
use dat0_engine::{DuckDBEngine, SortDirection, SortKey, Transformation};
use dat0_i18n::t;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// Process-global state root, config dir and recents store, leaked so a
/// session never reads a deleted directory mid-test.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
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
    dat0_core::globals::install_recents(Arc::new(std::sync::Mutex::new(
        dat0_core::recents::Recents::with_path(tmp.path().join("recents.json")),
    )));
    std::mem::forget(tmp);
    root
});

/// An engine the `hold` button keeps, as a query still running would.
static HELD: parking_lot::Mutex<Option<Arc<DuckDBEngine>>> = parking_lot::Mutex::new(None);

const COMMANDS: &[&str] = &[ids::SQL_HISTORY, ids::SQL_RUN, ids::LIVE_REFRESH];

#[derive(Clone, Props)]
struct HostProps {
    opening: Opening,
    /// Put in the session's history by the `seed` button.
    history: Vec<String>,
    /// Where the `save` button saves.
    save_to: PathBuf,
    /// What the `drop` button opens.
    drop: Option<PathBuf>,
    boot: Boot,
}

/// Props never change after mount.
impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        (&self.opening, &self.history, &self.save_to, &self.drop)
            == (&other.opening, &other.history, &other.save_to, &other.drop)
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
    let events = use_window_bus(boot, ws, surface);
    let seed = props.history.clone();
    let (save_to, dropped) = (props.save_to.clone(), props.drop.clone());
    rsx! {
        div { "data-a11y-id": "window-id", "{ws.window_id}" }
        div { "data-a11y-id": "window-name", "{ws.name}" }
        button {
            "data-a11y-id": "seed",
            onclick: move |_| {
                let slot = ws.session.peek().ready().cloned().expect("session ready");
                let entries = seed
                    .iter()
                    .map(|sql| HistoryEntry { sql: sql.clone(), ran_at: 0, ok: true, elapsed_ms: 0 })
                    .collect();
                slot.lock().set_query_history(entries).expect("seed history");
            },
        }
        button {
            "data-a11y-id": "save",
            onclick: move |_| {
                let folder = save_to.clone();
                spawn(async move { dat0_ui::workspace_save::save(ws, folder).await });
            },
        }
        button {
            "data-a11y-id": "read-only",
            onclick: move |_| {
                let mut read_only = ws.read_only;
                read_only.set(true);
            },
        }
        button {
            "data-a11y-id": "hold",
            onclick: move |_| {
                let slot = ws.session.peek().ready().cloned().expect("session ready");
                *HELD.lock() = Some(Arc::clone(&slot.lock().engine));
            },
        }
        button {
            "data-a11y-id": "drop",
            onclick: move |_| {
                let paths: Vec<PathBuf> = dropped.iter().cloned().collect();
                spawn(async move { session_boot::open_paths(ws, paths).await });
            },
        }
        for id in COMMANDS.iter().copied() {
            button {
                key: "{id}",
                "data-a11y-id": "do-{id}",
                onclick: {
                    let events = events.clone();
                    move |_| assert!(route(ws, &events, surface, id), "{id} was not routed")
                },
            }
        }
        Shell {}
    }
}

fn boot() -> Boot {
    let reg = ActionRegistry::new();
    register_all(&reg).expect("built-in actions register");
    Boot::new(reg, Vec::new())
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

fn session_at(dir: &Path) -> Option<dat0_core::session::SessionState> {
    dat0_core::session::migrate::load(&dir.join("session.json")).ok()
}

fn sorted(direction: SortDirection) -> Transformation {
    Transformation::Sort {
        keys: vec![SortKey {
            column: "qty".into(),
            direction,
        }],
    }
}

/// A folder under the state root holding `stock.csv`, and a folder beside it
/// to save into.
fn folders(name: &str) -> (PathBuf, PathBuf) {
    let dir = STATE_ROOT.join(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let csv = dir.join("stock.csv");
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\n").expect("write csv");
    let target = dir.join("project");
    std::fs::create_dir_all(&target).expect("mkdir target");
    (csv, target)
}

/// A scratch window over `csv`, painted.
fn window(csv: &Path, save_to: &Path, drop: Option<PathBuf>, history: &[&str]) -> Harness {
    let mut h = Harness::new(
        Host,
        HostProps {
            opening: Opening::files(vec![csv.to_path_buf()]),
            history: history.iter().map(|s| s.to_string()).collect(),
            save_to: save_to.to_path_buf(),
            drop,
            boot: boot(),
        },
    );
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "the CSV never painted"
    );
    h
}

fn scratch_of(h: &Harness) -> PathBuf {
    STATE_ROOT.join("scratch").join(text(h, "window-id"))
}

#[test]
#[serial]
fn a_scratch_window_is_saved_as_a_workspace_and_goes_on_in_it() {
    let rt = runtime();
    let _guard = rt.enter();
    let (csv, target) = folders("saved");
    let sql = "SELECT 7 AS seven";
    let boot = boot();
    let mut h = Harness::new(
        Host,
        HostProps {
            opening: Opening::files(vec![csv.clone()]),
            history: vec![sql.to_string()],
            save_to: target.clone(),
            drop: None,
            boot: boot.clone(),
        },
    );
    assert!(pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]));
    h.click("col-sort-1");
    assert!(pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]));
    // A query's rows, in a tab of their own: a view in this engine alone.
    h.click("seed");
    h.click(&format!("do-{}", ids::SQL_HISTORY));
    h.click("hist-row-0");
    h.click(&format!("do-{}", ids::SQL_RUN));
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "7"),
        "the query never ran: {:?}",
        text(&h, "console-error-text")
    );
    let scratch = scratch_of(&h);

    h.click("save");
    let done = t("workspace.save.done.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&done)
            && column(h, 0) == ["c", "b", "a"]),
        "saved, and back on its sorted table: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );
    assert_eq!(text(&h, "col-sort-1"), "▲", "its header still says so");
    assert!(
        h.by_a11y_id("tab-1").is_none(),
        "the query's rows did not move, so their tab closed"
    );

    let root = std::fs::canonicalize(&target).unwrap();
    let dat0 = root.join(".dat0");
    assert!(
        dat0.join("workspace.duckdb").is_file(),
        "the database moved"
    );
    assert!(
        dat0.join("manifest.json").is_file(),
        "and the save finished"
    );
    assert!(!scratch.exists(), "the scratch directory is gone");
    assert_eq!(text(&h, "window-name"), "project", "named for its folder");
    assert!(
        dat0_core::globals::recents_snapshot().contains(&root),
        "listed among the recent workspaces"
    );
    let me = uuid::Uuid::parse_str(&text(&h, "window-id")).unwrap();
    assert_eq!(
        boot.windows.holding(&root),
        Some(me),
        "opening the folder brings this window forward"
    );

    // The grid is on the workspace's engine: a change is made there, and
    // recorded in the workspace's session.
    h.click("col-sort-1");
    assert!(pump(&mut h, |h| column(h, 0) == ["a", "b", "c"]));
    let recorded = pump(&mut h, |_| {
        session_at(&dat0).is_some_and(|s| {
            s.tabs
                .first()
                .is_some_and(|t| t.transform_stack == [sorted(SortDirection::Desc)])
                && s.sql_tabs.iter().any(|q| q.sql == sql)
        })
    });
    assert!(
        recorded,
        "the workspace never recorded the change: {:?}",
        session_at(&dat0).map(|s| s.tabs)
    );
    // The tab still knows its file: reading it again replaces its own table
    // rather than importing beside it.
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\nd,0\n").unwrap();
    h.click(&format!("do-{}", ids::LIVE_REFRESH));
    assert!(
        pump(&mut h, |h| column(h, 0) == ["a", "b", "c", "d"]),
        "refreshed into its own table: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );

    // Closed and opened again, it is all there.
    drop(h);
    let mut again = Harness::new(
        Host,
        HostProps {
            opening: Opening::Workspace {
                root: root.clone(),
                networked: false,
            },
            history: Vec::new(),
            save_to: target,
            drop: None,
            boot: self::boot(),
        },
    );
    assert!(
        pump(&mut again, |h| column(h, 0) == ["a", "b", "c", "d"]),
        "reopened: {:?}; banners: {:?}",
        column(&again, 0),
        banners(&again)
    );
    assert_eq!(text(&again, "col-sort-1"), "▼");
}

/// A view change interrupts the view query before it, whichever tab that was
/// for. Every tab's view is rebuilt after a save, and after a reopen: each must
/// come back, not only the last one started.
#[test]
#[serial]
fn every_tab_keeps_its_view_through_a_save_and_a_reopen() {
    let rt = runtime();
    let _guard = rt.enter();
    let (csv, target) = folders("two-tabs");
    let other = csv.with_file_name("more.csv");
    std::fs::write(&other, "id,n\n1,b\n3,a\n2,c\n").unwrap();
    let mut h = window(&csv, &target, Some(other), &[]);
    h.click("col-sort-1");
    assert!(pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]));
    h.click("drop");
    assert!(pump(&mut h, |h| column(h, 0) == ["1", "3", "2"]));
    h.click("col-sort-0");
    assert!(pump(&mut h, |h| column(h, 0) == ["1", "2", "3"]));

    h.click("save");
    let done = t("workspace.save.done.title");
    assert!(pump(&mut h, |h| banners(h).contains(&done)
        && column(h, 0) == ["1", "2", "3"]));
    h.click("tab-0");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]),
        "the other tab, sorted on the new engine too: {:?}",
        column(&h, 0)
    );
    let root = std::fs::canonicalize(&target).unwrap();
    let dat0 = root.join(".dat0");
    assert!(pump(&mut h, |_| session_at(&dat0).is_some_and(|s| s
        .tabs
        .len()
        == 2
        && s.active_tab == Some(0))));

    drop(h);
    let mut again = Harness::new(
        Host,
        HostProps {
            opening: Opening::Workspace {
                root,
                networked: false,
            },
            history: Vec::new(),
            save_to: target,
            drop: None,
            boot: boot(),
        },
    );
    assert!(
        pump(&mut again, |h| column(h, 0) == ["c", "b", "a"]),
        "reopened on its first tab: {:?}",
        column(&again, 0)
    );
    again.click("tab-1");
    assert!(
        pump(&mut again, |h| column(h, 0) == ["1", "2", "3"]),
        "and the second tab's view came back too: {:?}",
        column(&again, 0)
    );
}

#[test]
#[serial]
fn a_folder_that_is_a_workspace_already_is_refused() {
    let rt = runtime();
    let _guard = rt.enter();
    let (csv, target) = folders("taken");
    std::fs::create_dir_all(target.join(".dat0")).unwrap();
    let mut h = window(&csv, &target, None, &[]);
    let scratch = scratch_of(&h);

    h.click("save");
    let exists = t("workspace.save.exists");
    assert!(
        pump(&mut h, |h| banners(h).contains(&exists)),
        "{:?}",
        banners(&h)
    );
    assert!(scratch.join("scratch.duckdb").is_file(), "nothing moved");
    assert_eq!(text(&h, "window-name"), "scratch");
    h.click("col-sort-1");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]),
        "and the window goes on as it was"
    );
}

/// A save that fails once the engine is closed reopens what is on disk. A
/// plain file where the folder should be fails it there: its `.dat0/` cannot
/// be made.
#[test]
#[serial]
fn a_save_that_fails_part_way_goes_on_with_what_is_on_disk() {
    let rt = runtime();
    let _guard = rt.enter();
    let (csv, target) = folders("fails");
    let not_a_folder = target.join("plain.txt");
    std::fs::write(&not_a_folder, "not a folder").unwrap();
    let mut h = window(&csv, &not_a_folder, None, &[]);
    h.click("col-sort-1");
    assert!(pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]));
    let scratch = scratch_of(&h);

    h.click("save");
    let failed = t("workspace.save.failed.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&failed)
            && column(h, 0) == ["c", "b", "a"]),
        "failed, and back on its session, sorted: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );
    assert!(
        scratch.join("scratch.duckdb").is_file(),
        "the session reopened where it was"
    );
    assert_eq!(text(&h, "window-name"), "scratch");

    // Its engine works, and still knows the tab's file.
    h.click("col-sort-1");
    assert!(pump(&mut h, |h| column(h, 0) == ["a", "b", "c"]));
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\nd,0\n").unwrap();
    h.click(&format!("do-{}", ids::LIVE_REFRESH));
    assert!(
        pump(&mut h, |h| column(h, 0) == ["a", "b", "c", "d"]),
        "refreshed into its own table: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );
}

#[test]
#[serial]
fn a_read_only_window_is_not_saved() {
    let rt = runtime();
    let _guard = rt.enter();
    let (csv, target) = folders("read-only");
    let mut h = window(&csv, &target, None, &[]);
    h.click("read-only");
    h.click("save");
    let refused = t("view.read_only");
    assert!(
        pump(&mut h, |h| banners(h).contains(&refused)),
        "{:?}",
        banners(&h)
    );
    assert!(!target.join(".dat0").exists(), "nothing moved");
}

#[test]
#[serial]
fn a_workspace_is_not_saved_again() {
    let rt = runtime();
    let _guard = rt.enter();
    let (csv, target) = folders("twice");
    let mut h = window(&csv, &target, None, &[]);
    h.click("save");
    let done = t("workspace.save.done.title");
    assert!(pump(&mut h, |h| banners(h).contains(&done)));

    h.click("save");
    let already = t("workspace.save.already");
    assert!(
        pump(&mut h, |h| banners(h).contains(&already)),
        "{:?}",
        banners(&h)
    );
}

#[test]
#[serial]
fn an_engine_still_in_use_is_left_where_it_was() {
    let rt = runtime();
    let _guard = rt.enter();
    let (csv, target) = folders("busy");
    let other = csv.with_file_name("more.csv");
    std::fs::write(&other, "id\n1\n").unwrap();
    let mut h = window(&csv, &target, Some(other), &[]);
    let scratch = scratch_of(&h);

    h.click("hold");
    h.click("save");
    // While the save waits, a file dropped on the window waits too.
    h.click("drop");
    let busy = t("workspace.save.busy");
    assert!(
        pump(&mut h, |h| banners(h).contains(&busy)),
        "{:?}",
        banners(&h)
    );
    assert!(scratch.join("scratch.duckdb").is_file(), "nothing moved");
    assert!(!target.join(".dat0").exists());
    assert!(
        pump(&mut h, |h| text(h, "tab-1").contains("more")),
        "the drop opened once the session was back: {:?}",
        text(&h, "tabstrip")
    );
    h.click("tab-0");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "and the window goes on as it was"
    );

    // Once nothing else holds it, it saves.
    HELD.lock().take();
    h.click("save");
    let done = t("workspace.save.done.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&done)),
        "{:?}",
        banners(&h)
    );
    assert!(target.join(".dat0/workspace.duckdb").is_file());
}
