//! The grid's viewport: which rows and columns a render lays out, scaled
//! scrolling, and keeping the cursor in view (PD-026).
//!
//! The grid's canvas used to be `rows × ROW_H` tall, uncapped. WebKit clamps a
//! layout length near 33.5M px, so every row past ~1,290,555 was unreachable:
//! End stopped short and scrolling found nothing below. Past [`MAX_CANVAS_H`]
//! the canvas stops growing, and the scroll position maps to rows in
//! proportion; below it nothing changes, pixel for pixel.
//!
//! The rows are laid out where they would sit on the full-height canvas,
//! shifted up by [`Scale::shift`], so the ones in view are always drawn where
//! the viewport looks. Everything that places a row — the rows, the cell
//! editor — goes through [`Scale::top`].

use super::{COL_W_DEFAULT, ROW_H};

/// Rows rendered above and below the viewport.
const OVERSCAN_ROWS: usize = 4;
/// Columns rendered left and right of the viewport.
const OVERSCAN_COLS: usize = 2;
/// The most rows and columns one render lays out, whatever size the viewport
/// reports. Before the stylesheet applies, the viewport is as tall as its
/// canvas — up to 30M px — and a render sized to that is a million rows.
const MAX_RENDER_ROWS: usize = 400;
const MAX_RENDER_COLS: usize = 100;

/// Scroll position and viewport size, written by `onscroll` / `onresize`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub scroll_top: f64,
    pub scroll_left: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        // A plausible first window, so the first paint is not a single row that
        // then reflows. Corrected by the first real scroll or resize event.
        Self {
            scroll_top: 0.0,
            scroll_left: 0.0,
            width: 900.0,
            height: 600.0,
        }
    }
}

/// The half-open row range and column range to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleRange {
    pub rows: std::ops::Range<usize>,
    pub cols: std::ops::Range<usize>,
}

/// Compute the render window, including overscan.
///
/// Pure, and separately tested: this is the arithmetic that decides whether the
/// grid shows the right data, and it is much easier to get wrong than to debug
/// through a window.
pub fn visible_range(vp: Viewport, total_rows: usize, widths: &[f64]) -> VisibleRange {
    let first_row =
        ((vp.scroll_top / ROW_H).floor().max(0.0) as usize).saturating_sub(OVERSCAN_ROWS);
    let last_row = ((((vp.scroll_top + vp.height) / ROW_H).ceil().max(0.0) as usize)
        + OVERSCAN_ROWS)
        .min(total_rows)
        .min(first_row.saturating_add(MAX_RENDER_ROWS));

    // Columns can differ in width, so walk offsets rather than dividing.
    let mut first_col = 0;
    let mut x = 0.0;
    for (i, w) in widths.iter().enumerate() {
        if x + w > vp.scroll_left {
            first_col = i;
            break;
        }
        x += w;
        first_col = i + 1;
    }
    let first_col = first_col.saturating_sub(OVERSCAN_COLS);

    let mut last_col = first_col;
    let mut x = offset_of(widths, first_col);
    let right = vp.scroll_left + vp.width;
    while last_col < widths.len() && x < right {
        x += widths[last_col];
        last_col += 1;
    }
    let last_col = (last_col + OVERSCAN_COLS)
        .min(widths.len())
        .min(first_col.saturating_add(MAX_RENDER_COLS));

    VisibleRange {
        rows: first_row..last_row.max(first_row),
        cols: first_col..last_col.max(first_col),
    }
}

/// Left edge of column `ix`.
///
/// Folded from `0.0` rather than summed: `f64`'s `Sum` identity is `-0.0`, so
/// the first column's offset would render as `left: -0px`.
pub fn offset_of(widths: &[f64], ix: usize) -> f64 {
    widths.iter().take(ix).fold(0.0, |acc, w| acc + w)
}

/// The tallest the canvas gets, kept a tenth under WebKit's clamp. Tables
/// up to 1,153,846 rows scroll one to one.
pub const MAX_CANVAS_H: f64 = 30_000_000.0;

/// How the canvas stands in for the rows at one scroll position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// The canvas's height: the rows' own, up to [`MAX_CANVAS_H`].
    pub canvas_h: f64,
    /// How far each row is drawn above `row × ROW_H`. Zero while the canvas
    /// holds every row at full height.
    pub shift: f64,
}

impl Scale {
    pub fn of(total_rows: usize, vp: &Viewport) -> Self {
        let natural = total_rows as f64 * ROW_H;
        if natural <= MAX_CANVAS_H {
            return Self {
                canvas_h: natural,
                shift: 0.0,
            };
        }
        // Scrolled to the bottom of the canvas means scrolled to the last
        // row: the scrollable span maps onto the rows' span.
        let span = (MAX_CANVAS_H - vp.height).max(1.0);
        let rows_span = (natural - vp.height).max(0.0);
        let rows_top = (vp.scroll_top / span).clamp(0.0, 1.0) * rows_span;
        Self {
            canvas_h: MAX_CANVAS_H,
            shift: rows_top - vp.scroll_top,
        }
    }

