//! The grid header: four zones per column, and column resize.
//!
//! Zones are `grip | body | sort | funnel`, and which one the pointer is in
//! decides whether a click resizes, selects, sorts or filters. The mapping is
//! `dat0_core::grid::header::zone_from_x`, shared with the tests that pin it.
//!
//! # Resize is ours now
//!
//! `gpui-component`'s table owned column resize; here a `mousedown` on the 6px
//! grip installs `mousemove`/`mouseup` listeners on **`document`**, because a
//! drag that leaves the header — which every real drag does — stops delivering
//! events to the element that started it. The listeners stream `clientX` back
//! over a `document::eval` channel.
//!
//! Widths are clamped to 10..=1200 and sub-pixel deltas are dropped, matching
//! upstream's behaviour so nothing regresses. Unlike upstream, the resulting
//! width is **persisted** in `ColumnView` — column widths used to reset on
//! every reload.

use dioxus::prelude::*;

use dat0_core::grid::header::{HEADER_FUNNEL_PX, HEADER_GRIP_PX, HEADER_SORT_PX};
use dat0_engine::transform::ProjectionColumn;

use super::{COL_W_DEFAULT, ROW_H, offset_of};

/// Narrowest a column may be dragged. Below this the grip and the sort/funnel
/// hit-targets overlap and the header stops being operable.
pub const MIN_COL_W: f64 = 10.0;
/// Widest a column may be dragged.
pub const MAX_COL_W: f64 = 1200.0;

/// Apply a drag delta to a starting width.
///
/// Pure so the clamp is testable without a pointer: the interesting cases are
/// the bounds and the sub-pixel no-op, none of which are convenient to drive
/// through a window.
pub fn resized(start_w: f64, delta_x: f64) -> f64 {
    if delta_x.abs() < 1.0 || !delta_x.is_finite() {
        return start_w.clamp(MIN_COL_W, MAX_COL_W);
    }
    (start_w + delta_x).clamp(MIN_COL_W, MAX_COL_W)
}

#[derive(Clone, Props, PartialEq)]
pub struct HeaderProps {
    pub columns: Vec<ProjectionColumn>,
    pub widths: Vec<f64>,
    /// Mirrors the body's horizontal scroll so the header tracks it.
    pub scroll_left: f64,
    /// The column currently being dragged to a new position, if any.
    pub dragging_col: Option<usize>,
    /// `(column, pointer x)` when a resize begins.
    pub on_resize_start: EventHandler<(usize, f64)>,
    pub on_reorder_start: EventHandler<usize>,
    pub on_reorder_drop: EventHandler<usize>,
}

#[component]
pub fn Header(props: HeaderProps) -> Element {
    let total_w: f64 = props.widths.iter().sum();
    let offset = -props.scroll_left;

    rsx! {
        div { class: "d0-grid-head", "data-a11y-id": "grid-head", role: "row",
            div {
                style: "position: absolute; top: 0; left: {offset}px; width: {total_w}px; height: {ROW_H}px;",
                for (i, col) in props.columns.iter().enumerate() {
                    {column_header(i, col, &props.widths, &props)}
                }
            }
        }
    }
}

