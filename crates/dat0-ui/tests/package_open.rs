//! Opening a `.dat0` package read-only, unpacking one, and exporting a
//! window's work as one (step 5.4d).
//!
//! File → Open, Unpack and Export Package were disabled, the sidebar's
//! Packages rows and the hero's recent packages opened the package as a data
//! file, and a dropped package was refused as a file type dat0 does not know
//! (PD-023). These tests seal a real package with dat0-core, then open and
//! unpack it each way and mount a window on what they ask for, and export
//! from a window and read back what it wrote.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, AppEventRx, Opening};
use dat0_core::session::queries::SavedQuery;
use dat0_core::session::{Session, Tab};
use dat0_engine::{QueryEngine as _, SortDirection, SortKey, Transformation};
use dat0_i18n::t;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

const BUDGET: u64 = 256 * 1024 * 1024;

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
    dat0_core::globals::install_recents(std::sync::Arc::new(std::sync::Mutex::new(
        dat0_core::recents::Recents::with_path(tmp.path().join("recents.json")),
    )));
    std::mem::forget(tmp);
    root
});

const COMMANDS: &[&str] = &[ids::SQL_LOAD_QUERY, ids::VIEW_SET_NULL];

#[derive(Clone, Props)]
struct HostProps {
    opening: Opening,
    /// What the `open` button opens through `package_open::open`, the `drop`
    /// button drops on the window, and the `export` button writes.
    package: Option<PathBuf>,
    /// Where the `unpack` button unpacks `package`.
    folder: Option<PathBuf>,
    boot: Boot,
}

