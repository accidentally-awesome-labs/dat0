//! The status bar says what the window is doing (PD-023, step 5.8c): the
//! engine, the memory, the rows in view, the selection and the last query.
//!
//! It said `engine duckdb · native` whatever the engine was doing, and its
//! memory and rows segments were fields nothing wrote, so they never showed;
//! nothing reported the selection or a query at all. This mounts the real
//! shell over a real session, runs a query into the grid, selects cells and
//! reads the bar. The engine's `motherduck` and the title bar's `live` are
//! asserted where MotherDuck connects, in `tests/connections_flow.rs`.

mod support;

use std::path::PathBuf;
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
use support::{Harness, Modifiers};

/// A state root and a config dir of the binary's own, the first run behind.
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

/// A thousand rows, one column.
const QUERY: &str = "SELECT range AS n FROM range(1000)";

/// `App`'s wiring, minus what needs a desktop window, and buttons that seed
/// the console's history and route its commands.
#[component]
fn Host() -> Element {
    Theme::provide(None);
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_hook(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    });
    use_context_provider(|| boot.registry.clone());
    session_boot::use_session(ws, Vec::new());
    let events = use_window_bus(boot, ws, surface);
    let ready = ws.session.read().ready().is_some();
    rsx! {
        if ready {
            div { "data-a11y-id": "session-ready" }
        }
        button {
            "data-a11y-id": "seed",
            onclick: move |_| {
                let slot = ws.session.peek().ready().cloned().expect("session ready");
                let entry = HistoryEntry {
                    sql: QUERY.to_string(),
                    ran_at: 0,
                    ok: true,
                    elapsed_ms: 0,
                };
                slot.lock().set_query_history(vec![entry]).expect("seed history");
            },
        }
        for id in [ids::SQL_HISTORY, ids::SQL_RUN] {
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

fn engine(state: &str) -> String {
    format!("{} · {}", t("status.engine"), t(state))
}

fn mouse(mods: Modifiers) -> dioxus::html::SerializedMouseData {
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    let at = Coordinates::new(
        ScreenPoint::new(0.0, 0.0),
        ClientPoint::new(0.0, 0.0),
        ElementPoint::new(0.0, 0.0),
        PagePoint::new(0.0, 0.0),
    );
    dioxus::html::SerializedMouseData::new(
        Some(MouseButton::Primary),
        MouseButton::Primary.into(),
        at,
        mods,
    )
}

/// A click on a cell, as the grid takes one: down on the cell, up anywhere.
fn press(h: &mut Harness, row: usize, col: usize, mods: Modifiers) {
    let cell = h
        .by_a11y_id(&format!("cell-{row}-{col}"))
        .unwrap_or_else(|| panic!("no cell {row},{col}"));
    h.dispatch(cell, "mousedown", mouse(mods));
    let vp = h.by_a11y_id("grid-viewport").expect("the grid");
    h.dispatch(vp, "mouseup", mouse(Modifiers::empty()));
    h.settle();
}

#[test]
#[serial]
fn the_status_bar_says_what_the_window_is_doing() {
    let rt = runtime();
    let _guard = rt.enter();
    let _ = STATE_ROOT.as_path();

    let mut h = Harness::new(Host, ());
    h.settle();
    let first = text(&h, "status-engine");
    assert!(
        first == engine("status.engine.starting") || first == engine("status.engine.native"),
        "the engine says what it is doing, not `native` regardless: {first:?}"
    );
    assert!(
        pump(&mut h, |h| has(h, "session-ready")),
        "the session opens"
    );
    assert!(
        pump(&mut h, |h| text(h, "status-engine")
            == engine("status.engine.native")),
        "{:?}",
        text(&h, "status-engine")
    );

    // The resident set, sampled from the first frame.
    assert!(
        pump(&mut h, |h| {
            let mem = text(h, "status-mem");
            mem.starts_with("mem ") && mem.ends_with(" MB") && mem != "mem 0 MB"
        }),
        "{:?}",
        text(&h, "statusbar")
    );

    // Nothing open: no rows, no selection, no query.
    for id in ["status-rows", "status-selection", "status-query"] {
        assert!(
            !has(&h, id),
            "{id} with nothing open: {:?}",
            text(&h, "statusbar")
        );
    }

    // A query into the grid.
    h.click("seed");
    h.click(&format!("do-{}", ids::SQL_HISTORY));
    h.settle();
    h.click("hist-row-0");
    h.settle();
    h.click(&format!("do-{}", ids::SQL_RUN));
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "0"),
        "the rows never reached the grid: {:?}",
        text(&h, "console-error-text")
    );
    assert!(
        pump(&mut h, |h| {
            let q = text(h, "status-query");
            q.starts_with(&format!("{} ", t("status.query"))) && q.ends_with(" ms")
        }),
        "the last query and its time: {:?}",
        text(&h, "statusbar")
    );
    assert!(
        pump(&mut h, |h| {
            let rows = text(h, "status-rows");
            rows.starts_with("rows 1–") && rows.ends_with(" / 1,000")
        }),
        "the rows in view and the table's size: {:?}",
        text(&h, "statusbar")
    );

    // One cell, then a block of three.
    press(&mut h, 0, 0, Modifiers::empty());
    assert!(
        pump(&mut h, |h| text(h, "status-selection") == "1 cell selected"),
        "{:?}",
        text(&h, "statusbar")
    );
    press(&mut h, 2, 0, Modifiers::SHIFT);
    assert!(
        pump(&mut h, |h| text(h, "status-selection")
            == "3 cells selected"),
        "{:?}",
        text(&h, "statusbar")
    );
}
