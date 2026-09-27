//! Sorting and filtering a column without a pointer (PD-040).
//!
//! A column's sort and funnel zones were the only way to sort or filter it:
//! spans no keyboard could reach, and neither the palette nor the grid's
//! context menu could do either. These tests mount the real `Shell` over a
//! real session, put the grid's cursor on a column, and sort and filter it
//! through the commands, and through the context menu opened from the
//! keyboard.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::html::input_data::keyboard_types::Key;
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

const COMMANDS: &[&str] = &[ids::VIEW_SORT_ASC, ids::VIEW_SORT_DESC, ids::VIEW_FILTER];

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

/// A window over `stock.csv`, with its rows painted and the grid's cursor
/// on the `qty` column.
fn window(dir: &str) -> Harness {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let csv = dir.join("stock.csv");
    std::fs::write(&csv, "name,qty\nb,2\na,3\nc,1\n").expect("write csv");
    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: vec![csv],
        },
    );
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "the CSV never painted: {:?}",
        column(&h, 0)
    );
    h.key_at("grid-viewport", Key::ArrowRight, Modifiers::empty());
    h
}

fn perform(h: &mut Harness, id: &str) {
    h.click(&format!("do-{id}"));
}

#[test]
#[serial]
fn the_sort_commands_sort_the_column_under_the_cursor() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("sorted");

    perform(&mut h, ids::VIEW_SORT_DESC);
    assert!(
        pump(&mut h, |h| column(h, 0) == ["a", "b", "c"]),
        "by qty, highest first: {:?}",
        column(&h, 0)
    );
    assert_eq!(text(&h, "col-sort-1"), "▼", "the header says so");

    perform(&mut h, ids::VIEW_SORT_ASC);
    assert!(
        pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]),
        "lowest first: {:?}",
        column(&h, 0)
    );
    assert_eq!(text(&h, "col-sort-1"), "▲");
}

#[test]
#[serial]
fn the_filter_command_opens_the_cursors_columns_filter() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("filtered");

    perform(&mut h, ids::VIEW_FILTER);
    let want = format!("{}: qty", dat0_i18n::t("grid.filter"));
    assert!(
        pump(&mut h, |h| h
            .by_a11y_id("filter-popover")
            .and_then(|k| h.attr(k, "aria-label"))
            .is_some_and(|l| l == want)),
        "qty's filter, as its funnel would open it"
    );
}

#[test]
#[serial]
fn the_context_menu_opens_from_the_keyboard_and_sorts() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("menu");

    // Shift+F10, where a keyboard has no menu key.
    h.key_at("grid-viewport", Key::F10, Modifiers::SHIFT);
    assert!(
        pump(&mut h, |h| h.by_a11y_id("context-menu").is_some()),
        "the menu opens without a pointer"
    );
    // Sort Ascending is third from the end; Up wraps from the first item.
    for _ in 0..3 {
        h.key_at("context-menu", Key::ArrowUp, Modifiers::empty());
    }
    h.key_at("context-menu", Key::Enter, Modifiers::empty());
    assert!(
        pump(&mut h, |h| column(h, 0) == ["c", "b", "a"]),
        "Sort Ascending, on the cursor's column: {:?}",
        column(&h, 0)
    );
    assert!(h.by_a11y_id("context-menu").is_none(), "and the menu went");

    // The menu key opens it the same way.
    h.key_at("grid-viewport", Key::ContextMenu, Modifiers::empty());
    assert!(pump(&mut h, |h| h.by_a11y_id("context-menu").is_some()));
}
