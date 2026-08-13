//! Phase 3 acceptance, part A — the grid shows real data and selects it.
//!
//! Runs the real `GridDataSource` over a real DuckDB table (no fake source, no
//! stubbed cells) and drives the mounted component through the headless
//! harness. What it pins:
//!
//! * cells render the values the engine actually returned, in the right places;
//! * the identity attributes are **absolute** row/column indices — the contract
//!   every later suite queries by;
//! * mouse selection works, which the GPUI grid never had: `render_td` attached
//!   no click handler and nothing subscribed to `TableEvent`.

mod support;

use std::sync::Arc;

use dioxus::prelude::*;
use tempfile::TempDir;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::{CellCoord, SelectionModel};
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid};
use support::{Harness, Modifiers};

/// Four columns, three rows — the same shape as the repo's `small/basic.csv`
/// fixture, built through the real CTAS path so the surrogate key is present
/// exactly as it is in the app.
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

    const SQL: &str = "SELECT * FROM (VALUES \
         (1, 'alpha', 1.5, true), \
         (2, 'bravo', 2.5, false), \
         (3, 'charlie', 3.5, true)) v(id, name, score, active)";
    engine
        .create_table("basic", SQL, DerivedOrigin::Sql(SQL.into()))
        .await
        .unwrap();

    let engine = Arc::new(engine);
    let ds = GridDataSource::new(Arc::clone(&engine), "basic".to_string())
        .await
        .unwrap();
    // Page 0 must be resident before the first paint: `cell_render_for_source`
    // is synchronous and returns the placeholder for a missing page.
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

// Same reason as `GridProps`: a data source owns an Arrow LRU and a DuckDB
// handle, so identity is the only equality that means anything.
impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source) && self.columns == other.columns
    }
}

/// A shell around the grid that owns the selection signal, so a test can read
/// the model back after driving the UI.
#[component]
fn Host(props: HostProps) -> Element {
    let rows = props.source.row_count as usize;
    let cols = props.columns.len();
    let selection = use_signal(|| SelectionModel::new(rows, cols));
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);
    use_context_provider(|| selection);

    rsx! {
        Grid {
            source: props.source.clone(),
            selection,
            columns: props.columns.clone(),
            widths,
        }
        // A readback surface: the harness sees text, not Rust state.
        div {
            "data-a11y-id": "sel-count",
            "{selection.read().selected_cell_count()}"
        }
        div {
            "data-a11y-id": "sel-active",
            "{selection.read().active().row},{selection.read().active().col}"
        }
    }
}

fn mount(source: Arc<GridDataSource>, columns: Vec<ProjectionColumn>) -> Harness {
    Harness::new(Host, HostProps { source, columns })
}

#[tokio::test]
async fn cells_render_the_engine_s_values_at_absolute_coordinates() {
    let (source, columns, _tmp) = fixture().await;
    let h = mount(source, columns);

    assert_eq!(h.text_of(h.by_a11y_id("cell-0-0").unwrap()), "1");
    assert_eq!(h.text_of(h.by_a11y_id("cell-0-1").unwrap()), "alpha");
    assert_eq!(h.text_of(h.by_a11y_id("cell-2-1").unwrap()), "charlie");

    // Absolute, not window-relative: a window-relative id would address a
    // different row after any scroll, and every later suite queries by these.
    assert!(h.by_a11y_id("row-0").is_some());
    assert!(h.by_a11y_id("row-2").is_some());
    assert!(h.by_a11y_id("row-3").is_none(), "only three rows exist");
}

#[tokio::test]
async fn a_cell_announces_its_column_and_value() {
    let (source, columns, _tmp) = fixture().await;
    let h = mount(source, columns);

    let cell = h.by_a11y_id("cell-0-1").unwrap();
    assert_eq!(h.attr(cell, "aria-label").as_deref(), Some("name: alpha"));
    assert_eq!(h.attr(cell, "role").as_deref(), Some("gridcell"));
    // A grid cell is reached by arrow keys from the grid, never by Tab: three
    // rows of tab stops is tolerable, a million is not.
    assert_eq!(h.attr(cell, "tabindex").as_deref(), Some("-1"));
}

#[tokio::test]
async fn the_header_names_every_column_in_display_order() {
    let (source, columns, _tmp) = fixture().await;
    let h = mount(source, columns);

    for (i, name) in ["id", "name", "score", "active"].iter().enumerate() {
        let col = h
            .by_a11y_id(&format!("col-{i}"))
            .unwrap_or_else(|| panic!("column {i} is missing"));
        assert_eq!(h.attr(col, "aria-label").as_deref(), Some(*name));
        assert_eq!(h.attr(col, "role").as_deref(), Some("columnheader"));
    }
}

