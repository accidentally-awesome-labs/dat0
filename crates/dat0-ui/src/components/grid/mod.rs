//! The virtualized data grid.
//!
//! Structure is the one Phase 0.1 measured at **p95 18 ms scroll-to-repaint**
//! over 1,000,000 rows, and it is load-bearing rather than incidental:
//!
//! ```text
//! div.d0-grid-viewport  overflow:auto, onscroll
//!   div.d0-grid-canvas  sized to the full virtual extent
//!     div.d0-grid-row   position:absolute, top = r * 26px, key = absolute row
//!       div.d0-cell     position:absolute, left = column offset, key = column
//! ```
//!
//! The canvas carries the whole extent so the scrollbar is honest — a
//! "load more" list lies about how much data there is — while only the visible
//! window plus **4 rows / 2 columns of overscan** exists as DOM. At 1 M rows
//! that is ~40 row nodes.
//!
//! # Identity attributes are a contract
//!
//! Every row is `data-a11y-id="row-{absolute_row}"` and every cell
//! `data-a11y-id="cell-{absolute_row}-{col}"`, with **absolute** indices, not
//! window-relative ones. Tests query by them, and a window-relative id would
//! silently address a different row after a scroll.
//!
//! # Mouse selection is new
//!
//! The GPUI grid had none: `render_td` attached no click handler and nothing
//! subscribed to `TableEvent`, so selection was keyboard-and-header-click only.
//! Here `onmousedown` selects, drag extends, ⇧ extends and ⌘/Ctrl adds a range —
//! all through `SelectionModel`, which already had the four methods and their
//! unit tests.

pub mod cell_editor;
pub mod context_menu;
pub mod header;

use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::renderers::CellAlignment;
use dat0_core::grid::selection::{CellCoord, SelectionModel};
use dat0_engine::transform::ProjectionColumn;

/// Row height, and the grid header's height. The design's `26px`.
pub const ROW_H: f64 = 26.0;
/// Default column width, matching the GPUI grid's fixed `px(100.)`.
pub const COL_W_DEFAULT: f64 = 100.0;
/// Rows rendered above and below the viewport.
const OVERSCAN_ROWS: usize = 4;
/// Columns rendered left and right of the viewport.
const OVERSCAN_COLS: usize = 2;

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
        .min(total_rows);

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
    let last_col = (last_col + OVERSCAN_COLS).min(widths.len());

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

/// Everything the grid needs. Held by the shell, so a re-render of the grid
/// does not re-read the engine.
///
/// `PartialEq` is hand-written because `GridDataSource` has none and should not:
/// it owns an LRU of Arrow batches and a DuckDB handle, and structural equality
/// over that is both expensive and meaningless. Pointer identity is the right
/// question — a different `Arc` is a different table.
#[derive(Clone, Props)]
pub struct GridProps {
    pub source: Arc<GridDataSource>,
    pub selection: Signal<SelectionModel>,
    /// Visible columns in display order, from the `ColumnView` fold.
    pub columns: Vec<ProjectionColumn>,
    /// Per-column widths, parallel to `columns`.
    ///
    /// A signal, not a value, because the grid writes it: a resize drag is a
    /// live gesture and round-tripping every pixel through the shell would put
    /// the whole tree in the drag's critical path. The shell observes it and
    /// persists into `ColumnView` — widths used to reset on every reload.
    pub widths: Signal<Vec<f64>>,
    /// Whether the workspace refuses mutations.
    #[props(default = false)]
    pub read_only: bool,
    /// A committed cell edit: `(cell, new text)`. The grid never touches the
    /// engine itself — one place decides what a write means.
    #[props(default)]
    pub on_edit: EventHandler<(CellCoord, String)>,
    /// A context-menu pick: `(action id, the right-clicked cell)`.
    #[props(default)]
    pub on_action: EventHandler<(&'static str, CellCoord)>,
}

impl PartialEq for GridProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && self.selection == other.selection
            && self.columns == other.columns
            && self.widths == other.widths
            && self.read_only == other.read_only
    }
}

