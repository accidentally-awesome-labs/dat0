//! The chart follows the active tab (step 5.5b).
//!
//! The charts pane was built with nothing feeding it (PD-023): its source was
//! never set, so it showed its empty state whatever was open, its axis pickers
//! had no columns to offer, and a plot query that failed was dropped without a
//! word. These tests mount the real `Shell` over a real session, open CSVs,
//! open the pane with Visualize and pick axes the way a pointer does, reading
//! the chart that comes back.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::charts::data::{PlotColumn, PlotTable};
use dat0_core::charts::spec::{ChartSpec, ChartType};
use dat0_i18n::t;
use dat0_ui::components::charts::host::write;
use dat0_ui::components::charts::{ChartFormat, palette};
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::{Harness, Key, Modifiers};

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

const COMMANDS: &[&str] = &[ids::CHART_VISUALIZE, ids::CHART_EXPORT_PNG, ids::VIEW_UNDO];

const SALES: &str = "region,amt\nEU,3\nUS,5\nEU,4\n";
const STOCK: &str = "name,qty\nb,2\na,3\nc,1\n";

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

fn has(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id).is_some()
}

/// The chart's SVG, or nothing while none is drawn.
fn svg(h: &Harness) -> String {
    h.by_a11y_id("chart-svg")
        .and_then(|k| h.attr(k, "dangerous_inner_html"))
        .unwrap_or_default()
}

fn disabled(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id)
        .and_then(|k| h.attr(k, "disabled"))
        .as_deref()
        == Some("true")
}

/// The chart pane's header: the table charted, and the chart's type.
fn head(h: &Harness) -> String {
    text(h, "pane-head-charts")
}

/// A window over `files`, written under `dir`, with its first grid painted.
fn window(dir: &str, files: &[(&str, &str)]) -> Harness {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let cli_paths = files
        .iter()
        .map(|(name, body)| {
            let path = dir.join(name);
            std::fs::write(&path, body).expect("write csv");
            path
        })
        .collect();
    let mut h = Harness::new(Host, HostProps { cli_paths });
    assert!(
        pump(&mut h, |h| !text(h, "cell-0-0").is_empty()),
        "no grid painted"
    );
    h
}

fn perform(h: &mut Harness, id: &str) {
    h.click(&format!("do-{id}"));
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

/// Type `value` into the cell at `(row, col)`, the way the grid's editor
/// takes it.
fn edit(h: &mut Harness, (row, col): (usize, usize), value: &str) {
    let cell = h
        .by_a11y_id(&format!("cell-{row}-{col}"))
        .unwrap_or_else(|| panic!("no cell {row},{col}"));
    h.dispatch(cell, "mousedown", mouse(Modifiers::empty()));
    let grid = h.by_a11y_id("grid-viewport").expect("the grid");
    h.dispatch(grid, "mouseup", mouse(Modifiers::empty()));
    h.key_at("grid-viewport", Key::Enter, Modifiers::empty());
    let editor = h.by_a11y_id("cell-editor").expect("Enter opens the editor");
    let typed = dioxus::html::SerializedFormData::new(value.to_string(), Vec::new());
    h.dispatch(editor, "input", typed);
    h.key(editor, Key::Enter, Modifiers::empty());
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
fn visualize_binds_the_chart_to_the_tab_and_an_axis_draws_it() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("bound", &[("sales.csv", SALES)]);

    perform(&mut h, ids::CHART_VISUALIZE);
    assert!(
        pump(&mut h, |h| head(h).contains("sales")),
        "the pane names the tab's table: {:?}",
        head(&h)
    );
    assert!(
        head(&h).contains(&t("chart.type.bar")),
        "a text and a number column suggest a bar chart: {:?}",
        head(&h)
    );
    assert_eq!(text(&h, "chart-empty"), t("chart.panel.empty"));
    assert!(disabled(&h, "chart-export-png"), "nothing drawn to export");

    h.click("chart-axis-x");
    assert!(
        pump(&mut h, |h| !svg(h).is_empty()),
        "a bar per region: {:?}",
        text(&h, "chart-body")
    );
    assert!(text(&h, "chart-axis-x").contains("region"));
    assert!(!disabled(&h, "chart-export-png"));
    assert!(!disabled(&h, "chart-save"));
    let counted = svg(&h);

    h.click("chart-axis-y");
    assert!(
        pump(&mut h, |h| !svg(h).is_empty() && svg(h) != counted),
        "the bars sum amt instead of counting rows"
    );
    assert!(text(&h, "chart-axis-y").contains("amt"));
}

#[test]
#[serial]
fn a_chart_that_cannot_be_drawn_says_why() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("undrawable", &[("sales.csv", SALES)]);
    perform(&mut h, ids::CHART_VISUALIZE);
    assert!(pump(&mut h, |h| head(h).contains("sales")));
    h.click("chart-axis-x");
    assert!(pump(&mut h, |h| !svg(h).is_empty()));

    // A line needs a y, and none is picked.
    h.click("chart-type");
    assert!(
        pump(&mut h, |h| text(h, "chart-error").contains("y column")),
        "chart: {:?}",
        text(&h, "chart-body")
    );
    assert!(svg(&h).is_empty(), "no stale chart beside the reason");
    assert!(disabled(&h, "chart-export-png"));
}

