//! Opening a workspace folder (step 5.4b).
//!
//! Open Workspace, Open Recent, the demo and Resume all posted the folder as a
//! file to open, and the drop path refused it as an unsupported file type
//! (PD-023). These tests make a real workspace with dat0-core, open it through
//! `workspace_open` and the router, and mount a window on what they ask for.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, AppEventRx, Opening};
use dat0_core::session::{Session, Tab};
use dat0_engine::{QueryEngine as _, RegisterOpts, SortDirection, SortKey, Transformation};
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
    first_run_done(true);
    dat0_core::globals::install_state_root(root.clone());
    dat0_core::globals::install_recents(std::sync::Arc::new(std::sync::Mutex::new(
        dat0_core::recents::Recents::with_path(tmp.path().join("recents.json")),
    )));
    std::mem::forget(tmp);
    root
});

fn settings_file() -> PathBuf {
    PathBuf::from(std::env::var_os("DAT0_CONFIG_DIR").expect("config dir")).join("settings.toml")
}

fn first_run_done(done: bool) {
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(settings_file()),
        done,
    )
    .expect("seed first_run_done");
}

const COMMANDS: &[&str] = &[ids::RECOVERY_REVIEW];

#[derive(Clone, Props)]
struct HostProps {
    opening: Opening,
    /// What the `open` button opens through `workspace_open::open`.
    open: Option<PathBuf>,
    boot: Boot,
}

/// Props never change after mount.
impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        (&self.opening, &self.open) == (&other.opening, &other.open)
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
    let open = props.open.clone();
    rsx! {
        div { "data-a11y-id": "window-id", "{ws.window_id}" }
        div { "data-a11y-id": "window-name", "{ws.name}" }
        button {
            "data-a11y-id": "open",
            onclick: {
                let events = events.clone();
                move |_| {
                    if let Some(folder) = open.clone() {
                        dat0_ui::workspace_open::open(ws, &events, folder);
                    }
                }
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

fn mount(opening: Opening, open: Option<PathBuf>, boot: Boot) -> Harness {
    let _ = STATE_ROOT.as_path();
    Harness::new(
        Host,
        HostProps {
            opening,
            open,
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

/// A workspace in `STATE_ROOT/<name>`, made the way Save Workspace makes one:
/// a scratch session holding `stock.csv`, sorted by quantity, promoted into
/// the folder. Returns the folder, canonical.
fn workspace(rt: &tokio::runtime::Runtime, name: &str) -> PathBuf {
    let root = STATE_ROOT.join(name);
    std::fs::create_dir_all(&root).expect("mkdir");
    let csv = root.join("stock.csv");
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\n").expect("write csv");
    rt.block_on(async {
        let mut sess = Session::new(&STATE_ROOT.join("making"), BUDGET)
            .await
            .expect("session");
        let info = sess
            .engine
            .register_file_as_table(&csv, RegisterOpts::default())
            .await
            .expect("import");
        let sorted = Transformation::Sort {
            keys: vec![SortKey {
                column: "qty".into(),
                direction: SortDirection::Asc,
            }],
        };
        sess.set_tabs(
            vec![Tab {
                table_name: info.name,
                source_path: Some(csv.clone()),
                transform_stack: vec![sorted],
                undo_cursor: 1,
                extra: Default::default(),
            }],
            Some(0),
        )
        .expect("tabs");
        let scratch = sess.home.root_dir().to_path_buf();
        sess.engine.close().await.expect("close");
        let promoted = dat0_core::workspace::promote::promote_files(
            &root,
            &scratch,
            dat0_core::time::now_epoch_secs(),
        )
        .expect("promote");
        sess.adopt_workspace(promoted.root.clone(), promoted.lock, BUDGET)
            .await
            .expect("adopt");
        std::fs::remove_dir_all(&promoted.old_scratch_dir).expect("remove scratch");
    });
    std::fs::canonicalize(&root).expect("canonical")
}

fn banners(h: &Harness) -> String {
    text(h, "banner-host")
}

#[test]
#[serial]
fn a_workspace_opens_with_its_tabs_and_views_and_is_remembered() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "sales");
    let (boot, _rx) = boot();
    let mut h = mount(
        Opening::Workspace {
            root: root.clone(),
            networked: false,
        },
        None,
        boot.clone(),
    );
    assert!(
        pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]),
        "its tab, sorted as it was left: {:?}; banners: {:?}",
        column(&h, 0),
        banners(&h)
    );
    assert_eq!(text(&h, "col-sort-1"), "▲");
    assert_eq!(text(&h, "window-name"), "sales", "named for its folder");
    assert!(
        dat0_core::globals::recents_snapshot().contains(&root),
        "and listed among the recent workspaces"
    );
    let me = uuid::Uuid::parse_str(&text(&h, "window-id")).unwrap();
    assert_eq!(
        boot.windows.holding(&root),
        Some(me),
        "this window holds it"
    );
}

#[test]
#[serial]
fn a_folder_without_a_workspace_is_refused_and_says_why() {
    let rt = runtime();
    let _guard = rt.enter();
    let plain = STATE_ROOT.join("just-a-folder");
    std::fs::create_dir_all(&plain).unwrap();
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), Some(plain), boot);
    h.click("open");
    h.settle();
    assert!(
        banners(&h).contains(&t("workspace.open.not_a_workspace")),
        "{:?}",
        banners(&h)
    );
    assert!(asked(&mut rx).is_empty(), "and no window is opened");
}

