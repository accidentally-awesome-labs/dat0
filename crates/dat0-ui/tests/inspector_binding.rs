//! The inspector profiles the active tab (step 5.5d), and saved charts come
//! back from its lineage (step 5.5c).
//!
//! The inspector was built with nothing feeding it (PD-023): nothing set its
//! target, so it said "No table selected" whatever was open, its lineage was
//! never built, and a saved chart could not be shown again. These tests mount
//! the real `Shell` over a real session, open CSVs and read the inspector the
//! way a user does: its overview, its column cards and their small charts, its
//! two modes, and its lineage rows.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_i18n::t;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::{Harness, Modifiers};

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
    ids::INSPECTOR_TOGGLE,
    ids::CHART_VISUALIZE,
    ids::VIEW_DELETE_ROWS,
    ids::LIVE_REFRESH,
];

/// Forty sales: a region of two values, and forty amounts.
fn sales(rows: usize) -> String {
    let mut csv = String::from("region,amt\n");
    for i in 1..=rows {
        let region = if i % 2 == 0 { "EU" } else { "US" };
        csv.push_str(&format!("{region},{i}\n"));
    }
    csv
}

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
        // A chart as a package stores it: its source schema-qualified, the
        // form `ChartSpec` documents and the demo package carries.
        button {
            "data-a11y-id": "store-chart",
            onclick: move |_| {
                let slot = ws.session.peek().ready().cloned().expect("session ready");
                let mut session = slot.lock();
                let mut charts = session.charts().to_vec();
                dat0_core::session::charts::upsert_chart(
                    &mut charts,
                    dat0_core::session::charts::SavedChart {
                        id: uuid::Uuid::now_v7(),
                        name: "Stored".into(),
                        spec: dat0_engine::chart_spec::ChartSpec {
                            chart_type: dat0_engine::chart_spec::ChartType::Bar,
                            source: "\"main\".\"sales\"".into(),
                            x: Some("region".into()),
                            y: Some("amt".into()),
                            group: None,
                            color: None,
                            title: String::new(),
                        },
                        saved_at: 0,
                    },
                );
                session.set_charts(charts).expect("store the chart");
            },
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

fn has(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id).is_some()
}

fn overview(h: &Harness) -> String {
    text(h, "inspector-overview")
}

fn lineage(h: &Harness) -> String {
    text(h, "inspector-lineage")
}

fn perform(h: &mut Harness, id: &str) {
    h.click(&format!("do-{id}"));
}

fn type_into(h: &mut Harness, id: &str, value: &str) {
    let field = h.by_a11y_id(id).expect("the field");
    let typed = dioxus::html::SerializedFormData::new(value.to_string(), Vec::new());
    h.dispatch(field, "input", typed);
    h.settle();
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

/// Put the grid's cursor on `(row, col)`.
fn press(h: &mut Harness, row: usize, col: usize) {
    let cell = h
        .by_a11y_id(&format!("cell-{row}-{col}"))
        .unwrap_or_else(|| panic!("no cell {row},{col}"));
    h.dispatch(cell, "mousedown", mouse(Modifiers::empty()));
    let grid = h.by_a11y_id("grid-viewport").expect("the grid");
    h.dispatch(grid, "mouseup", mouse(Modifiers::empty()));
}

/// A window over `files`, written under `dir`, with its first grid painted.
fn window(dir: &str, files: &[(&str, String)]) -> (Harness, Vec<PathBuf>) {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let cli_paths: Vec<PathBuf> = files
        .iter()
        .map(|(name, body)| {
            let path = dir.join(name);
            std::fs::write(&path, body).expect("write csv");
            path
        })
        .collect();
    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: cli_paths.clone(),
        },
    );
    assert!(
        pump(&mut h, |h| !text(h, "cell-0-0").is_empty()),
        "no grid painted"
    );
    (h, cli_paths)
}

/// The tab strip's tab for `table`.
fn tab(h: &Harness, table: &str) -> String {
    (0..8)
        .map(|i| format!("tab-{i}"))
        .find(|id| text(h, id).contains(table))
        .unwrap_or_else(|| panic!("no tab for {table}"))
}

#[test]
#[serial]
fn the_inspector_profiles_the_active_tab_with_its_small_charts() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, _) = window("profiled", &[("sales.csv", sales(40))]);

    perform(&mut h, ids::INSPECTOR_TOGGLE);
    assert!(
        pump(&mut h, |h| overview(h) == "sales — 40 rows · 2 cols"),
        "overview: {:?}",
        overview(&h)
    );
    assert!(has(&h, "inspector-card-region") && has(&h, "inspector-card-amt"));
    assert!(
        !text(&h, "inspector-cards").contains("rowid"),
        "the row id is no one's column"
    );
    assert!(
        pump(&mut h, |h| has(h, "inspector-chart-region")
            && has(h, "inspector-chart-amt")),
        "a region's two values are drawn, and forty amounts as a histogram"
    );
    assert!(
        pump(&mut h, |h| lineage(h).contains("sales.csv")),
        "the table came from its file: {:?}",
        lineage(&h)
    );
}