/// Props never change after mount.
impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        (&self.opening, &self.package, &self.folder)
            == (&other.opening, &other.package, &other.folder)
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
    let (opened, dropped, exported) = (
        props.package.clone(),
        props.package.clone(),
        props.package.clone(),
    );
    let unpacked = props.package.clone().zip(props.folder.clone());
    rsx! {
        div { "data-a11y-id": "window-name", "{ws.name}" }
        button {
            "data-a11y-id": "open",
            onclick: {
                let events = events.clone();
                move |_| {
                    if let Some(package) = opened.clone() {
                        dat0_ui::package_open::open(ws, &events, package);
                    }
                }
            },
        }
        button {
            "data-a11y-id": "unpack",
            onclick: {
                let events = events.clone();
                move |_| {
                    let events = events.clone();
                    if let Some((package, folder)) = unpacked.clone() {
                        spawn(async move {
                            dat0_ui::package_unpack::unpack(ws, &events, package, folder).await
                        });
                    }
                }
            },
        }
        button {
            "data-a11y-id": "export",
            onclick: move |_| {
                if let Some(dest) = exported.clone() {
                    spawn(async move { dat0_ui::package_export::export(ws, dest).await });
                }
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

/// A boot whose process bus the test reads: no window takes the lease, so
/// what a window asks for there stays for the test.
fn boot() -> (Boot, AppEventRx) {
    let reg = ActionRegistry::new();
    register_all(&reg).expect("built-in actions register");
    let boot = Boot::new(reg, Vec::new());
    let rx = boot.rx.lock().take().expect("the process bus");
    (boot, rx)
}

fn mount(opening: Opening, package: Option<PathBuf>, boot: Boot) -> Harness {
    mount_with(opening, package, None, boot)
}

fn mount_with(
    opening: Opening,
    package: Option<PathBuf>,
    folder: Option<PathBuf>,
    boot: Boot,
) -> Harness {
    let _ = STATE_ROOT.as_path();
    Harness::new(
        Host,
        HostProps {
            opening,
            package,
            folder,
            boot,
        },
    )
}

/// The windows asked for on the process bus so far.
fn asked(rx: &mut AppEventRx) -> Vec<Opening> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|ev| match ev {
            AppEvent::OpenWindow(opening) => Some(opening),
            _ => None,
        })
        .collect()
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

/// A package sealed from a session holding `stock`, its tab sorted by
/// quantity, highest first, and one saved query. Returns its path, canonical.
fn package(rt: &tokio::runtime::Runtime, name: &str) -> PathBuf {
    package_with(rt, name, |_| {})
}

/// [`package`], with `tweak` applied to what is sealed.
fn package_with(
    rt: &tokio::runtime::Runtime,
    name: &str,
    tweak: impl FnOnce(&mut dat0_format::PackageContents),
) -> PathBuf {
    let dir = STATE_ROOT.join(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = dir.join(format!("{name}.dat0"));
    rt.block_on(async {
        let mut sess = Session::new(&STATE_ROOT.join("sealing"), BUDGET)
            .await
            .expect("session");
        sess.engine
            .execute(
                "CREATE TABLE stock AS SELECT * FROM (VALUES ('b', 2), ('a', 3), ('c', 1)) t(name, qty)",
            )
            .await
            .expect("table");
        sess.engine.ensure_rowid("stock").await.expect("row ids");
        let sorted = Transformation::Sort {
            keys: vec![SortKey {
                column: "qty".into(),
                direction: SortDirection::Desc,
            }],
        };
        sess.set_tabs(
            vec![Tab {
                table_name: "stock".into(),
                source_path: None,
                transform_stack: vec![sorted],
                undo_cursor: 1,
                extra: Default::default(),
            }],
            Some(0),
        )
        .expect("tabs");
        sess.set_saved_queries(vec![SavedQuery {
            id: uuid::Uuid::now_v7(),
            name: "lucky".into(),
            sql: "SELECT 7 AS lucky".into(),
            saved_at: 0,
        }])
        .expect("saved query");
        let mut contents = dat0_core::package::session_to_contents(&sess)
            .await
            .expect("contents");
        tweak(&mut contents);
        dat0_format::Writer::write(&contents, sess.engine.as_ref(), &out)
            .await
            .expect("seal");
        sess.engine.close().await.expect("close");
    });
    std::fs::canonicalize(&out).expect("canonical")
}

#[test]
#[serial]
fn a_package_opens_read_only_with_its_tabs_views_and_queries() {
    let rt = runtime();
    let _guard = rt.enter();
    let path = package(&rt, "sealed");
    let (boot, _rx) = boot();
    let mut h = mount(
        Opening::Inspect {
            package: path.clone(),
        },
        None,
        boot,
    );
    assert!(
        pump(&mut h, |h| column(h, 0) == ["a", "b", "c"]),
        "its tab, sorted as it was sealed: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );
    assert_eq!(text(&h, "col-sort-1"), "▼");
    assert_eq!(text(&h, "window-name"), "sealed", "named for its file");

    // Read-only: an edit is refused, and says so.
    h.click(&format!("do-{}", ids::VIEW_SET_NULL));
    h.settle();
    assert!(
        banners(&h).contains(&t("view.read_only")),
        "{:?}",
        banners(&h)
    );
    assert_eq!(column(&h, 0), ["a", "b", "c"], "and nothing changed");

    // Its saved query came along.
    h.click(&format!("do-{}", ids::SQL_LOAD_QUERY));
    assert!(
        text(&h, "saved-row-0").contains("lucky"),
        "{:?}",
        text(&h, "saved-queries")
    );

    let recent = dat0_core::recents::RecentEntry::Package { path };
    let remembered = dat0_core::globals::recents()
        .unwrap()
        .lock()
        .unwrap()
        .list()
        .contains(&recent);
    assert!(remembered, "and it is listed among the recent packages");
}

#[test]
#[serial]
fn open_package_asks_for_a_window_of_its_own() {
    let rt = runtime();
    let _guard = rt.enter();
    let path = package(&rt, "asked");
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), Some(path.clone()), boot);
    h.click("open");
    h.settle();
    assert_eq!(asked(&mut rx), [Opening::Inspect { package: path }]);
}

#[test]
#[serial]
fn a_dropped_package_opens_in_a_window_of_its_own() {
    let rt = runtime();
    let _guard = rt.enter();
    let path = package(&rt, "dropped");
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), Some(path.clone()), boot);
    h.click("drop");
    h.settle();
    assert_eq!(asked(&mut rx), [Opening::Inspect { package: path }]);
    assert!(
        !banners(&h).contains(&t("drop.unsupported")),
        "not refused as a file type: {:?}",
        banners(&h)
    );
}

