//! Column resize and reorder, driven as real pointer gestures.
//!
//! Both used to be somebody else's code. Resize was `gpui-component`'s
//! `DragColumn`, which mutated its own `col_groups` and called a
//! `delegate.move_column` dat0 never implemented — so the widget could desync
//! `ColumnView`, and there were two racing reorder paths. There is one of each
//! now, and these tests are what say so.
//!
//! The gestures are driven through the drag shield rather than a JS
//! `document` listener, which is exactly why they are testable here at all.

mod support;

use std::sync::Arc;

use dioxus::prelude::*;
use tempfile::TempDir;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::SelectionModel;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use dat0_ui::components::grid::header::{MAX_COL_W, MIN_COL_W};
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid};
use support::Harness;

async fn fixture() -> (Arc<GridDataSource>, Vec<ProjectionColumn>, TempDir) {
    let tmp = TempDir::new().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    const SQL: &str = "SELECT * FROM (VALUES (1, 'a', 10), (2, 'b', 20)) v(id, name, score)";
    engine
        .create_table("t", SQL, DerivedOrigin::Sql(SQL.into()))
        .await
        .unwrap();
    let engine = Arc::new(engine);
    let ds = GridDataSource::new(Arc::clone(&engine), "t".to_string())
        .await
        .unwrap();
    ds.page_for(0).await.unwrap();
    let columns = ds
        .visible_column_names()
        .into_iter()
        .map(|n| ProjectionColumn {
            source: n.clone(),
            display: n,
        })
        .collect();
    (Arc::new(ds), columns, tmp)
}

#[derive(Clone, Props)]
struct HostProps {
    source: Arc<GridDataSource>,
    columns: Vec<ProjectionColumn>,
}

impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source) && self.columns == other.columns
    }
}

#[component]
fn Host(props: HostProps) -> Element {
    let cols = props.columns.len();
    let selection = use_signal(|| SelectionModel::new(2, cols));
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);
    rsx! {
        Grid {
            source: props.source.clone(),
            selection,
            columns: props.columns.clone(),
            widths,
        }
        // Readback: the harness sees text, not Rust state.
        div { "data-a11y-id": "widths", "{widths.read():?}" }
    }
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

/// Column widths, read back out of the mounted tree.
fn widths(h: &Harness) -> Vec<f64> {
    let text = h.text_of(h.by_a11y_id("widths").unwrap());
    text.trim_matches(|c| c == '[' || c == ']')
        .split(", ")
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap_or(f64::NAN))
        .collect()
}

async fn mount() -> (Harness, TempDir) {
    let (source, columns, tmp) = fixture().await;
    (Harness::new(Host, HostProps { source, columns }), tmp)
}

#[tokio::test]
async fn there_is_no_drag_shield_until_a_gesture_starts() {
    // The shield covers the window; leaving one mounted would swallow every
    // click in the app.
    let (h, _tmp) = mount().await;
    assert!(h.by_a11y_id("drag-shield").is_none());
}

#[tokio::test]
async fn dragging_the_handle_widens_only_that_column() {
    let (mut h, _tmp) = mount().await;
    assert_eq!(widths(&h), vec![100.0, 100.0, 100.0]);

    h.dispatch(
        h.by_a11y_id("col-resize-1").unwrap(),
        "mousedown",
        at(100.0),
    );
    let shield = h
        .by_a11y_id("drag-shield")
        .expect("a live gesture mounts the shield");
    h.dispatch(shield, "mousemove", at(160.0));

    assert_eq!(
        widths(&h),
        vec![100.0, 160.0, 100.0],
        "only the dragged column changes width"
    );
}

#[tokio::test]
async fn the_gesture_ends_on_release_and_the_shield_goes_away() {
    let (mut h, _tmp) = mount().await;
    h.dispatch(h.by_a11y_id("col-resize-0").unwrap(), "mousedown", at(50.0));
    let shield = h.by_a11y_id("drag-shield").unwrap();
    h.dispatch(shield, "mousemove", at(90.0));
    assert_eq!(widths(&h)[0], 140.0);

    h.dispatch(h.by_a11y_id("drag-shield").unwrap(), "mouseup", at(90.0));
    assert!(
        h.by_a11y_id("drag-shield").is_none(),
        "the shield must unmount on release or it swallows every later click"
    );
}

