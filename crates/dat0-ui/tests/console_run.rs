//! The SQL console runs queries (step 5.2).
//!
//! Until this, Run and Cancel only cleared the error strip, History and Load
//! opened empty dialogs, and both save prompts threw their answer away (PD-023)
//! — six commands the palette had to hide. These tests mount the real `Shell`
//! over a real session and drive the console the way a user reaches it: SQL
//! arrives in a query tab from the history list, and every command is routed,
//! as the palette and the menu bar route it.

mod support;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::session::queries::HistoryEntry;
use dat0_i18n::t;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// Process-global state root and config dir, leaked so a session never reads a
/// deleted directory mid-test. The shape `shell_grid_binding.rs` uses.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("state");
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&root).expect("mkdir state");
    std::fs::create_dir_all(&cfg).expect("mkdir cfg");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", &cfg) };
    // Without this the shell auto-opens the first-run tour over the grid.
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(cfg.join("settings.toml")),
        true,
    )
    .expect("seed first_run_done");
    dat0_core::globals::install_state_root(root.clone());
    std::mem::forget(tmp);
    root
});

fn state_root() -> &'static Path {
    STATE_ROOT.as_path()
}

const COMMANDS: &[&str] = &[
    ids::CONSOLE_TOGGLE,
    ids::SQL_RUN,
    ids::SQL_CANCEL,
    ids::SQL_HISTORY,
    ids::SQL_LOAD_QUERY,
    ids::SQL_SAVE_QUERY,
    ids::SQL_SAVE_AS_TABLE,
];

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    /// Put in the session's history once it opens: the one way SQL reaches a
    /// query tab without a real editor to type into.
    history: Vec<String>,
}

/// `App`'s wiring, minus what needs a desktop window.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let mut ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_hook(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    });
    use_context_provider(|| boot.registry.clone());
    session_boot::use_session(ws, Vec::new());
    let events = use_window_bus(boot, ws, surface);

    let seed = props.history.clone();
    let ready = ws.session.read().ready().is_some();
    rsx! {
        if ready {
            div { "data-a11y-id": "session-ready" }
        }
        button {
            "data-a11y-id": "seed",
            onclick: move |_| {
                let slot = ws.session.peek().ready().cloned().expect("session ready");
                let mut session = slot.lock();
                let entries = seed
                    .iter()
                    .map(|sql| HistoryEntry { sql: sql.clone(), ran_at: 0, ok: true, elapsed_ms: 0 })
                    .collect();
                session.set_query_history(entries).expect("seed history");
            },
        }
        button {
            "data-a11y-id": "read-only",
            onclick: move |_| ws.read_only.set(true),
        }
        for id in COMMANDS.iter().copied() {
            button {
                key: "{id}",
                "data-a11y-id": "do-{id}",
                onclick: {
                    let events = events.clone();
                    move |_| {
                        assert!(route(ws, &events, surface, id), "{id} was not routed");
                    }
                },
            }
        }
        Shell {}
    }
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

fn has(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id).is_some()
}

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

fn perform(h: &mut Harness, id: &str) {
    h.click(&format!("do-{id}"));
    h.settle();
}

/// Mount a window whose history holds `sql`, newest last, and wait for its
/// session.
fn window(history: &[&str]) -> Harness {
    let _ = state_root();
    let mut h = Harness::new(
        Host,
        HostProps {
            history: history.iter().map(|s| s.to_string()).collect(),
        },
    );
    assert!(
        pump(&mut h, |h| has(h, "session-ready")),
        "the session never opened"
    );
    h.click("seed");
    h
}

/// Open history entry `i` (newest first, as the list shows it) in a new
/// query tab.
fn load(h: &mut Harness, i: usize) {
    perform(h, ids::SQL_HISTORY);
    h.click(&format!("hist-row-{i}"));
    h.settle();
}