#[test]
#[serial]
fn a_missing_package_says_so() {
    let rt = runtime();
    let _guard = rt.enter();
    let gone = STATE_ROOT.join("gone.dat0");
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), Some(gone), boot);
    h.click("open");
    h.settle();
    assert!(
        banners(&h).contains(&t("package.open.failed.title")),
        "{:?}",
        banners(&h)
    );
    assert!(asked(&mut rx).is_empty());
}

#[test]
#[serial]
fn a_recent_package_opens_from_the_hero_and_the_sidebar() {
    let rt = runtime();
    let _guard = rt.enter();
    let path = package(&rt, "recent");
    {
        let recents = dat0_core::globals::recents().unwrap();
        let mut recents = recents.lock().unwrap();
        recents
            .push(dat0_core::recents::RecentEntry::Package { path: path.clone() })
            .unwrap();
    }
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), None, boot);
    assert!(pump(&mut h, |h| h.by_a11y_id("hero-recent-0").is_some()));
    // Newest first: the one just pushed.
    h.click("hero-recent-0");
    h.settle();
    assert_eq!(
        asked(&mut rx),
        [Opening::Inspect {
            package: path.clone()
        }],
        "the hero opens it read-only, not as a workspace: {:?}",
        banners(&h)
    );

    h.click("row-packages-0");
    h.settle();
    assert_eq!(
        asked(&mut rx),
        [Opening::Inspect { package: path }],
        "and so does the sidebar, rather than as a file"
    );
}

/// Unpacking writes the package into the folder as a workspace, and opens it.
/// What the folder already held stays as it was: the package's Parquet is
/// staged inside `.dat0/`, not in the folder's own `data/`.
#[test]
#[serial]
fn a_package_unpacks_into_a_workspace_and_opens_it() {
    let rt = runtime();
    let _guard = rt.enter();
    let path = package(&rt, "to-unpack");
    let folder = STATE_ROOT.join("unpacked-here");
    std::fs::create_dir_all(folder.join("data")).unwrap();
    let theirs = folder.join("data").join("stock.parquet");
    std::fs::write(&theirs, b"the user's own file").unwrap();
    let (boot, mut rx) = boot();
    let mut h = mount_with(
        Opening::files(Vec::new()),
        Some(path),
        Some(folder.clone()),
        boot,
    );
    h.click("unpack");
    let done = t("package.unpack.done.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&done)),
        "{:?}",
        banners(&h)
    );
    let root = std::fs::canonicalize(&folder).unwrap();
    assert_eq!(
        asked(&mut rx),
        [Opening::Workspace {
            root: root.clone(),
            networked: false
        }],
        "and the workspace opens in a window of its own"
    );
    let dat0 = root.join(".dat0");
    for f in ["workspace.duckdb", "manifest.json", "session.json"] {
        assert!(dat0.join(f).is_file(), "{f}");
    }
    assert!(!dat0.join("unpack").exists(), "the staged Parquet is gone");
    assert_eq!(
        std::fs::read(&theirs).unwrap(),
        b"the user's own file",
        "and the folder's own data/ is untouched"
    );
}

