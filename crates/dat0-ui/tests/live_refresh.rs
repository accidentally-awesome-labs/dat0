//! Live Refresh (step 5.7a): read a tab's file again and keep its view.
//!
//! The command opened its dialog with zero edits and zero deletes, whatever
//! the view held, threw the answer away, and nothing watched the file
//! (PD-023). These tests mount the real `Shell` over a real session, open a
//! CSV, change it on disk, and read the grid back.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
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

const COMMANDS: &[&str] = &[ids::LIVE_REFRESH];

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    cli_paths: Vec<PathBuf>,
}

/// `App`'s wiring, minus what needs a desktop window.
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

/// A window over `stock.csv` in its own folder, with its rows painted.
fn window(dir: &str) -> (Harness, PathBuf) {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let csv = dir.join("stock.csv");
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\n").expect("write csv");
    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: vec![csv.clone()],
        },
    );
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "the CSV never painted: {:?}",
        column(&h, 0)
    );
    (h, csv)
}

fn refresh(h: &mut Harness) {
    h.click(&format!("do-{}", ids::LIVE_REFRESH));
}

fn banner_says(h: &Harness, key: &str) -> bool {
    h.has_label_contains(&dat0_i18n::t(key))
}

#[test]
#[serial]
fn refresh_reads_the_file_again_and_keeps_the_sort() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, csv) = window("sorted");

    h.click("col-sort-1");
    assert!(pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]));

    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\nd,0\n").unwrap();
    refresh(&mut h);
    assert!(
        pump(&mut h, |h| column(h, 0) == ["d", "c", "b", "a"]),
        "the new row, in the sort that was on: {:?}",
        column(&h, 0)
    );
    assert_eq!(text(&h, "col-sort-1"), "▲", "the sort is still on");
}

#[test]
#[serial]
fn refresh_asks_before_it_drops_an_edit() {
    use dioxus::html::input_data::keyboard_types::Key;
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, csv) = window("edited");

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

    std::fs::write(&csv, "name,qty\nb,2\na,4\nc,1\n").unwrap();
    refresh(&mut h);
    assert!(
        h.by_a11y_id("live-refresh").is_some(),
        "an edit would be lost, so it asks first"
    );
    assert!(
        text(&h, "live-refresh-body").contains('1'),
        "and says how many: {:?}",
        text(&h, "live-refresh-body")
    );
    h.click("live-refresh-confirm");
    assert!(
        pump(&mut h, |h| column(h, 1) == ["2", "4", "1"]),
        "the file's value, the edit dropped: {:?}",
        column(&h, 1)
    );
}

#[test]
#[serial]
fn a_filter_on_a_column_the_file_lost_lands_on_the_bare_table() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, csv) = window("drifted");

    h.click("col-sort-0");
    assert!(pump(&mut h, |h| column(h, 0) == ["a", "b", "c"]));

    // `name` is gone from the file: the sort on it cannot replay.
    std::fs::write(&csv, "label,qty\nx,2\ny,3\n").unwrap();
    refresh(&mut h);
    assert!(
        pump(&mut h, |h| column(h, 0) == ["x", "y"]),
        "the file's rows, unsorted: {:?}",
        column(&h, 0)
    );
    assert!(banner_says(&h, "livedata.replay.schema_drift"));
}

#[test]
#[serial]
fn a_file_changed_on_disk_offers_a_refresh() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, csv) = window("watched");
    let title = dat0_i18n::t("livedata.changed.title").replace("{file}", "stock.csv");

    // Rewrite until the watcher, started when the tab became active, sees
    // it: writing before it is up would be missed.
    let mut seen = false;
    for i in 0..20 {
        std::fs::write(&csv, format!("name,qty\nb,2\na,3\nc,1\ne,{i}\n")).unwrap();
        if pump_for(&mut h, Duration::from_secs(1), |h| {
            h.has_label_contains(&title)
        }) {
            seen = true;
            break;
        }
    }
    assert!(seen, "the change was never reported");

    // The banner's own button refreshes.
    let slot = (0..8)
        .find(|i| {
            h.by_a11y_id(&format!("banner-{i}"))
                .and_then(|b| h.attr(b, "aria-label"))
                .is_some_and(|l| l == title)
        })
        .expect("the banner's slot");
    h.click(&format!("banner-{slot}-act-primary"));
    assert!(
        pump(&mut h, |h| column(h, 0).len() == 4),
        "refreshed: {:?}",
        column(&h, 0)
    );
    assert!(
        !h.has_label_contains(&title),
        "and the banner goes once it has been acted on"
    );
}

fn pump_for(h: &mut Harness, for_: Duration, done: impl Fn(&Harness) -> bool) -> bool {
    let until = std::time::Instant::now() + for_;
    while std::time::Instant::now() < until {
        h.settle();
        if done(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}