fn type_into(h: &mut Harness, id: &str, value: &str) {
    let field = h.by_a11y_id(id).expect("the field");
    h.dispatch(
        field,
        "input",
        dioxus::html::SerializedFormData::new(value.to_string(), Vec::new()),
    );
    h.settle();
}

fn banners(h: &Harness) -> String {
    text(h, "banner-host")
}

#[test]
#[serial]
fn a_query_lands_in_the_grid_as_a_tab_named_for_its_query_tab() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["SELECT 42 AS answer"]);

    load(&mut h, 0);
    perform(&mut h, ids::SQL_RUN);

    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "42"),
        "the result never reached the grid; tabs: {:?}, console error: {:?}",
        text(&h, "tabstrip"),
        text(&h, "console-error-text"),
    );
    // `Tabs::new` opens "Query 1"; the history pick opened "Query 2".
    assert_eq!(text(&h, "tab-0"), "Query 2", "named for its query tab");
    assert!(!has(&h, "tab-1"));

    // Running it again replaces the rows in place rather than adding a tab.
    // The console is open so the Run chip shows when the run is over.
    perform(&mut h, ids::CONSOLE_TOGGLE);
    perform(&mut h, ids::SQL_RUN);
    assert!(
        pump(&mut h, |h| has(h, "console-run")),
        "the rerun never finished"
    );
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "42"),
        "the rerun's rows never painted"
    );
    assert!(
        !has(&h, "tab-1"),
        "a second run of the same tab adds no tab"
    );

    // Both runs are in the history, after the seeded entry.
    perform(&mut h, ids::SQL_HISTORY);
    assert!(
        has(&h, "hist-row-2"),
        "history: {}",
        text(&h, "sql-history-list")
    );
}

#[test]
#[serial]
fn a_failing_query_says_why_in_the_console() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["SELECT * FROM no_such_table"]);

    load(&mut h, 0);
    perform(&mut h, ids::SQL_RUN);

    assert!(
        pump(&mut h, |h| has(h, "console-error")),
        "no error strip; the console was not opened to show it"
    );
    let why = text(&h, "console-error-text");
    assert!(
        why.contains("no_such_table"),
        "DuckDB's own message: {why:?}"
    );
    assert!(
        !why.contains("GridDataSource"),
        "not the wrapper chain: {why:?}"
    );
    assert!(
        !why.contains("__dat0_qr_"),
        "and not the view dat0 wraps the query in: {why:?}"
    );
    assert!(has(&h, "console-run"), "the run is over, so Run is back");
}

#[test]
#[serial]
fn a_statement_that_returns_no_rows_says_it_ran() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["CREATE TABLE made_here AS SELECT 1 AS x"]);

    load(&mut h, 0);
    perform(&mut h, ids::SQL_RUN);

    assert!(
        pump(&mut h, |h| banners(h).contains(&t("sql.exec.done"))),
        "no word that it ran; banners: {:?}",
        banners(&h)
    );
    assert!(!has(&h, "grid"), "and no grid: nothing came back");
}

#[test]
#[serial]
fn cancel_interrupts_a_running_query() {
    let rt = runtime();
    let _guard = rt.enter();
    // A hundred billion pairs: long enough that nothing finishes it before
    // Cancel does, short enough that a broken Cancel fails the test instead of
    // hanging it.
    let mut h = window(&[
        "SELECT count(*) FROM range(100000) a, range(1000000) b WHERE (a.range + b.range) % 7 = 3",
    ]);

    load(&mut h, 0);
    perform(&mut h, ids::CONSOLE_TOGGLE);
    perform(&mut h, ids::SQL_RUN);
    assert!(has(&h, "console-cancel"), "Run became Cancel while it runs");

    // Let the run reach the count, so this is DuckDB's interrupt at work and
    // not only the check between the run's two steps.
    for _ in 0..40 {
        h.settle();
        std::thread::sleep(Duration::from_millis(25));
    }
    perform(&mut h, ids::SQL_CANCEL);
    assert!(
        pump(&mut h, |h| has(h, "console-run")),
        "the query was never interrupted"
    );
    assert_eq!(text(&h, "console-error-text"), t("sql.cancelled"));
}