#[test]
#[serial]
fn current_view_profiles_what_the_grid_shows_and_whole_table_the_table() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, _) = window("modes", &[("sales.csv", sales(40))]);
    perform(&mut h, ids::INSPECTOR_TOGGLE);
    assert!(pump(&mut h, |h| overview(h).contains("40 rows")));

    // A row deleted in the grid is a step laid over the table.
    press(&mut h, 0, 0);
    perform(&mut h, ids::VIEW_DELETE_ROWS);
    assert!(
        pump(&mut h, |h| text(h, "cell-0-1") == "2"),
        "row 1 is gone"
    );
    h.settle();
    assert!(
        overview(&h).contains("40 rows"),
        "the whole table still has it: {:?}",
        overview(&h)
    );

    h.click("inspector-mode-toggle");
    assert!(
        pump(&mut h, |h| overview(h).contains("39 rows")),
        "the view does not: {:?}",
        overview(&h)
    );
    assert_eq!(text(&h, "inspector-mode-toggle"), t("inspector.mode.view"));

    h.click("inspector-mode-toggle");
    assert!(
        pump(&mut h, |h| overview(h).contains("40 rows")),
        "{:?}",
        overview(&h)
    );
}

#[test]
#[serial]
fn it_follows_the_active_tab_and_reads_a_refreshed_file_again() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, paths) = window(
        "follows",
        &[
            ("sales.csv", sales(40)),
            ("stock.csv", "name,qty\nb,2\na,3\n".into()),
        ],
    );
    let (sales_tab, stock_tab) = (tab(&h, "sales"), tab(&h, "stock"));
    h.click(&sales_tab);
    perform(&mut h, ids::INSPECTOR_TOGGLE);
    assert!(pump(&mut h, |h| overview(h) == "sales — 40 rows · 2 cols"));

    h.click(&stock_tab);
    assert!(
        pump(&mut h, |h| overview(h) == "stock — 2 rows · 2 cols"),
        "{:?}",
        overview(&h)
    );
    h.click(&sales_tab);
    assert!(pump(&mut h, |h| overview(h) == "sales — 40 rows · 2 cols"));

    // The file grows, and Live Refresh reads it again: the profile kept for
    // the table is of rows it no longer has.
    std::fs::write(&paths[0], sales(41)).expect("the file grows");
    perform(&mut h, ids::LIVE_REFRESH);
    assert!(
        pump(&mut h, |h| overview(h) == "sales — 41 rows · 2 cols"),
        "{:?}",
        overview(&h)
    );
}

#[test]
#[serial]
fn a_saved_chart_is_in_its_tables_lineage_and_comes_back_from_it() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, _) = window("lineage", &[("sales.csv", sales(40))]);
    perform(&mut h, ids::INSPECTOR_TOGGLE);
    perform(&mut h, ids::CHART_VISUALIZE);
    assert!(pump(&mut h, |h| text(h, "pane-head-charts").contains("sales")));
    h.click("chart-axis-x");
    assert!(pump(&mut h, |h| text(h, "chart-axis-x").contains("region")));

    h.click("chart-save");
    type_into(&mut h, "name-prompt-field", "By region");
    h.click("name-prompt-ok");
    assert!(
        pump(&mut h, |h| has(h, "lineage-1-By region")),
        "the chart is drawn from the table: {:?}",
        lineage(&h)
    );

    // Change the chart, then take it back from the lineage.
    h.click("chart-type");
    h.click("chart-axis-x");
    assert!(pump(&mut h, |h| !text(h, "chart-axis-x").contains("region")));
    h.click("lineage-1-By region");
    assert!(
        pump(&mut h, |h| text(h, "chart-axis-x").contains("region")
            && text(h, "pane-head-charts").contains(&t("chart.type.bar"))),
        "the saved chart, as it was saved: {:?} / {:?}",
        text(&h, "chart-axis-x"),
        text(&h, "pane-head-charts")
    );
}

/// A package's chart names its table schema and all, `"main"."sales"`, as the
/// demo's does. The lineage read only `"sales"`, so such a chart was neither
/// listed nor shown again (PD-023, step 5.11b).
#[test]
#[serial]
fn a_chart_a_package_stored_is_in_its_tables_lineage_and_comes_back() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, _) = window("stored", &[("sales.csv", sales(40))]);
    h.click("store-chart");
    perform(&mut h, ids::INSPECTOR_TOGGLE);
    assert!(
        pump(&mut h, |h| has(h, "lineage-1-Stored")),
        "listed under its table: {:?}",
        lineage(&h)
    );

    h.click("lineage-1-Stored");
    assert!(
        pump(&mut h, |h| text(h, "chart-axis-x").contains("region")),
        "shown again: {:?} / {:?}",
        text(&h, "pane-head-charts"),
        text(&h, "banner-host")
    );
}
