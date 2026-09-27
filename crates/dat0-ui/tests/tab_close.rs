//! Closing a data tab (PD-038).
//!
//! The tab strip only activated tabs and no command closed one, so a window's
//! tabs only accumulated. These tests mount the real `Shell` over a real
//! session, open two CSVs, close one, and read back what went with it — its
//! view — and what did not — its table.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::html::input_data::keyboard_types::Key;
use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_engine::QueryEngine as _;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// Process-global state root and config dir, leaked so a session never reads a
/// deleted directory mid-test. The shape `live_refresh.rs` uses.
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

const COMMANDS: &[&str] = &[ids::VIEW_CLOSE_TAB];

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    cli_paths: Vec<PathBuf>,
}

/// `App`'s wiring, minus what needs a desktop window, plus two readbacks the
/// harness cannot see on its own: the tabs the session has recorded, and the
/// tables its engine holds.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_hook(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    });
    use_context_provider(|| boot.registry.clone());
    session_boot::use_session(ws, props.cli_paths.clone());
    let events = use_window_bus(boot, ws, surface);

    let mut recorded = use_signal(String::new);
    let mut held = use_signal(String::new);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let Some(session) = ws.session.peek().ready().cloned() else {
                continue;
            };
            let (tabs, engine) = {
                let s = session.lock();
                let tabs: Vec<String> = s.tabs().iter().map(|t| t.table_name.clone()).collect();
                (tabs.join(","), s.engine.clone())
            };
            if *recorded.peek() != tabs {
                recorded.set(tabs);
            }
            let mut tables: Vec<String> = engine
                .get_tables()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.name)
                .collect();
            tables.sort();
            let tables = tables.join(",");
            if *held.peek() != tables {
                held.set(tables);
            }
        }
    });

    rsx! {
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
        // What the inspector's lineage and a saved chart do: bring a table's
        // tab up, opening one when it has none.
        button {
            "data-a11y-id": "reopen-alpha",
            onclick: move |_| ws.show_tab("alpha".to_string(), None),
        }
        div { "data-a11y-id": "recorded", "{recorded}" }
        div { "data-a11y-id": "held", "{held}" }
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

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

fn column(h: &Harness, col: usize) -> Vec<String> {
    (0..10)
        .map(|r| text(h, &format!("cell-{r}-{col}")))
        .take_while(|v| !v.is_empty())
        .collect()
}

/// The strip's tab titles, in order.
fn tabs(h: &Harness) -> Vec<String> {
    (0..)
        .map_while(|i| h.by_a11y_id(&format!("tab-{i}")).map(|k| h.text_of(k)))
        .collect()
}

/// A window over `alpha.csv` and `beta.csv` in their own folder, with alpha
/// showing.
fn window(dir: &str) -> Harness {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let alpha = dir.join("alpha.csv");
    std::fs::write(&alpha, "name,qty\nb,2\na,3\nc,1\n").expect("write alpha");
    let beta = dir.join("beta.csv");
    std::fs::write(&beta, "id\n7\n8\n").expect("write beta");
    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: vec![alpha, beta],
        },
    );
    assert!(
        pump(&mut h, |h| tabs(h) == ["alpha.csv", "beta.csv"]),
        "both files opened: {:?}",
        tabs(&h)
    );
    h.click("tab-0");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "alpha never painted: {:?}",
        column(&h, 0)
    );
    h
}

fn close_active(h: &mut Harness) {
    h.click(&format!("do-{}", ids::VIEW_CLOSE_TAB));
}

#[test]
#[serial]
fn a_closed_tab_leaves_its_table_in_the_session() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("kept");

    h.click("tab-close-0");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["7", "8"]),
        "beta took its place: {:?}",
        column(&h, 0)
    );
    assert_eq!(tabs(&h), ["beta.csv"]);
    assert!(
        pump(&mut h, |h| text(h, "recorded") == "beta"),
        "the session no longer records alpha's tab: {:?}",
        text(&h, "recorded")
    );
    assert!(
        text(&h, "held").split(',').any(|t| t == "alpha"),
        "and still holds its table: {:?}",
        text(&h, "held")
    );

    // So it can come back.
    h.click("reopen-alpha");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "alpha's rows, from the table that stayed: {:?}",
        column(&h, 0)
    );
}

#[test]
#[serial]
fn a_sort_closes_with_its_tab() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("sorted");

    h.click("col-sort-0");
    assert!(pump(&mut h, |h| column(h, 0) == ["a", "b", "c"]));

    // A sort is not work the table lacks: no dialog.
    close_active(&mut h);
    assert!(h.by_a11y_id("close-tab").is_none(), "nothing to confirm");
    assert!(
        pump(&mut h, |h| tabs(h) == ["beta.csv"]),
        "closed at once: {:?}",
        tabs(&h)
    );

    h.click("reopen-alpha");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "the view went with the tab, so the table comes back bare: {:?}",
        column(&h, 0)
    );
    assert!(
        h.by_a11y_id("pipeline-chip-0").is_none(),
        "no step is left over from the closed tab"
    );
}

#[test]
#[serial]
fn closing_asks_before_it_drops_an_edit() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("edited");

    // a's quantity, typed over.
    let vp = h.by_a11y_id("grid-viewport").unwrap();
    h.key(vp, Key::ArrowDown, Modifiers::empty());
    h.key(vp, Key::ArrowRight, Modifiers::empty());
    let vp = h.by_a11y_id("grid-viewport").unwrap();
    h.key(vp, Key::Enter, Modifiers::empty());
    let editor = h.by_a11y_id("cell-editor").expect("the editor");
    h.dispatch(
        editor,
        "input",
        dioxus::html::SerializedFormData::new("30".into(), Vec::new()),
    );
    h.key(editor, Key::Enter, Modifiers::empty());
    assert!(pump(&mut h, |h| column(h, 1) == ["2", "30", "1"]));

    close_active(&mut h);
    assert!(
        h.by_a11y_id("close-tab").is_some(),
        "an edit would be lost, so it asks first"
    );
    let body = text(&h, "close-tab-body");
    assert!(
        body.contains("alpha.csv") && body.contains('1'),
        "and says which tab, and how many: {body:?}"
    );

    h.click("close-tab-cancel");
    assert!(h.by_a11y_id("close-tab").is_none());
    assert_eq!(tabs(&h), ["alpha.csv", "beta.csv"], "Cancel keeps the tab");
    assert_eq!(column(&h, 1), ["2", "30", "1"], "and its edit");

    close_active(&mut h);
    h.click("close-tab-confirm");
    assert!(
        pump(&mut h, |h| tabs(h) == ["beta.csv"]),
        "closed: {:?}",
        tabs(&h)
    );

    h.click("reopen-alpha");
    assert!(
        pump(&mut h, |h| column(h, 1) == ["2", "3", "1"]),
        "the edit was the view's, and the table never had it: {:?}",
        column(&h, 1)
    );
}
