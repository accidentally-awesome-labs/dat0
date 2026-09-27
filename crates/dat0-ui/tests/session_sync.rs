//! A window's work survives it (step 5.7b).
//!
//! The Dioxus shell recorded a file's tab in the session when it opened, and
//! nothing after that: not a sort, not an edit, not a line of SQL. The
//! recovery panel's Open threw its answer away, and nothing restored a
//! session's tabs into a window anyway (PD-023). These tests mount the real
//! `Shell` over a real session, record into it, and open a second window on
//! what the first one left behind.

mod support;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, Opening};
use dat0_core::session::queries::HistoryEntry;
use dat0_engine::{SortDirection, SortKey, Transformation};
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// Process-global state root and config dir, leaked so a session never reads a
/// deleted directory mid-test. The shape `grid_edits.rs` uses.
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
    std::mem::forget(tmp);
    root
});

const COMMANDS: &[&str] = &[
    ids::SQL_HISTORY,
    ids::SQL_RUN,
    ids::LIVE_REFRESH,
    ids::RECOVERY_REVIEW,
];

#[derive(Clone, Props)]
struct HostProps {
    opening: Opening,
    /// Put in the session's history by the `seed` button: the one way SQL
    /// reaches a query tab without a real editor to type into.
    history: Vec<String>,
    /// The process bus, handed to the test: what the window asks for there
    /// is read back rather than performed.
    boot: Boot,
}

/// Props never change after mount; the boot is compared by what it opens.
impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        (&self.opening, &self.history) == (&other.opening, &other.history)
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
    rsx! {
        div { "data-a11y-id": "window-id", "{ws.window_id}" }
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
/// what the window posts there stays for the test.
fn boot() -> (Boot, dat0_core::events::AppEventRx) {
    let reg = ActionRegistry::new();
    register_all(&reg).expect("built-in actions register");
    let boot = Boot::new(reg, Vec::new());
    let rx = boot.rx.lock().take().expect("the process bus");
    (boot, rx)
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

fn mount(opening: Opening, history: &[&str], boot: Boot) -> Harness {
    Harness::new(
        Host,
        HostProps {
            opening,
            history: history.iter().map(|s| s.to_string()).collect(),
            boot,
        },
    )
}

/// The scratch directory of the window `h` shows.
fn scratch_of(h: &Harness) -> PathBuf {
    STATE_ROOT.join("scratch").join(text(h, "window-id"))
}

fn session_at(dir: &Path) -> Option<dat0_core::session::SessionState> {
    dat0_core::session::migrate::load(&dir.join("session.json")).ok()
}

fn asc(column: &str) -> Transformation {
    Transformation::Sort {
        keys: vec![SortKey {
            column: column.into(),
            direction: SortDirection::Asc,
        }],
    }
}

/// A window over `stock.csv` in `dir`, sorted by quantity, with `sql` open in
/// a second query tab, and all of it recorded. Returns the window, the CSV
/// and the window's scratch directory.
fn working_window(dir: &str, sql: &str) -> (Harness, PathBuf, PathBuf) {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let csv = dir.join("stock.csv");
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\n").expect("write csv");
    let (boot, _rx) = boot();
    let mut h = mount(Opening::files(vec![csv.clone()]), &[sql], boot);
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "the CSV never painted"
    );
    h.click("seed");
    h.click("col-sort-1");
    assert!(pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]));
    h.click(&format!("do-{}", ids::SQL_HISTORY));
    h.click("hist-row-0");

    let scratch = scratch_of(&h);
    let recorded = pump(&mut h, |_| {
        session_at(&scratch).is_some_and(|s| {
            s.tabs
                .first()
                .is_some_and(|t| t.transform_stack == [asc("qty")])
                && s.sql_tabs.iter().any(|q| q.sql == sql)
        })
    });
    assert!(
        recorded,
        "never recorded: {:?}",
        session_at(&scratch).map(|s| s.tabs)
    );
    (h, csv, scratch)
}

