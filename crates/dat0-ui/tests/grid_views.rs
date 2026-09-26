//! The grid's view: sort, filter, the pipeline bar, Undo/Redo, reordering
//! (step 5.3).
//!
//! The header's sort and funnel zones had no handler, the filter popover was
//! never mounted, the pipeline bar was handed an empty stack, and dragging a
//! header moved only its width (PD-023). These tests mount the real `Shell`
//! over a real session, open a CSV, and drive the header the way a pointer
//! does, reading the cells that come back.

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

const COMMANDS: &[&str] = &[ids::VIEW_UNDO, ids::VIEW_REDO];

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

/// The first column, top to bottom, as far as the rows go.
fn names(h: &Harness) -> Vec<String> {
    (0..10)
        .map(|r| text(h, &format!("cell-{r}-0")))
        .take_while(|v| !v.is_empty())
        .collect()
}

/// A window over a three-row CSV, with its rows painted.
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
        pump(&mut h, |h| names(h) == ["b", "a", "c"]),
        "the CSV never painted: {:?}",
        names(&h)
    );
    h
}

fn perform(h: &mut Harness, id: &str) {
    h.click(&format!("do-{id}"));
}

fn typed(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

fn drag_data() -> dioxus::html::SerializedDragData {
    use dioxus::html::SerializedDataTransfer;
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    use dioxus::html::point_interaction::SerializedPointInteraction;

    let c = Coordinates::new(
        ScreenPoint::new(0.0, 0.0),
        ClientPoint::new(0.0, 0.0),
        ElementPoint::new(0.0, 0.0),
        PagePoint::new(0.0, 0.0),
    );
    dioxus::html::SerializedDragData {
        mouse: SerializedPointInteraction::new(
            Some(MouseButton::Primary),
            MouseButton::Primary.into(),
            c,
            dioxus::prelude::Modifiers::empty(),
        ),
        data_transfer: SerializedDataTransfer {
            items: Vec::new(),
            files: Vec::new(),
            effect_allowed: "move".into(),
            drop_effect: "move".into(),
        },
    }
}

/// Filter `name` to rows equal to `value`, through the funnel's popover.
fn filter_name(h: &mut Harness, value: &str) {
    h.click("col-funnel-0");
    assert!(has(h, "filter-popover"), "the funnel opens its popover");
    let field = h.by_a11y_id("filter-value").expect("a value field");
    h.dispatch(field, "input", typed(value));
    h.click("filter-apply");
}

#[test]
#[serial]
fn a_sort_click_cycles_ascending_descending_and_off() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("sort");

    h.click("col-sort-1");
    assert!(
        pump(&mut h, |h| names(h) == ["c", "b", "a"]),
        "ascending by qty: {:?}",
        names(&h)
    );
    assert_eq!(text(&h, "col-sort-1"), "▲");
    assert!(
        has(&h, "pipeline-chip-0"),
        "the sort is on the pipeline bar"
    );

    h.click("col-sort-1");
    assert!(
        pump(&mut h, |h| names(h) == ["a", "b", "c"]),
        "descending by qty: {:?}",
        names(&h)
    );
    assert_eq!(text(&h, "col-sort-1"), "▼");

    h.click("col-sort-1");
    assert!(
        pump(&mut h, |h| names(h) == ["b", "a", "c"]),
        "and off again, in file order: {:?}",
        names(&h)
    );
    assert_eq!(text(&h, "col-sort-1"), "");
    assert!(!has(&h, "pipeline-chip-0"), "nothing left on the bar");
}