#[tokio::test]
async fn a_click_selects_one_cell() {
    // New behaviour: the GPUI grid had no mouse selection at all.
    let (source, columns, _tmp) = fixture().await;
    let mut h = mount(source, columns);
    // A fresh grid has an *active* cell but nothing selected — the two are
    // different, and conflating them is how a bare Delete wipes row 0.
    assert_eq!(h.text_of(h.by_a11y_id("sel-count").unwrap()), "0");
    assert_eq!(h.text_of(h.by_a11y_id("sel-active").unwrap()), "0,0");

    let cell = h.by_a11y_id("cell-1-2").unwrap();
    h.dispatch(cell, "mousedown", mouse(Modifiers::empty()));

    assert_eq!(h.text_of(h.by_a11y_id("sel-active").unwrap()), "1,2");
    assert_eq!(h.text_of(h.by_a11y_id("sel-count").unwrap()), "1");
}

#[tokio::test]
async fn shift_click_extends_to_a_rectangle() {
    let (source, columns, _tmp) = fixture().await;
    let mut h = mount(source, columns);

    h.dispatch(
        h.by_a11y_id("cell-0-0").unwrap(),
        "mousedown",
        mouse(Modifiers::empty()),
    );
    h.dispatch(
        h.by_a11y_id("cell-1-1").unwrap(),
        "mousedown",
        mouse(Modifiers::SHIFT),
    );

    // (0,0)..=(1,1) is four cells.
    assert_eq!(h.text_of(h.by_a11y_id("sel-count").unwrap()), "4");
}

#[tokio::test]
async fn meta_click_adds_a_disjoint_cell_rather_than_replacing() {
    let (source, columns, _tmp) = fixture().await;
    let mut h = mount(source, columns);

    h.dispatch(
        h.by_a11y_id("cell-0-0").unwrap(),
        "mousedown",
        mouse(Modifiers::empty()),
    );
    h.dispatch(
        h.by_a11y_id("cell-2-3").unwrap(),
        "mousedown",
        mouse(Modifiers::META),
    );

    assert_eq!(
        h.text_of(h.by_a11y_id("sel-count").unwrap()),
        "2",
        "a meta-click must add to the selection, not replace it"
    );
}

#[tokio::test]
async fn a_drag_extends_the_selection_and_stops_at_mouseup() {
    let (source, columns, _tmp) = fixture().await;
    let mut h = mount(source, columns);

    // Press on (0,0), move across (0,1) and (0,2): three cells.
    h.dispatch(
        h.by_a11y_id("cell-0-0").unwrap(),
        "mousedown",
        mouse(Modifiers::empty()),
    );
    h.dispatch(
        h.by_a11y_id("cell-0-2").unwrap(),
        "mouseenter",
        mouse(Modifiers::empty()),
    );
    assert_eq!(h.text_of(h.by_a11y_id("sel-count").unwrap()), "3");

    // Release, then hover elsewhere: hovering must not keep extending.
    h.dispatch(
        h.by_a11y_id("grid-viewport").unwrap(),
        "mouseup",
        mouse(Modifiers::empty()),
    );
    h.dispatch(
        h.by_a11y_id("cell-2-3").unwrap(),
        "mouseenter",
        mouse(Modifiers::empty()),
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("sel-count").unwrap()),
        "3",
        "a hover after mouseup must not extend the selection"
    );
}

#[tokio::test]
async fn the_selected_cell_is_marked_for_the_stylesheet() {
    let (source, columns, _tmp) = fixture().await;
    let mut h = mount(source, columns);

    h.dispatch(
        h.by_a11y_id("cell-1-1").unwrap(),
        "mousedown",
        mouse(Modifiers::empty()),
    );

    let cell = h.by_a11y_id("cell-1-1").unwrap();
    let class = h.attr(cell, "class").unwrap_or_default();
    assert!(class.contains("is-selected"), "{class}");
    assert!(class.contains("is-active"), "{class}");

    let other = h.by_a11y_id("cell-0-0").unwrap();
    let class = h.attr(other, "class").unwrap_or_default();
    assert!(!class.contains("is-selected"), "{class}");
}

#[tokio::test]
async fn a_numeric_column_is_right_aligned_and_text_is_not() {
    let (source, columns, _tmp) = fixture().await;
    let h = mount(source, columns);

    let id = h.by_a11y_id("cell-0-0").unwrap();
    assert!(
        h.attr(id, "class").unwrap_or_default().contains("is-right"),
        "an integer column reads as a number and must right-align"
    );
    let name = h.by_a11y_id("cell-0-1").unwrap();
    assert!(
        !h.attr(name, "class")
            .unwrap_or_default()
            .contains("is-right")
    );
}

#[tokio::test]
async fn select_all_covers_every_cell() {
    let (source, columns, _tmp) = fixture().await;
    let cols = columns.len();
    let mut model = SelectionModel::new(source.row_count as usize, cols);
    model.select_all();
    // 3 rows x 4 columns; asserted against the model the grid renders from, so
    // the count and the geometry cannot disagree.
    assert_eq!(model.selected_cell_count(), 12);
    assert_eq!(model.active(), CellCoord { row: 0, col: 0 });
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