#[test]
#[serial]
fn a_window_records_its_tabs_their_views_and_its_sql() {
    let rt = runtime();
    let _guard = rt.enter();
    let (_h, csv, scratch) = working_window("recorded", "SELECT 7 AS seven");

    let s = session_at(&scratch).expect("session.json");
    assert_eq!(s.tabs.len(), 1);
    assert_eq!(s.tabs[0].table_name, "stock");
    assert_eq!(s.tabs[0].source_path.as_deref(), Some(csv.as_path()));
    assert_eq!(s.tabs[0].undo_cursor, 1);
    assert_eq!(s.active_tab, Some(0));
    let titles: Vec<&str> = s.sql_tabs.iter().map(|q| q.title.as_str()).collect();
    assert_eq!(titles, ["Query 1", "Query 2"]);
    assert_eq!(s.active_sql_tab, Some(1), "the tab the history pick opened");
    assert!(
        !s.nothing_to_recover(),
        "a sorted view and typed SQL are worth recovering"
    );
}

#[test]
#[serial]
fn a_recovered_session_comes_back_with_its_tabs_views_and_sql() {
    let rt = runtime();
    let _guard = rt.enter();
    let (first, csv, scratch) = working_window("recovered", "SELECT 7 AS seven");
    // The window closes; what it left is an orphan now.
    drop(first);
    assert!(!dat0_core::globals::is_live_scratch_dir(&scratch));

    let (boot, _rx) = boot();
    let mut h = mount(
        Opening::Recover {
            dir: scratch.clone(),
        },
        &[],
        boot,
    );
    assert!(
        pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]),
        "the tab, sorted as it was: {:?}",
        column(&h, 0)
    );
    assert_eq!(text(&h, "col-sort-1"), "▲", "and its header says so");
    assert!(
        dat0_core::globals::is_live_scratch_dir(&scratch),
        "open again, so neither offered for recovery nor swept"
    );

    // The console's SQL came back too: running its active tab runs it.
    h.click(&format!("do-{}", ids::SQL_RUN));
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "7"),
        "the recovered SQL never ran: {:?}",
        text(&h, "console-error-text")
    );

    // The tab still knows its file: reading it again replaces its own table
    // rather than importing beside it, and keeps the view.
    h.click("tab-0");
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\nd,0\n").unwrap();
    h.click(&format!("do-{}", ids::LIVE_REFRESH));
    assert!(
        pump(&mut h, |h| column(h, 0) == ["d", "c", "b", "a"]),
        "refreshed into its own table: {:?}; banners: {:?}",
        column(&h, 0),
        text(&h, "banner-host")
    );
}

#[test]
#[serial]
fn the_recovery_panel_opens_a_session_in_a_window_of_its_own() {
    let rt = runtime();
    let _guard = rt.enter();
    let (first, _csv, scratch) = working_window("offered", "SELECT 1");
    drop(first);
    // Other tests' windows left sessions too: this one is the one offered.
    for e in std::fs::read_dir(STATE_ROOT.join("scratch"))
        .unwrap()
        .flatten()
    {
        if e.path() != scratch {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }

    let (boot, mut rx) = boot();
    let mut h = mount(Opening::files(Vec::new()), &[], boot);
    let own = scratch_of(&h);
    assert!(pump(&mut h, |_| own.join("session.json").is_file()));
    h.click(&format!("do-{}", ids::RECOVERY_REVIEW));
    assert!(
        text(&h, "recovery-row-0").contains("stock"),
        "the orphan, labelled by its table: {:?}",
        text(&h, "recovery")
    );
    assert!(
        h.by_a11y_id("recovery-row-1").is_none(),
        "and not the window that is open"
    );

    h.click("recovery-open-0");
    let asked: Vec<Opening> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|ev| match ev {
            AppEvent::OpenWindow(opening) => Some(opening),
            _ => None,
        })
        .collect();
    assert_eq!(
        asked,
        [Opening::Recover { dir: scratch }],
        "Open asks for a window on the session"
    );
    assert!(h.by_a11y_id("recovery").is_none(), "and the panel closes");
}