/// The grid.
#[component]
pub fn Grid(props: GridProps) -> Element {
    let mut viewport = use_signal(Viewport::default);
    // True while the primary button is held on the grid body, so
    // `onmouseenter` on a cell extends rather than hovers.
    let mut dragging = use_signal(|| false);

    // A live resize or reorder gesture: `(column, pointer origin, width at
    // grab)` / the column being dragged.
    let mut resizing = use_signal(|| Option::<(usize, f64, f64)>::None);
    let mut reordering = use_signal(|| Option::<usize>::None);
    // The cell being edited, and where the context menu is open.
    let mut editing = use_signal(|| Option::<CellCoord>::None);
    let mut menu_at = use_signal(|| Option::<(f64, f64)>::None);

    let source = props.source.clone();
    let total_rows = source.row_count as usize;
    let mut widths_sig = props.widths;
    let widths = widths_sig();
    let total_w: f64 = widths.iter().sum();
    let total_h = total_rows as f64 * ROW_H;

    let range = visible_range(viewport(), total_rows, &widths);

    // Page ahead for what is on screen. The residency probe is the same cheap
    // guard the GPUI path used: if both boundary pages are already cached the
    // synchronous render already paints real values, so there is nothing to
    // fetch and no task to spawn. Without it a fast scroll spawns a task per
    // frame over data it already has.
    //
    // `pages_loaded` is what makes the fetch visible. The LRU behind
    // `GridDataSource` is a `Mutex`, not a signal, so filling it changes
    // nothing Dioxus is watching: the cells that rendered before the page
    // landed keep their `–` placeholder until something unrelated happens to
    // re-render the grid. Bumping a signal the render reads is the whole fix,
    // and it is why the counter is read one line below rather than in the
    // effect — a scope only subscribes to what its RENDER touches.
    let mut pages_loaded = use_signal(|| 0_u64);
    let _ = pages_loaded();
    {
        let source = props.source.clone();
        let (start, last) = (range.rows.start, range.rows.end.saturating_sub(1));
        use_effect(move || {
            // Read inside the effect so a scroll re-runs it: `use_effect`
            // re-runs on the signals its body touches, and `start`/`last` are
            // plain values computed during render. Without this the grid
            // prefetches exactly once, at mount, and every page below the
            // first screenful stays blank however far you scroll.
            let _ = viewport();
            if start > last || source.pages_resident(start, last) {
                return;
            }
            let source = source.clone();
            spawn(async move {
                // DuckDB I/O, never on the render path.
                for row in [start as u64, last as u64] {
                    if let Err(e) = source.page_for(row).await {
                        tracing::warn!("grid page {row} failed: {e:#}");
                    }
                }
                // The cache is not reactive; this is the repaint.
                let next = pages_loaded().wrapping_add(1);
                pages_loaded.set(next);
            });
        });
    }

    let mut selection = props.selection;
    let columns = props.columns.clone();
    let n_cols = columns.len();
    let read_only = props.read_only;
    let on_edit = props.on_edit;
    let on_action = props.on_action;

    // The cell under edit, resolved once. The seed text and the widget choice
    // both hang off the same column, and looking that column up twice is how
    // an editor ends up showing one cell's value with another cell's type.
    let edit_ctx = editing().map(|coord| {
        let src = columns
            .get(coord.col)
            .map(|c| c.source.as_str())
            .unwrap_or("");
        (
            coord,
            source
                .cell_display_for_source(coord.row, src)
                .unwrap_or_default(),
            source
                .column_type_for_source(src)
                .unwrap_or(dat0_core::view::filter_popover::ColumnType::String),
        )
    });

    rsx! {
        div { class: "d0-grid", "data-a11y-id": "grid", role: "grid",
            header::Header {
                columns: columns.clone(),
                widths: widths.clone(),
                scroll_left: viewport().scroll_left,
                dragging_col: reordering(),
                on_resize_start: move |(col, x): (usize, f64)| {
                    let w = widths_sig.read().get(col).copied().unwrap_or(COL_W_DEFAULT);
                    resizing.set(Some((col, x, w)));
                },
                on_reorder_start: move |col: usize| reordering.set(Some(col)),
                on_reorder_drop: move |to: usize| {
                    if let Some(from) = reordering.take() {
                        if from != to && from < n_cols && to < n_cols {
                            let mut w = widths_sig.write();
                            let moved = w.remove(from);
                            w.insert(to, moved);
                        }
                    }
                },
            }

            // While a pointer gesture is live, a full-window shield takes every
            // move and the release.
            //
            // The alternative is a `document`-level listener installed through
            // `document::eval`, which is what the plan sketched. A shield keeps
            // the whole gesture inside the Dioxus event system: no JS, no
            // channel to leak on unmount, and — the reason that matters here —
            // the headless harness can drive a resize by dispatching at the
            // shield, so the clamp is covered by a test rather than by hand.
            if resizing().is_some() {
                div {
                    class: "d0-drag-shield",
                    "data-a11y-id": "drag-shield",
                    onmousemove: move |e| {
                        let Some((col, origin, start_w)) = resizing() else { return };
                        let x = e.data().client_coordinates().x;
                        let next = header::resized(start_w, x - origin);
                        let mut w = widths_sig.write();
                        if let Some(slot) = w.get_mut(col) {
                            *slot = next;
                        }
                    },
                    onmouseup: move |_| resizing.set(None),
                }
            }

            div {
                class: "d0-grid-viewport",
                "data-a11y-id": "grid-viewport",
                tabindex: "0",
                onscroll: move |e| {
                    let d = e.data();
                    viewport.set(Viewport {
                        scroll_top: d.scroll_top(),
                        scroll_left: d.scroll_left(),
                        width: f64::from(d.client_width()),
                        height: f64::from(d.client_height()),
                    });
                },
                onmouseup: move |_| dragging.set(false),
                onmouseleave: move |_| dragging.set(false),
                // The grid's cursor grammar. Not part of the shell's chord
                // cascade: arrow keys mean "move the cursor" here and something
                // else everywhere else, which is the definition of a modal
                // surface. `stop_propagation` on a hit keeps an arrow from also
                // scrolling a palette behind us.
                onkeydown: move |e| {
                    if editing().is_some() {
                        return;
                    }
                    if let Some(k) = crate::keys::grid_key(&e.key(), e.modifiers()) {
                        e.prevent_default();
                        e.stop_propagation();
                        dat0_core::grid::keymap::apply_key(&mut selection.write(), k);
                        return;
                    }
                    // Enter opens the editor on the active cell, the
                    // spreadsheet convention.
                    if e.key() == Key::Enter && !read_only {
                        e.prevent_default();
                        editing.set(Some(selection.read().active()));
                    }
                },

                div {
                    class: "d0-grid-canvas",
                    style: "width: {total_w}px; height: {total_h}px;",
                    // The scroll position this DOM was built for.
                    //
                    // Only the perf harness reads it, and it is what makes
                    // scroll-to-repaint measurable at all: it lets the driver
                    // pair a rendered frame with the scroll event that caused
                    // it. Timing an unqualified MutationObserver instead
                    // reports ~0 ms, because the mutation for scroll N lands
                    // just after scroll N+1's timestamp — it measures the
                    // wrong pair. One attribute on one element per render.
                    "data-top": "{viewport().scroll_top}",
                    oncontextmenu: move |e| {
                        e.prevent_default();
                        let p = e.data().client_coordinates();
                        menu_at.set(Some((p.x, p.y)));
                    },

                    for r in range.rows.clone() {
                        div {
                            key: "{r}",
                            class: "d0-grid-row",
                            "data-a11y-id": "row-{r}",
                            role: "row",
                            "aria-rowindex": "{r + 1}",
                            style: "top: {r as f64 * ROW_H}px; width: {total_w}px;",

                            for c in range.cols.clone() {
                                {cell(
                                    &source,
                                    &columns,
                                    &widths,
                                    r,
                                    c,
                                    &selection.read(),
                                    move |ev: MouseEvent, coord| {
                                        let m = ev.modifiers();
                                        let mut s = selection.write();
                                        if m.shift() {
                                            s.extend_to(coord);
                                        } else if m.meta() || m.ctrl() {
                                            s.add_click(coord);
                                        } else {
                                            s.click(coord);
                                        }
                                        dragging.set(true);
                                    },
                                    move |coord| {
                                        if dragging() {
                                            selection.write().extend_to(coord);
                                        }
                                    },
                                )}
                            }
                        }
                    }

                    if let Some((coord, initial, column_type)) = edit_ctx {
                        cell_editor::CellEditor {
                            cell: coord,
                            initial,
                            column_type,
                            widths: widths.clone(),
                            on_done: move |outcome| {
                                editing.set(None);
                                if let cell_editor::EditOutcome::Commit { value, move_by } = outcome {
                                    on_edit.call((coord, value));
                                    let (dr, dc) = move_by;
                                    if dr != 0 || dc != 0 {
                                        selection.write().move_active(dr, dc);
                                    }
                                }
                            },
                        }
                    }
                }
            }

            if let Some(at) = menu_at() {
                context_menu::ContextMenu {
                    at,
                    cell: selection.read().active(),
                    has_selection: selection.read().has_selection(),
                    read_only,
                    on_pick: move |(id, coord)| {
                        menu_at.set(None);
                        on_action.call((id, coord));
                    },
                    on_dismiss: move |_| menu_at.set(None),
                }
            }
        }
    }
}