#[test]
#[serial]
fn a_package_is_not_unpacked_over_a_workspace() {
    let rt = runtime();
    let _guard = rt.enter();
    let path = package(&rt, "not-over");
    let folder = STATE_ROOT.join("already-a-workspace");
    std::fs::create_dir_all(folder.join(".dat0")).unwrap();
    let (boot, mut rx) = boot();
    let mut h = mount_with(Opening::files(Vec::new()), Some(path), Some(folder), boot);
    h.click("unpack");
    let refused = t("package.unpack.exists");
    assert!(
        pump(&mut h, |h| banners(h).contains(&refused)),
        "{:?}",
        banners(&h)
    );
    assert!(asked(&mut rx).is_empty());
}

#[test]
#[serial]
fn a_package_that_will_not_unpack_leaves_the_folder_as_it_was() {
    let rt = runtime();
    let _guard = rt.enter();
    let folder = STATE_ROOT.join("unpack-fails");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.txt"), "mine").unwrap();
    // It verifies, but its recipe names a column its data does not have: the
    // unpack fails once it has begun writing the workspace.
    let broken = package_with(&rt, "unmakeable", |c| {
        c.recipe.tables[0]
            .schema
            .push(dat0_format::ColumnFingerprint {
                name: "gone".into(),
                r#type: "INTEGER".into(),
            });
    });
    let (boot, mut rx) = boot();
    let mut h = mount_with(
        Opening::files(Vec::new()),
        Some(broken),
        Some(folder.clone()),
        boot,
    );
    h.click("unpack");
    let failed = t("package.unpack.failed.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&failed)),
        "{:?}",
        banners(&h)
    );
    let left: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["notes.txt"], "no half-made workspace");
    assert!(asked(&mut rx).is_empty());
}

/// A workspace unpacked from [`package`] into `name` under the state root.
fn workspace(rt: &tokio::runtime::Runtime, name: &str) -> PathBuf {
    let sealed = package(rt, &format!("{name}-sealed"));
    let root = STATE_ROOT.join(name);
    rt.block_on(async {
        let parsed = dat0_format::Reader::open(&sealed).expect("sealed");
        dat0_core::package::contents_to_workspace(&parsed, &root, BUDGET)
            .await
            .expect("unpacked");
    });
    root
}

fn mount_workspace(root: PathBuf, package: Option<PathBuf>) -> Harness {
    let (boot, _rx) = boot();
    let opening = Opening::Workspace {
        root,
        networked: false,
    };
    let mut h = mount(opening, package, boot);
    assert!(
        pump(&mut h, |h| column(h, 0) == ["a", "b", "c"]),
        "its tab: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );
    h
}

#[test]
#[serial]
fn a_window_exports_its_tables_views_and_queries_as_a_package() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "export-ws");
    let out = STATE_ROOT.join("exported.dat0");
    std::fs::write(&out, b"an older package").unwrap();
    let mut h = mount_workspace(root, Some(out.clone()));

    h.click("export");
    let done = t("package.export.done.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&done)),
        "{:?}",
        banners(&h)
    );

    let parsed = dat0_format::Reader::open(&out).expect("the package opens and verifies");
    let tables: Vec<_> = parsed
        .recipe
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(tables, ["stock"]);
    let views = &parsed.views.views;
    assert_eq!(views.len(), 1, "{views:?}");
    assert_eq!(views[0].table_name, "stock");
    assert_eq!(views[0].transform_stack.len(), 1, "with its sort");
    let queries: Vec<_> = parsed
        .queries
        .queries
        .iter()
        .map(|q| q.name.as_str())
        .collect();
    assert_eq!(queries, ["lucky"]);
}

#[test]
#[serial]
fn an_export_that_fails_says_so() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "export-fails-ws");
    let out = STATE_ROOT.join("no-such-folder").join("exported.dat0");
    let mut h = mount_workspace(root, Some(out.clone()));

    h.click("export");
    let failed = t("package.export.failed.title");
    assert!(
        pump(&mut h, |h| banners(h).contains(&failed)),
        "{:?}",
        banners(&h)
    );
    assert!(!out.exists());
}
