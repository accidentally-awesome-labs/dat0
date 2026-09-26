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
pub mod edits;
pub mod export;
pub mod header;
pub mod refresh;
pub mod scroll;
pub mod views;

use std::rc::Rc;
use std::sync::Arc;

use dioxus::html::geometry::PixelsVector2D;
use dioxus::prelude::*;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::renderers::CellAlignment;
use dat0_core::grid::selection::{CellCoord, SelectionModel};
use dat0_engine::transform::ProjectionColumn;

pub use scroll::{Viewport, VisibleRange, offset_of, visible_range};

/// Row height, and the grid header's height. The design's `26px`.
pub const ROW_H: f64 = 26.0;
/// Default column width, matching the GPUI grid's fixed `px(100.)`.
pub const COL_W_DEFAULT: f64 = 100.0;
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
    /// Each column's sort and filter state, for the header.
    #[props(default)]
    pub marks: Vec<views::Mark>,
    /// A sort-zone click, with Shift: `(column, extend)`.
    #[props(default)]
    pub on_sort: EventHandler<(usize, bool)>,
    /// A funnel click: `(column, client x, client y)`.
    #[props(default)]
    pub on_funnel: EventHandler<(usize, f64, f64)>,
    /// A header dragged to a new place: `(from, to)`.
    #[props(default)]
    pub on_reorder: EventHandler<(usize, usize)>,
}

impl PartialEq for GridProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && self.selection == other.selection
            && self.columns == other.columns
            && self.widths == other.widths
            && self.read_only == other.read_only
            && self.marks == other.marks
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
    // Capped past ~1.15M rows, where the scroll position maps to rows in
    // proportion: WebKit cannot lay out a taller canvas (PD-026).
    let sc = scroll::Scale::of(total_rows, &viewport());
    let total_h = sc.canvas_h;

    let range = visible_range(sc.rows_view(viewport()), total_rows, &widths);

    // The scrolling element, once the renderer has one: `None` in the headless
    // harness, which has no layout and nothing to scroll.
    let mut viewport_el = use_signal(|| Option::<Rc<MountedData>>::None);
    // Keep the cursor on screen after a keyboard move. The viewport signal is
    // set at once, so the rows render where the cursor went; the element is
    // scrolled to match. Without this, Ctrl+End moved the cursor to the last
    // row and left the view where it was.
    let mut reveal = move |at: CellCoord| {
        let vp = viewport();
        let widths = widths_sig.peek().clone();
        let Some((left, top)) = scroll::reveal(vp, total_rows, &widths, at.row, at.col) else {
            return;
        };
        viewport.set(Viewport {
            scroll_left: left,
            scroll_top: top,
            ..vp
        });
        if let Some(el) = viewport_el() {
            spawn(async move {
                let _ = el
                    .scroll(PixelsVector2D::new(left, top), ScrollBehavior::Instant)
                    .await;
            });
        }
    };

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
        // The source's identity is a dependency too. A mounted grid is handed
        // a new source when the tab changes or a query reruns in place, and an
        // effect that watched only the viewport went on checking the old
        // source's cache: the new table sat on placeholders until a scroll.
        let bound = Arc::as_ptr(&source) as usize;
        use_effect(use_reactive!(|bound| {
            let _ = bound;
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
        }));
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
                            // The owner moves the column; its width follows
                            // the column, not the place (`views::use_fit`).
                            props.on_reorder.call((from, to));
                        }
                    }
                },
                marks: props.marks.clone(),
                on_sort: props.on_sort,
                on_funnel: props.on_funnel,
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
                onmounted: move |e| viewport_el.set(Some(e.data())),
                // Its size as laid out, rather than `Viewport::default` until
                // the first scroll: on first layout, when the stylesheet lands,
                // and whenever the window or a pane beside it resizes.
                onresize: move |e| {
                    if let Ok(size) = e.data().get_content_box_size() {
                        let mut vp = viewport.write();
                        vp.width = size.width;
                        vp.height = size.height;
                    }
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
                        reveal(selection.peek().active());
                        return;
                    }
                    if let Some(id) = crate::keys::grid_verb(&e.key(), e.modifiers()) {
                        e.prevent_default();
                        e.stop_propagation();
                        let at = selection.read().active();
                        on_action.call((id, at));
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
                            style: "top: {sc.top(r)}px; width: {total_w}px;",

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
                                        // A right-click inside the selection keeps it, so
                                        // the context menu acts on all of it; outside, it
                                        // selects the cell it landed on. On macOS a
                                        // Ctrl-click is that right-click, so only Cmd adds.
                                        let mac = cfg!(target_os = "macos");
                                        let secondary = ev.trigger_button()
                                            == Some(dioxus::html::input_data::MouseButton::Secondary)
                                            || (mac && m.ctrl());
                                        if secondary {
                                            if !s.contains(coord.row, coord.col) {
                                                s.click(coord);
                                            }
                                            return;
                                        }
                                        if m.shift() {
                                            s.extend_to(coord);
                                        } else if m.meta() || (!mac && m.ctrl()) {
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
                            shift: sc.shift,
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