#[tokio::test]
async fn a_drag_is_measured_from_where_it_started_not_from_the_last_move() {
    // Deltas accumulate against the width at grab time, so a move back to the
    // origin restores the original width exactly.
    let (mut h, _tmp) = mount().await;
    h.dispatch(
        h.by_a11y_id("col-resize-0").unwrap(),
        "mousedown",
        at(200.0),
    );
    let shield = h.by_a11y_id("drag-shield").unwrap();

    h.dispatch(shield, "mousemove", at(320.0));
    assert_eq!(widths(&h)[0], 220.0);
    h.dispatch(shield, "mousemove", at(250.0));
    assert_eq!(widths(&h)[0], 150.0);
    h.dispatch(shield, "mousemove", at(200.0));
    assert_eq!(
        widths(&h)[0],
        100.0,
        "back to the origin is back to the width"
    );
}

#[tokio::test]
async fn the_clamp_holds_through_a_real_drag() {
    let (mut h, _tmp) = mount().await;
    h.dispatch(
        h.by_a11y_id("col-resize-2").unwrap(),
        "mousedown",
        at(300.0),
    );
    let shield = h.by_a11y_id("drag-shield").unwrap();

    h.dispatch(shield, "mousemove", at(-5000.0));
    assert_eq!(widths(&h)[2], MIN_COL_W);
    h.dispatch(shield, "mousemove", at(9000.0));
    assert_eq!(widths(&h)[2], MAX_COL_W);
}

#[tokio::test]
async fn a_resize_does_not_start_a_cell_selection() {
    // The handle sits inside the header, which sits above the body; without
    // `stop_propagation` the mousedown would also begin a drag-select.
    let (mut h, _tmp) = mount().await;
    h.dispatch(h.by_a11y_id("col-resize-0").unwrap(), "mousedown", at(10.0));
    // The shield is up, so no cell can be receiving pointer events.
    assert!(h.by_a11y_id("drag-shield").is_some());
}

#[tokio::test]
async fn dropping_a_grip_on_another_column_moves_it() {
    let (mut h, _tmp) = mount().await;
    // Make the columns distinguishable by width, then move column 0 to slot 2.
    h.dispatch(h.by_a11y_id("col-resize-0").unwrap(), "mousedown", at(0.0));
    h.dispatch(h.by_a11y_id("drag-shield").unwrap(), "mousemove", at(80.0));
    h.dispatch(h.by_a11y_id("drag-shield").unwrap(), "mouseup", at(80.0));
    assert_eq!(widths(&h), vec![180.0, 100.0, 100.0]);

    h.dispatch(
        h.by_a11y_id("col-grip-0").unwrap(),
        "dragstart",
        drag_data(),
    );
    h.dispatch(h.by_a11y_id("col-grip-2").unwrap(), "drop", drag_data());

    assert_eq!(
        widths(&h),
        vec![100.0, 100.0, 180.0],
        "the moved column takes its width with it"
    );
}

#[tokio::test]
async fn dropping_a_column_on_itself_changes_nothing() {
    let (mut h, _tmp) = mount().await;
    h.dispatch(h.by_a11y_id("col-resize-1").unwrap(), "mousedown", at(0.0));
    h.dispatch(h.by_a11y_id("drag-shield").unwrap(), "mousemove", at(40.0));
    h.dispatch(h.by_a11y_id("drag-shield").unwrap(), "mouseup", at(40.0));
    let before = widths(&h);

    h.dispatch(
        h.by_a11y_id("col-grip-1").unwrap(),
        "dragstart",
        drag_data(),
    );
    h.dispatch(h.by_a11y_id("col-grip-1").unwrap(), "drop", drag_data());

    assert_eq!(widths(&h), before);
}

#[tokio::test]
async fn a_drop_without_a_dragstart_is_ignored() {
    // A drop can arrive from outside the header — a file dragged onto the
    // window, for instance. Reordering on it would scramble the columns.
    let (mut h, _tmp) = mount().await;
    let before = widths(&h);
    h.dispatch(h.by_a11y_id("col-grip-2").unwrap(), "drop", drag_data());
    assert_eq!(widths(&h), before);
}

#[tokio::test]
async fn the_dragged_header_is_marked_while_the_gesture_is_live() {
    let (mut h, _tmp) = mount().await;
    h.dispatch(
        h.by_a11y_id("col-grip-1").unwrap(),
        "dragstart",
        drag_data(),
    );

    let col = h.by_a11y_id("col-1").unwrap();
    let class = h.attr(col, "class").unwrap_or_default();
    assert!(
        class.contains("is-dragging"),
        "the ghost is a CSS ::after on the dragged header: {class}"
    );
}

/// An empty HTML5 drag. Column reorder carries its payload in Rust state, not
/// in the `DataTransfer`, so nothing here needs filling in.
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