    /// Where row `r` is drawn on the canvas.
    pub fn top(&self, r: usize) -> f64 {
        r as f64 * ROW_H - self.shift
    }

    /// The viewport as the rows see it: scrolled to where it looks among
    /// them, which is what decides the rows to render.
    pub fn rows_view(&self, vp: Viewport) -> Viewport {
        Viewport {
            scroll_top: vp.scroll_top + self.shift,
            ..vp
        }
    }
}

/// The scroll position, `(left, top)`, that brings the cell at `(row, col)`
/// wholly into view by the least movement, or `None` when it is in view.
pub fn reveal(
    vp: Viewport,
    total_rows: usize,
    widths: &[f64],
    row: usize,
    col: usize,
) -> Option<(f64, f64)> {
    let natural = total_rows as f64 * ROW_H;
    let rows_top = Scale::of(total_rows, &vp).rows_view(vp).scroll_top;
    let cell_top = row as f64 * ROW_H;
    let want_top = if cell_top < rows_top {
        cell_top
    } else if cell_top + ROW_H > rows_top + vp.height {
        cell_top + ROW_H - vp.height
    } else {
        rows_top
    };

    let left = offset_of(widths, col);
    let right = left + widths.get(col).copied().unwrap_or(COL_W_DEFAULT);
    let want_left = if left < vp.scroll_left {
        left
    } else if right > vp.scroll_left + vp.width {
        (right - vp.width).max(0.0)
    } else {
        vp.scroll_left
    };

    if want_top == rows_top && want_left == vp.scroll_left {
        return None;
    }
    Some((
        want_left,
        scroll_top_for(want_top.max(0.0), natural, vp.height),
    ))
}