#[test]
#[serial]
fn the_chart_follows_the_active_tab_and_keeps_each_tables_chart() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("follows", &[("sales.csv", SALES), ("stock.csv", STOCK)]);
    let (sales, stock) = (tab(&h, "sales"), tab(&h, "stock"));
    h.click(&sales);
    perform(&mut h, ids::CHART_VISUALIZE);
    assert!(
        pump(&mut h, |h| head(h).contains("sales")),
        "{:?}",
        head(&h)
    );
    h.click("chart-axis-x");
    assert!(pump(&mut h, |h| !svg(h).is_empty()));

    h.click(&stock);
    assert!(
        pump(&mut h, |h| head(h).contains("stock")
            && has(h, "chart-empty")),
        "the other tab's table, nothing picked yet: {:?} / {:?}",
        head(&h),
        text(&h, "chart-body")
    );
    assert!(!text(&h, "chart-axis-x").contains("region"));

    h.click(&sales);
    assert!(
        pump(&mut h, |h| head(h).contains("sales") && !svg(h).is_empty()),
        "the first tab's chart, as it was left: {:?}",
        text(&h, "chart-body")
    );
    assert!(text(&h, "chart-axis-x").contains("region"));
}

#[test]
#[serial]
fn an_edited_value_is_drawn_and_undoing_it_draws_the_chart_again() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("edited", &[("sales.csv", SALES)]);
    perform(&mut h, ids::CHART_VISUALIZE);
    assert!(pump(&mut h, |h| head(h).contains("sales")));
    h.click("chart-axis-x");
    assert!(pump(&mut h, |h| !svg(h).is_empty()));
    let counted = svg(&h);
    h.click("chart-axis-y");
    assert!(pump(&mut h, |h| !svg(h).is_empty() && svg(h) != counted));
    let summed = svg(&h);

    // EU's 3 becomes 30: the edit is laid over the table as a view, and the
    // chart is drawn from what the grid reads.
    edit(&mut h, (0, 1), "30");
    assert!(
        pump(&mut h, |h| text(h, "cell-0-1") == "30"),
        "the edit lands: {:?}",
        text(&h, "cell-0-1")
    );
    assert!(
        pump(&mut h, |h| !svg(h).is_empty() && svg(h) != summed),
        "the chart still shows the value before the edit"
    );

    perform(&mut h, ids::VIEW_UNDO);
    assert!(
        pump(&mut h, |h| svg(h) == summed),
        "Undo draws the chart it took the edit from"
    );
}

#[test]
#[serial]
fn with_no_table_open_the_chart_asks_for_one() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: Vec::new(),
        },
    );
    h.settle();
    perform(&mut h, ids::CHART_VISUALIZE);
    assert!(
        pump(&mut h, |h| text(h, "chart-empty")
            == t("chart.panel.no_source")),
        "{:?}",
        text(&h, "chart-body")
    );

    perform(&mut h, ids::CHART_EXPORT_PNG);
    assert!(
        pump(&mut h, |h| text(h, "banner-host")
            .contains(&t("chart.export.nothing"))),
        "{:?}",
        text(&h, "banner-host")
    );
}

#[derive(Clone, PartialEq, Props)]
struct WriterProps {
    to: PathBuf,
    format: ChartFormat,
}

/// Writes a two-bar chart to `to` when clicked, and shows the banners.
#[component]
fn Writer(props: WriterProps) -> Element {
    let ws = Workspace::provide();
    let WriterProps { to, format } = props;
    rsx! {
        button {
            "data-a11y-id": "write",
            onclick: move |_| {
                let to = to.clone();
                let spec = ChartSpec {
                    chart_type: ChartType::Bar,
                    source: "\"sales\"".into(),
                    x: Some("region".into()),
                    y: Some("amt".into()),
                    group: None,
                    color: None,
                    title: String::new(),
                };
                let rows = PlotTable {
                    rows: 2,
                    columns: vec![
                        PlotColumn { name: "k".into(), num: None, text: Some(vec!["EU".into(), "US".into()]) },
                        PlotColumn { name: "v".into(), num: Some(vec![7.0, 5.0]), text: None },
                    ],
                };
                let pal = palette(&dat0_core::theme::builtin_or_default("dark"));
                spawn(write(ws, spec, rows, pal, format, to));
            },
        }
        div { "data-a11y-id": "banners",
            for b in ws.banners.read().iter() {
                span { "{b.title} {b.body}" }
            }
        }
    }
}

#[test]
#[serial]
fn an_export_writes_the_chart_and_says_where() {
    let rt = runtime();
    let _guard = rt.enter();
    let dir = STATE_ROOT.join("export");
    std::fs::create_dir_all(&dir).expect("mkdir");

    for (format, name, magic) in [
        (ChartFormat::Png, "sales.png", &b"\x89PNG"[..]),
        (ChartFormat::Svg, "sales.svg", &b"<svg"[..]),
    ] {
        let to = dir.join(name);
        let mut h = Harness::new(
            Writer,
            WriterProps {
                to: to.clone(),
                format,
            },
        );
        h.settle();
        h.click("write");
        let done = t("chart.export.done");
        assert!(
            pump(&mut h, |h| text(h, "banners").contains(&done)),
            "{:?}",
            text(&h, "banners")
        );
        assert!(text(&h, "banners").contains(name), "says where");
        let bytes = std::fs::read(&to).expect("the file");
        assert!(bytes.starts_with(magic), "{name}");
    }

    // Drawn in the palette it was handed, not in plotters' own colours.
    let svg = std::fs::read_to_string(dir.join("sales.svg"))
        .unwrap()
        .to_ascii_lowercase();
    let bg = palette(&dat0_core::theme::builtin_or_default("dark")).background;
    let hex = format!("#{:02x}{:02x}{:02x}", bg.0, bg.1, bg.2);
    assert!(svg.contains(&hex), "the background is not {hex}");
}