fn column_header(
    ix: usize,
    col: &ProjectionColumn,
    widths: &[f64],
    props: &HeaderProps,
) -> Element {
    let left = offset_of(widths, ix);
    let width = widths.get(ix).copied().unwrap_or(COL_W_DEFAULT);
    let name = col.display.clone();
    let is_dragging = props.dragging_col == Some(ix);

    let on_resize_start = props.on_resize_start;
    let on_reorder_start = props.on_reorder_start;
    let on_reorder_drop = props.on_reorder_drop;

    rsx! {
        div {
            key: "{ix}",
            class: if is_dragging { "d0-colheader is-dragging" } else { "d0-colheader" },
            "data-a11y-id": "col-{ix}",
            role: "columnheader",
            "aria-label": "{name}",
            "aria-colindex": "{ix + 1}",
            tabindex: "-1",
            style: "left: {left}px; width: {width}px;",

            // Reorder is HTML5 drag on the grip, unchanged from the GPUI app —
            // `zone_from_x` has always mapped the left 6px to Grip = reorder,
            // and moving it would retrain every existing user.
            //
            // There is exactly ONE reorder implementation now. The GPUI build
            // had two racing: dat0's grip-driven `Transformation::Reorder`
            // through `ViewModel`, and gpui-component's own `DragColumn`, which
            // mutated `col_groups` and called a `move_column` dat0 never
            // implemented — so the widget could desync `ColumnView`.
            div {
                class: "d0-grip",
                "data-a11y-id": "col-grip-{ix}",
                style: "width: {HEADER_GRIP_PX}px;",
                draggable: true,
                ondragstart: move |_| on_reorder_start.call(ix),
                ondragover: move |e| e.prevent_default(),
                ondrop: move |e| {
                    e.prevent_default();
                    on_reorder_drop.call(ix);
                },
            }
            span { style: "flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis;", "{name}" }
            span {
                "data-a11y-id": "col-sort-{ix}",
                style: "width: {HEADER_SORT_PX}px; text-align: center;",
            }
            span {
                "data-a11y-id": "col-funnel-{ix}",
                style: "width: {HEADER_FUNNEL_PX}px; text-align: center;",
            }
            // Resize sits astride the column's trailing edge, the universal
            // convention, and deliberately NOT on the grip: the grip is
            // reorder, and one 6px target cannot mean two gestures. Being a
            // separate element also keeps `zone_from_x`'s four-zone contract
            // untouched.
            div {
                class: "d0-col-resize",
                "data-a11y-id": "col-resize-{ix}",
                onmousedown: move |e| {
                    e.stop_propagation();
                    on_resize_start.call((ix, e.data().client_coordinates().x));
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_core::grid::header::{ColumnHeaderZone, zone_from_x};

    #[test]
    fn a_drag_widens_and_narrows_within_the_clamp() {
        assert_eq!(resized(100.0, 50.0), 150.0);
        assert_eq!(resized(100.0, -50.0), 50.0);
    }

    #[test]
    fn the_clamp_holds_at_both_ends() {
        // Below MIN the grip overlaps the sort/funnel targets and the header
        // stops being operable; above MAX one column can hide every other.
        assert_eq!(resized(20.0, -1000.0), MIN_COL_W);
        assert_eq!(resized(1000.0, 5000.0), MAX_COL_W);
    }

    #[test]
    fn a_subpixel_delta_is_a_no_op() {
        // Matching upstream: without this every stray pointer jitter writes a
        // new width and the persist debounce never settles.
        assert_eq!(resized(137.0, 0.4), 137.0);
        assert_eq!(resized(137.0, -0.9), 137.0);
    }

    #[test]
    fn a_non_finite_delta_cannot_corrupt_a_width() {
        // A NaN width would propagate into the canvas style and collapse the
        // whole grid, and it round-trips into the session file.
        assert_eq!(resized(120.0, f64::NAN), 120.0);
        assert_eq!(resized(120.0, f64::INFINITY), 120.0);
    }

    #[test]
    fn an_already_out_of_range_width_is_brought_back_in() {
        assert_eq!(resized(5.0, 0.0), MIN_COL_W);
        assert_eq!(resized(9000.0, 0.0), MAX_COL_W);
    }

    #[test]
    fn the_header_zones_line_up_with_the_rendered_widths() {
        // The rendered grip/sort/funnel spans and `zone_from_x` must agree, or
        // a click lands in a zone the user cannot see.
        let w = 200.0_f32;
        assert_eq!(zone_from_x(1.0, w), ColumnHeaderZone::Grip);
        assert_eq!(zone_from_x(HEADER_GRIP_PX + 1.0, w), ColumnHeaderZone::Body);
        assert_eq!(
            zone_from_x(w - HEADER_FUNNEL_PX - 1.0, w),
            ColumnHeaderZone::Sort
        );
        assert_eq!(zone_from_x(w - 1.0, w), ColumnHeaderZone::Funnel);
    }
}