#[test]
#[serial]
fn an_interrupted_save_is_not_opened() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "half-saved");
    std::fs::remove_file(root.join(".dat0/manifest.json")).unwrap();
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), Some(root), boot);
    h.click("open");
    h.settle();
    assert!(
        banners(&h).contains(&t("workspace.open.incomplete.title")),
        "{:?}",
        banners(&h)
    );
    assert!(asked(&mut rx).is_empty());
}

#[test]
#[serial]
fn a_workspace_opens_in_a_window_of_its_own() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "own-window");
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), Some(root.clone()), boot);
    h.click("open");
    h.settle();
    assert_eq!(
        asked(&mut rx),
        [Opening::Workspace {
            root,
            networked: false
        }]
    );
}

#[test]
#[serial]
fn a_workspace_open_in_this_process_is_not_opened_twice() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "held");
    let (boot, mut rx) = boot();
    let mut holder = mount(
        Opening::Workspace {
            root: root.clone(),
            networked: false,
        },
        None,
        boot.clone(),
    );
    assert!(pump(&mut holder, |h| column(h, 0).len() == 3));
    let mut other = mount(Opening::files(Vec::new()), Some(root.clone()), boot.clone());
    other.click("open");
    other.settle();
    assert!(
        asked(&mut rx).is_empty(),
        "no second window on the same workspace"
    );
    let held_by = uuid::Uuid::parse_str(&text(&holder, "window-id")).unwrap();
    assert_eq!(
        boot.windows.holding(&root),
        Some(held_by),
        "the window that has it is the one brought forward"
    );
}

