//! Phase 3 acceptance, part B — the grid is virtualized.
//!
//! The claim dat0 makes is "a billion rows on one laptop". What makes that
//! possible is that the DOM holds tens of rows regardless of how many the table
//! has, and that scrolling *moves* the window rather than growing it.
//!
//! This asserts both against a **one-million-row** source. It is deliberately
//! not a performance test — Phase 0.1's spike measures frame timing in a real
//! compositing window, which a headless harness cannot do. What is checked here
//! is the property that makes the performance possible, and which a refactor
//! could silently lose while every other test still passed.

mod support;

use std::sync::Arc;

use dioxus::prelude::*;
use tempfile::TempDir;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::SelectionModel;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid, ROW_H};
use support::Harness;

const ROWS: u64 = 1_000_000;

/// A million rows, generated inside DuckDB rather than imported: `range()` is
/// instant and the point here is the row *count*, not the ingest path.
async fn million_rows() -> (Arc<GridDataSource>, Vec<ProjectionColumn>, TempDir) {
    let tmp = TempDir::new().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    let sql =
        format!("SELECT i AS id, 'row ' || i AS label, i * 1.5 AS score FROM range({ROWS}) t(i)");
    engine
        .create_table("big", &sql, DerivedOrigin::Sql(sql.clone()))
        .await
        .unwrap();

    let engine = Arc::new(engine);
    let ds = GridDataSource::new(Arc::clone(&engine), "big".to_string())
        .await
        .unwrap();
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
    let selection = use_signal(|| SelectionModel::new(ROWS as usize, cols));
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);
    rsx! {
        Grid {
            source: props.source.clone(),
            selection,
            columns: props.columns.clone(),
            widths,
        }
    }
}

/// An 800x600 viewport scrolled to `scroll_top`.
fn scroll(scroll_top: f64) -> dioxus::html::SerializedScrollData {
    dioxus::html::SerializedScrollData {
        scroll_top,
        scroll_left: 0.0,
        scroll_width: 300,
        scroll_height: (ROWS as f64 * ROW_H) as i32,
        client_width: 800,
        client_height: 600,
    }
}

#[tokio::test]
async fn a_million_rows_render_as_tens_of_nodes() {
    let (source, columns, _tmp) = million_rows().await;
    assert_eq!(source.row_count, ROWS);

    let mut h = Harness::new(Host, HostProps { source, columns });
    h.dispatch(
        h.by_a11y_id("grid-viewport").unwrap(),
        "scroll",
        scroll(0.0),
    );

    let rows = h.by_role("row").len();
    // 600 / 26 = 23 visible, + 4 overscan, + 1 header row.
    assert!(
        rows <= 40,
        "the DOM holds {rows} rows for a 1,000,000-row table — virtualization is gone"
    );
    assert!(
        rows >= 20,
        "only {rows} rows rendered; the window is too small"
    );
}

#[tokio::test]
async fn scrolling_moves_the_window_rather_than_growing_it() {
    let (source, columns, _tmp) = million_rows().await;
    let mut h = Harness::new(Host, HostProps { source, columns });

    h.dispatch(
        h.by_a11y_id("grid-viewport").unwrap(),
        "scroll",
        scroll(0.0),
    );
    assert!(h.by_a11y_id("row-0").is_some());

    // Jump most of the way down.
    h.dispatch(
        h.by_a11y_id("grid-viewport").unwrap(),
        "scroll",
        scroll(ROW_H * 900_000.0),
    );

    assert!(
        h.by_a11y_id("row-900000").is_some(),
        "the row at the new scroll position is not mounted"
    );
    assert!(
        h.by_a11y_id("row-0").is_none(),
        "row 0 is still mounted after scrolling 900,000 rows — the window grew instead of moving"
    );
    // An absolute bound, not a delta against the count at the top. At
    // `scroll_top = 0` there is nothing above row 0, so the window is short by
    // one overscan block; away from an edge it gets overscan on both sides.
    // That difference is the design working, not the window growing.
    let after = h.by_role("row").len();
    assert!(
        after <= 40,
        "the window holds {after} rows mid-table — virtualization is gone"
    );
}

#[tokio::test]
async fn the_canvas_advertises_the_full_extent() {
    // The scrollbar must be honest about how much data there is: a canvas sized
    // to the window is the "infinite scroll" lie, and it breaks drag-to-position.
    let (source, columns, _tmp) = million_rows().await;
    let h = Harness::new(Host, HostProps { source, columns });

    let canvas = h
        .dom()
        .walk()
        .into_iter()
        .find(|k| {
            h.dom()
                .get(*k)
                .attr("class")
                .is_some_and(|c| c.contains("d0-grid-canvas"))
        })
        .expect("the canvas exists");

    let style = h.attr(canvas, "style").unwrap_or_default();
    let want_h = format!("height: {}px", ROWS as f64 * ROW_H);
    assert!(style.contains(&want_h), "{style}");
}

#[tokio::test]
async fn a_row_beyond_the_window_is_not_in_the_dom() {
    let (source, columns, _tmp) = million_rows().await;
    let mut h = Harness::new(Host, HostProps { source, columns });
    h.dispatch(
        h.by_a11y_id("grid-viewport").unwrap(),
        "scroll",
        scroll(0.0),
    );

    assert!(h.by_a11y_id("row-500000").is_none());
    assert!(h.by_a11y_id("row-999999").is_none());
}