/// One cell.
///
/// A free function rather than a component: a component per cell would add a
/// scope, a props comparison and a memo per cell per frame, which at ~400 live
/// cells is exactly the overhead the Phase-0 budget has no room for.
#[allow(clippy::too_many_arguments)]
fn cell(
    source: &Arc<GridDataSource>,
    columns: &[ProjectionColumn],
    widths: &[f64],
    row: usize,
    col: usize,
    selection: &SelectionModel,
    on_down: impl FnMut(MouseEvent, CellCoord) + 'static + Clone,
    on_enter: impl FnMut(CellCoord) + 'static + Clone,
) -> Element {
    let Some(column) = columns.get(col) else {
        return rsx! {};
    };
    let left = offset_of(widths, col);
    let width = widths.get(col).copied().unwrap_or(COL_W_DEFAULT);

    // A page that is not resident renders the placeholder rather than blocking
    // the render loop on DuckDB.
    let display = source.cell_render_for_source(row, &column.source);
    let (text, right, is_null) = match &display {
        Some(d) => (
            d.text.clone(),
            matches!(d.alignment, CellAlignment::Right),
            d.is_null,
        ),
        None => ("—".to_string(), false, false),
    };

    let selected = selection.contains(row, col);
    let active = selection.active() == CellCoord { row, col };

    let mut class = String::with_capacity(48);
    class.push_str("d0-cell");
    if right {
        class.push_str(" is-right");
    }
    if is_null {
        class.push_str(" is-null");
    }
    if selected {
        class.push_str(" is-selected");
    }
    if active {
        class.push_str(" is-active");
    }

    let coord = CellCoord { row, col };
    let mut down = on_down.clone();
    let mut enter = on_enter.clone();
    let label = format!("{}: {}", column.display, text);

    rsx! {
        div {
            key: "{col}",
            class: "{class}",
            "data-a11y-id": "cell-{row}-{col}",
            role: "gridcell",
            "aria-label": "{label}",
            "aria-colindex": "{col + 1}",
            tabindex: "-1",
            style: "left: {left}px; width: {width}px;",
            onmousedown: move |e| down(e, coord),
            onmouseenter: move |_| enter(coord),
            if is_null { "NULL" } else { "{text}" }
        }
    }
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
}