#[test]
#[serial]
fn a_saved_query_can_be_loaded_and_deleted() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["SELECT 7 AS lucky"]);

    load(&mut h, 0);
    perform(&mut h, ids::SQL_SAVE_QUERY);
    type_into(&mut h, "name-prompt-field", "lucky");
    h.click("name-prompt-ok");
    h.settle();
    assert!(
        banners(&h).contains(&t("sql.query_saved")),
        "banners: {:?}",
        banners(&h)
    );
    assert!(
        banners(&h).contains(&t("workspace.prompt.title")),
        "a saved query is work worth keeping, so the window suggests saving it"
    );

    perform(&mut h, ids::SQL_LOAD_QUERY);
    assert!(text(&h, "saved-row-0").contains("lucky"));
    h.click("saved-row-0");
    h.settle();
    assert!(!has(&h, "saved-queries"), "a pick closes the picker");

    // The pick opened a third query tab; running it proves it carries the SQL.
    perform(&mut h, ids::SQL_RUN);
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "7"),
        "the loaded query did not run; error: {:?}",
        text(&h, "console-error-text")
    );
    assert_eq!(text(&h, "tab-0"), "Query 3");

    perform(&mut h, ids::SQL_LOAD_QUERY);
    h.click("saved-del-0");
    h.settle();
    assert!(
        has(&h, "saved-empty"),
        "the picker shows the list without it"
    );
}

#[test]
#[serial]
fn save_as_table_keeps_the_statement_as_a_table() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["SELECT 1 AS one, 2 AS two"]);

    load(&mut h, 0);
    perform(&mut h, ids::SQL_SAVE_AS_TABLE);
    type_into(&mut h, "name-prompt-field", "kept");
    h.click("name-prompt-ok");

    assert!(
        pump(&mut h, |h| text(h, "cell-0-1") == "2"),
        "the new table never opened; error: {:?}",
        text(&h, "console-error-text")
    );
    assert_eq!(text(&h, "tab-0"), "kept");
    assert!(banners(&h).contains(&t("sql.table_saved")));
}

#[test]
#[serial]
fn a_read_only_workspace_runs_only_queries_that_return_rows() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["CREATE TABLE nope AS SELECT 1"]);

    h.click("read-only");
    load(&mut h, 0);
    perform(&mut h, ids::SQL_RUN);
    assert_eq!(text(&h, "console-error-text"), t("sql.error.read_only"));
}

#[test]
#[serial]
fn an_empty_query_tab_says_there_is_nothing_to_run() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&[]);

    // From the palette, with the console shut: it opens to say why.
    perform(&mut h, ids::SQL_RUN);
    assert_eq!(text(&h, "console-error-text"), t("sql.error.empty"));
}

#[test]
#[serial]
fn describe_and_show_land_in_the_grid_too() {
    // DuckDB will not define a view as one of these; the run selects from it.
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["DESCRIBE SELECT 42 AS answer"]);

    load(&mut h, 0);
    perform(&mut h, ids::SQL_RUN);
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "answer"),
        "DESCRIBE never reached the grid; error: {:?}",
        text(&h, "console-error-text")
    );
}

#[test]
#[serial]
fn a_pragma_runs_and_says_its_rows_are_not_shown() {
    // Nor select from a PRAGMA: it runs as a statement, and says so rather
    // than dropping its rows without a word.
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window(&["PRAGMA version"]);

    load(&mut h, 0);
    perform(&mut h, ids::SQL_RUN);
    assert!(
        pump(&mut h, |h| banners(h).contains(&t("sql.exec.done"))),
        "the PRAGMA never ran; error: {:?}",
        text(&h, "console-error-text")
    );
    assert!(banners(&h).contains(&t("sql.exec.rows_hidden")));
}