/// The canvas scroll position that shows the rows from `rows_top` down: the
/// inverse of [`Scale::of`].
fn scroll_top_for(rows_top: f64, natural: f64, vp_h: f64) -> f64 {
    if natural <= MAX_CANVAS_H {
        return rows_top;
    }
    let span = (MAX_CANVAS_H - vp_h).max(1.0);
    let rows_span = (natural - vp_h).max(1.0);
    (rows_top / rows_span * span).clamp(0.0, span)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(n: usize) -> Vec<f64> {
        vec![COL_W_DEFAULT; n]
    }

    #[test]
    fn the_window_is_tens_of_rows_not_a_million() {
        let vp = Viewport {
            scroll_top: 0.0,
            scroll_left: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let r = visible_range(vp, 1_000_000, &uniform(40));
        // 600 / 26 = 23 visible, + 4 overscan below, + 0 above at the top.
        assert!(r.rows.len() <= 40, "{:?}", r.rows);
        assert_eq!(r.rows.start, 0);
        // 800 / 100 = 8 visible, + 2 overscan.
        assert!(r.cols.len() <= 12, "{:?}", r.cols);
    }

    #[test]
    fn scrolling_moves_the_window_and_keeps_it_small() {
        let vp = Viewport {
            scroll_top: ROW_H * 900_000.0,
            scroll_left: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let r = visible_range(vp, 1_000_000, &uniform(40));
        assert!(r.rows.contains(&900_000), "{:?}", r.rows);
        assert!(!r.rows.contains(&0), "{:?}", r.rows);
        assert!(r.rows.len() <= 40, "{:?}", r.rows);
    }

    #[test]
    fn overscan_is_applied_on_both_sides_once_away_from_the_edge() {
        let vp = Viewport {
            scroll_top: ROW_H * 100.0,
            scroll_left: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let r = visible_range(vp, 1_000_000, &uniform(40));
        assert_eq!(r.rows.start, 100 - OVERSCAN_ROWS);
    }

    #[test]
    fn the_window_never_runs_past_the_data() {
        // A viewport taller than the table must not ask for rows that do not
        // exist — the row loop would index past the end of the source.
        let vp = Viewport {
            scroll_top: 0.0,
            scroll_left: 0.0,
            width: 800.0,
            height: 6000.0,
        };
        let r = visible_range(vp, 3, &uniform(4));
        assert_eq!(r.rows, 0..3);
        assert_eq!(r.cols.end, 4);
    }

    #[test]
    fn a_viewport_as_tall_as_its_canvas_still_renders_a_bounded_window() {
        // Before the stylesheet applies, the viewport is as tall as its
        // canvas; a render sized to that is every row of the table.
        let vp = Viewport {
            scroll_top: 0.0,
            scroll_left: 0.0,
            width: 1_000_000.0,
            height: 30_000_000.0,
        };
        let r = visible_range(vp, 2_000_000, &uniform(5_000));
        assert!(r.rows.len() <= MAX_RENDER_ROWS, "{:?}", r.rows);
        assert!(r.cols.len() <= MAX_RENDER_COLS, "{:?}", r.cols);
    }

    #[test]
    fn an_empty_table_yields_an_empty_window_rather_than_a_panic() {
        let r = visible_range(Viewport::default(), 0, &[]);
        assert!(r.rows.is_empty());
        assert!(r.cols.is_empty());
    }

    #[test]
    fn horizontal_scroll_walks_real_widths_not_an_average() {
        // Columns are resizable, so dividing by a nominal width would show the
        // wrong columns as soon as one is dragged.
        let widths = vec![300.0, 50.0, 50.0, 50.0, 300.0];
        let vp = Viewport {
            scroll_top: 0.0,
            scroll_left: 320.0,
            width: 100.0,
            height: 600.0,
        };
        let r = visible_range(vp, 10, &widths);
        // 320px lands inside column 1 (300..350); minus 2 overscan → 0.
        assert_eq!(r.cols.start, 0);
        assert!(r.cols.contains(&1), "{:?}", r.cols);
    }

    #[test]
    fn offsets_accumulate_real_widths() {
        let w = vec![10.0, 20.0, 30.0];
        assert_eq!(offset_of(&w, 0), 0.0);
        assert_eq!(offset_of(&w, 1), 10.0);
        assert_eq!(offset_of(&w, 3), 60.0);
    }

    fn vp(scroll_top: f64) -> Viewport {
        Viewport {
            scroll_top,
            scroll_left: 0.0,
            width: 800.0,
            height: 600.0,
        }
    }

    #[test]
    fn a_table_under_the_cap_scrolls_one_to_one() {
        let s = Scale::of(1_000_000, &vp(26_000.0));
        assert_eq!(s.canvas_h, 26_000_000.0);
        assert_eq!(s.shift, 0.0);
        assert_eq!(s.top(1_000), 26_000.0);
    }

    /// PD-026's own case: three million rows, on a canvas WebKit would have
    /// clamped at row ~1,290,555.
    #[test]
    fn every_row_of_a_table_past_the_cap_is_reachable() {
        let rows = 3_000_000;
        let top = Scale::of(rows, &vp(0.0));
        assert_eq!(top.canvas_h, MAX_CANVAS_H);
        let first = visible_range(top.rows_view(vp(0.0)), rows, &[100.0]);
        assert_eq!(first.rows.start, 0);

        // Scrolled to the bottom of the canvas: the last row is on screen.
        let bottom = vp(MAX_CANVAS_H - 600.0);
        let s = Scale::of(rows, &bottom);
        let last = visible_range(s.rows_view(bottom), rows, &[100.0]);
        assert_eq!(last.rows.end, rows, "{:?}", last.rows);
        // Drawn inside the viewport, and so inside the canvas.
        let y = s.top(rows - 1);
        assert!(
            y >= bottom.scroll_top && y + ROW_H <= bottom.scroll_top + 600.0 + 0.5,
            "the last row is drawn at {y}, the view is at {}",
            bottom.scroll_top
        );
        assert!(y + ROW_H <= MAX_CANVAS_H + 0.5);
    }

    #[test]
    fn a_row_in_view_needs_no_scroll() {
        assert_eq!(reveal(vp(0.0), 100, &[100.0], 3, 0), None);
    }

    #[test]
    fn a_row_below_the_view_scrolls_to_the_bottom_edge_and_one_above_to_the_top() {
        // Row 100's bottom edge at the viewport's bottom edge.
        assert_eq!(
            reveal(vp(0.0), 1_000, &[100.0], 100, 0),
            Some((0.0, 101.0 * ROW_H - 600.0))
        );
        assert_eq!(
            reveal(vp(50.0 * ROW_H), 1_000, &[100.0], 10, 0),
            Some((0.0, 10.0 * ROW_H))
        );
    }

    #[test]
    fn a_column_past_the_right_edge_scrolls_sideways() {
        let widths = [300.0; 5];
        assert_eq!(reveal(vp(0.0), 10, &widths, 0, 3), Some((400.0, 0.0)));
    }

    #[test]
    fn revealing_the_last_row_of_a_huge_table_scrolls_to_the_canvas_bottom() {
        let rows = 3_000_000;
        let (_, top) = reveal(vp(0.0), rows, &[100.0], rows - 1, 0).expect("a scroll");
        assert!(
            (top - (MAX_CANVAS_H - 600.0)).abs() < 1.0,
            "scrolls to {top}"
        );
        // And the row it asked for is the one that is then in view.
        let at = vp(top);
        let s = Scale::of(rows, &at);
        let range = visible_range(s.rows_view(at), rows, &[100.0]);
        assert!(range.rows.contains(&(rows - 1)), "{:?}", range.rows);
    }
}