#[test]
#[serial]
fn a_workspace_held_on_another_machine_is_opened_only_when_asked() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "synced");
    let foreign = dat0_core::workspace::lock_manifest::LockManifest {
        pid: 4242,
        hostname: "elsewhere.example".into(),
        started_at: dat0_core::time::now_epoch_secs(),
        dat0_version: "0.1.0".into(),
        tombstoned: false,
    };
    let lock_json = root.join(".dat0/lock.json");
    std::fs::write(&lock_json, serde_json::to_vec(&foreign).unwrap()).unwrap();
    // Every workspace is on a sync drive, as far as this test is concerned.
    let settings = std::fs::read_to_string(settings_file()).unwrap_or_default();
    std::fs::write(
        settings_file(),
        format!("{settings}\n[workspace]\ntreat_all_as_networked = true\n"),
    )
    .unwrap();

    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), Some(root.clone()), boot.clone());
    h.click("open");
    h.settle();
    std::fs::write(settings_file(), settings).unwrap();
    assert!(
        h.by_a11y_id("workspace-in-use").is_some(),
        "it asks first; banners: {:?}",
        banners(&h)
    );
    assert!(
        text(&h, "workspace-in-use-body").contains("elsewhere.example"),
        "naming the machine"
    );
    assert!(asked(&mut rx).is_empty(), "and opens nothing yet");

    h.click("workspace-in-use-proceed");
    let opening = Opening::Workspace {
        root: root.clone(),
        networked: true,
    };
    assert_eq!(
        asked(&mut rx),
        std::slice::from_ref(&opening),
        "Open anyway opens it"
    );

    // The window it opens records itself as the holder.
    drop(h);
    let mut w = mount(opening, None, boot);
    assert!(pump(&mut w, |h| column(h, 0).len() == 3));
    let now: dat0_core::workspace::lock_manifest::LockManifest =
        serde_json::from_slice(&std::fs::read(&lock_json).unwrap()).unwrap();
    assert_eq!(now.pid, std::process::id());
    assert_eq!(now.hostname, dat0_core::workspace::identity::hostname());
}

#[test]
#[serial]
fn resume_finishes_an_interrupted_save_and_opens_the_workspace() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "resumed");
    std::fs::remove_file(root.join(".dat0/manifest.json")).unwrap();
    {
        let recents = dat0_core::globals::recents().unwrap();
        let mut recents = recents.lock().unwrap();
        recents
            .push(dat0_core::recents::RecentEntry::Workspace { path: root.clone() })
            .unwrap();
    }

    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), None, boot);
    assert!(pump(&mut h, |h| h
        .by_a11y_id("do-recovery.review")
        .is_some()));
    h.click(&format!("do-{}", ids::RECOVERY_REVIEW));
    let resume = (0..64)
        .map(|i| format!("recovery-resume-{i}"))
        .find(|id| h.by_a11y_id(id).is_some())
        .expect("the interrupted save is offered for Resume");
    h.click(&resume);
    assert!(
        root.join(".dat0/manifest.json").is_file(),
        "the save is finished"
    );
    assert_eq!(
        asked(&mut rx),
        [Opening::Workspace {
            root,
            networked: false
        }],
        "and the workspace opened"
    );
}

#[test]
#[serial]
fn the_demo_opens_as_a_workspace() {
    let rt = runtime();
    let _guard = rt.enter();
    let _ = STATE_ROOT.as_path();
    // The demo card is on the first-run hero.
    first_run_done(false);
    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), None, boot);
    assert!(pump(&mut h, |h| h.by_a11y_id("hero-open-demo").is_some()));
    first_run_done(true);
    h.click("hero-open-demo");
    let mut opened = Vec::new();
    for _ in 0..4800 {
        h.settle();
        opened.extend(asked(&mut rx));
        if !opened.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    match &opened[..] {
        [Opening::Workspace { root, .. }] => {
            // Unpacked into a fresh folder under the state root.
            assert!(root.join(".dat0/manifest.json").is_file(), "{root:?}");
            assert_eq!(
                root.file_name().and_then(|n| n.to_str()),
                Some("demo"),
                "so its window is named \"demo\""
            );
            assert!(root.starts_with(std::fs::canonicalize(STATE_ROOT.join("demo")).unwrap()));
        }
        other => panic!("expected the demo's workspace, got {other:?}"),
    }
}

#[test]
#[serial]
fn a_recent_workspace_on_the_hero_opens_as_a_workspace() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = workspace(&rt, "recent-on-hero");
    {
        let recents = dat0_core::globals::recents().unwrap();
        let mut recents = recents.lock().unwrap();
        recents
            .push(dat0_core::recents::RecentEntry::Workspace { path: root.clone() })
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
        [Opening::Workspace {
            root,
            networked: false
        }],
        "opened as a workspace, not refused as a file: {:?}",
        banners(&h)
    );
}