#[test]
#[serial]
fn a_funnel_filters_its_column_and_clear_takes_off_only_that_filter() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("filter");

    h.click("col-sort-1");
    assert!(pump(&mut h, |h| names(h) == ["c", "b", "a"]));

    filter_name(&mut h, "a");
    assert!(
        pump(&mut h, |h| names(h) == ["a"]),
        "only the matching row: {:?}",
        names(&h)
    );
    assert!(!has(&h, "filter-popover"), "Apply closes the popover");
    let funnel = h.by_a11y_id("col-funnel-0").unwrap();
    assert_eq!(
        h.attr(funnel, "class").as_deref(),
        Some("d0-funnel is-on"),
        "a filtered column says so"
    );

    // Clearing the name filter must leave the sort alone.
    h.click("col-funnel-0");
    h.click("filter-clear");
    assert!(
        pump(&mut h, |h| names(h) == ["c", "b", "a"]),
        "all rows back, still sorted: {:?}",
        names(&h)
    );
    assert_eq!(text(&h, "col-sort-1"), "▲");
}

#[test]
#[serial]
fn the_pipeline_bar_and_undo_redo_move_through_the_stack() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("pipeline");

    h.click("col-sort-1");
    assert!(pump(&mut h, |h| names(h) == ["c", "b", "a"]));
    filter_name(&mut h, "b");
    assert!(pump(&mut h, |h| names(h) == ["b"]));
    assert!(has(&h, "pipeline-chip-1"), "two ops on the bar");

    // Drop the filter from the bar, which lists its steps when expanded: all
    // rows, sorted.
    h.click("pipeline-toggle");
    h.click("pipeline-remove-1");
    assert!(
        pump(&mut h, |h| names(h) == ["c", "b", "a"]),
        "{:?}",
        names(&h)
    );

    perform(&mut h, ids::VIEW_UNDO);
    assert!(
        pump(&mut h, |h| names(h) == ["b"]),
        "undo puts the filter back: {:?}",
        names(&h)
    );
    perform(&mut h, ids::VIEW_REDO);
    assert!(
        pump(&mut h, |h| names(h) == ["c", "b", "a"]),
        "redo takes it off again: {:?}",
        names(&h)
    );
}

#[test]
#[serial]
fn dragging_a_header_moves_the_column_not_just_its_width() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("reorder");

    // Widen name first, so its width can be told apart from qty's.
    h.dispatch(h.by_a11y_id("col-resize-0").unwrap(), "mousedown", at(0.0));
    h.dispatch(h.by_a11y_id("drag-shield").unwrap(), "mousemove", at(80.0));
    h.dispatch(h.by_a11y_id("drag-shield").unwrap(), "mouseup", at(80.0));
    let wide = width_of(&h, "col-0");

    h.dispatch(
        h.by_a11y_id("col-grip-0").unwrap(),
        "dragstart",
        drag_data(),
    );
    h.dispatch(h.by_a11y_id("col-grip-1").unwrap(), "drop", drag_data());

    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "2"),
        "qty is first now, so row 0 starts with b's quantity: {:?}",
        text(&h, "cell-0-0")
    );
    let first = h.by_a11y_id("col-0").unwrap();
    assert_eq!(h.attr(first, "aria-label").as_deref(), Some("qty"));
    assert_eq!(width_of(&h, "col-1"), wide, "name kept its width");
    assert_ne!(width_of(&h, "col-0"), wide, "and qty kept its own");

    // Undo puts the column back, and the width goes with it again.
    perform(&mut h, ids::VIEW_UNDO);
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "b"),
        "{:?}",
        text(&h, "cell-0-0")
    );
    assert_eq!(width_of(&h, "col-0"), wide);
}

/// A mouse event at `x`, which is all a resize gesture reads.
fn at(x: f64) -> dioxus::html::SerializedMouseData {
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    let c = Coordinates::new(
        ScreenPoint::new(x, 0.0),
        ClientPoint::new(x, 0.0),
        ElementPoint::new(x, 0.0),
        PagePoint::new(x, 0.0),
    );
    dioxus::html::SerializedMouseData::new(
        Some(MouseButton::Primary),
        MouseButton::Primary.into(),
        c,
        dioxus::prelude::Modifiers::empty(),
    )
}

/// The `width: …px` a header cell is laid out at.
fn width_of(h: &Harness, id: &str) -> String {
    let style = h
        .by_a11y_id(id)
        .and_then(|k| h.attr(k, "style"))
        .unwrap_or_default();
    style
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("width:"))
        .unwrap_or_default()
        .to_string()
}
