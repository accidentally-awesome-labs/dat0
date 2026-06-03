//! DataGrid: gpui-component Table wrapper over duckdb::arrow batches.

pub mod cell_editor;
pub mod clipboard;
pub mod context_menu;
pub mod data_source;
pub mod edit_ops;
pub mod keymap;
pub mod renderers;
pub mod selection;

pub use data_source::GridDataSource;

use std::sync::Arc;

use gpui::{
    App, Context, IntoElement, ParentElement, Pixels, Point, SharedString, Styled, WeakEntity,
    Window, div, prelude::*,
};
use gpui_component::table::{Column, TableDelegate, TableState};

use renderers::{CellAlignment, type_badge};

// ── Internal drag payload for column-header reorder (T6) ─────────────────────
//
// GPUI 0.2.2 `on_drag` / `on_drop` require a payload type that:
//   1. Is `Clone` (GPUI clones the value on drag start).
//   2. Implements `Render` (GPUI renders the drag ghost from this entity).
//
// Pattern mirrors the `DragInfo` struct in
// `gpui-0.2.2/examples/drag_drop.rs` (the real internal-drag example):
//   `.on_drag(value, |val, position, _, cx| cx.new(|_| val.position(position)))`
// The drag source calls the constructor; `on_drop` receives a `&ReorderDrag`.

/// Drag payload for a column-header reorder gesture.
///
/// `from` is the screen column index that started the drag. On drop the
/// target header's index is used as `to`, and `WorkspaceShell::on_reorder_columns`
/// is called to apply the display-only `Reorder` transform.
#[derive(Clone, Copy)]
pub(crate) struct ReorderDrag {
    /// Screen column index of the dragged header.
    pub from: usize,
    /// Current drag-ghost position (updated by the `on_drag` constructor so
    /// the ghost follows the pointer).
    position: Point<Pixels>,
}

impl ReorderDrag {
    fn new(from: usize) -> Self {
        Self {
            from,
            position: Point::default(),
        }
    }

    fn with_position(mut self, pos: Point<Pixels>) -> Self {
        self.position = pos;
        self
    }
}

impl gpui::Render for ReorderDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        // Ghost: a small labelled pill that follows the pointer. Offset from the
        // pointer so the drop-target hit test lands on the target rather than on
        // the ghost itself (mirroring the drag_drop.rs example's pl/pt approach).
        div().pl(self.position.x).pt(self.position.y).child(
            div()
                .px_2()
                .py_1()
                .bg(gpui::rgba(0x3b82f6aa))
                .text_color(gpui::white())
                .text_xs()
                .rounded_md()
                .child(format!("col {}", self.from)),
        )
    }
}

// ── Four-zone header geometry ─────────────────────────────────────────────────
//
// The column header is a horizontal flex row laid out as:
//
//   [ grip-stub | body (column name) | sort-icon | funnel-icon ]
//
// Grip and body are positional sub-elements whose visual widths are defined
// below as named constants.  Sort and funnel are icon-sized hit targets on the
// right edge.
//
// Why not use raw x-offset math here?  `render_th` runs inside GPUI's flex
// layout pass; we don't have absolute bounds until after paint.  The four-zone
// split is therefore expressed as flex children rather than as an
// x-position → zone mapping.  A pure zone-from-x helper (for hit-testing in
// tests or future pointer-event work) is provided alongside these constants.
//
//   Grip → column drag-reorder (P4c T6, live)
//   Body single-click → column select (P4c T13, live)
//   Body double-click → inline header rename (P4c T7, live)

/// Width of the left-edge drag-grip in logical pixels.
/// P4c (column resize) will replace the invisible stub with a real handle.
/// Kept here as a single source of truth for the geometry constant.
pub const HEADER_GRIP_PX: f32 = 6.0;

/// Width of the right-edge funnel icon hit-target in logical pixels.
pub const HEADER_FUNNEL_PX: f32 = 20.0;

/// Width of the sort-icon hit-target in logical pixels (sits left of funnel).
pub const HEADER_SORT_PX: f32 = 20.0;

/// Classify an x-offset (measured from the left edge of the header cell) into
/// a zone, given the total cell width.  Used for unit tests; the actual render
/// uses flex children rather than raw x offsets.
///
/// Zone boundaries (left → right):
///   `0 .. HEADER_GRIP_PX`                              → Grip
///   `HEADER_GRIP_PX .. (cell_width - HEADER_SORT_PX - HEADER_FUNNEL_PX)` → Body
///   `(cell_width - HEADER_SORT_PX - HEADER_FUNNEL_PX) .. (cell_width - HEADER_FUNNEL_PX)` → Sort
///   `(cell_width - HEADER_FUNNEL_PX) .. cell_width`    → Funnel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnHeaderZone {
    /// Left-edge resize grip.  Stub: no-op click (P4c column-resize will fill).
    Grip,
    /// Column-name / body click area.  Stub: no-op click (future P4b row-selection will fill).
    Body,
    /// Sort-direction toggle icon.  Live in P4a (cycles Asc/Desc via T12).
    Sort,
    /// Filter funnel icon.  Live in P4a (opens popover via T10).
    Funnel,
}

/// Map an x-offset within a header cell to the appropriate [`ColumnHeaderZone`].
///
/// `cell_width` is the full logical-pixel width of the cell including padding.
/// `x` is the cursor/click offset measured from the left edge of the cell.
///
/// Values outside `[0, cell_width]` clamp to the nearest edge zone (Grip or
/// Funnel) rather than panicking — callers should not rely on out-of-range
/// inputs but the function is total.
pub fn zone_from_x(x: f32, cell_width: f32) -> ColumnHeaderZone {
    let funnel_start = cell_width - HEADER_FUNNEL_PX;
    let sort_start = funnel_start - HEADER_SORT_PX;
    if x < HEADER_GRIP_PX {
        ColumnHeaderZone::Grip
    } else if x < sort_start.max(HEADER_GRIP_PX) {
        ColumnHeaderZone::Body
    } else if x < funnel_start.max(sort_start.max(HEADER_GRIP_PX)) {
        ColumnHeaderZone::Sort
    } else {
        ColumnHeaderZone::Funnel
    }
}

/// Adapter that bridges [`GridDataSource`] (Arrow-paged, interior-mutable
/// cache) to gpui-component's `TableDelegate` (main-thread, `&mut self`,
/// synchronous render contract).
///
/// Per `docs/internal/gpui-table-api-notes.md` §4.1: the delegate is
/// `Sized + 'static` — no `Send`/`Sync` bounds — so it lives on the GPUI
/// main thread alongside the parent view. The `Arc<GridDataSource>` field
/// is the only cross-thread handle; pages are fetched off-thread by the
/// async paging layer wired up in PD-018 (`prefetch_visible_rows`).
///
/// As of PD-018 this is a fully paged delegate: `render_td` reads real values
/// from the LRU page cache via [`GridDataSource::cell_render`] (synchronous,
/// never triggers a DuckDB fetch). Pages that haven't loaded yet fall back to
/// an em-dash placeholder for that cell only; real values appear as pages
/// stream in via the background prefetch path.
pub struct GridTableDelegate {
    /// Shared paging source. `Arc` so the same source can be inspected by
    /// other views (e.g., status bar row-count badge in a later task).
    source: Arc<GridDataSource>,
    /// Pre-computed column metadata. Cloned cheap (per spike §2) so we can
    /// hand a `&Column` to the widget on each `column()` call without
    /// re-deriving from the Arrow schema every frame.
    columns: Vec<Column>,
    /// Weak handle to the owning `WorkspaceShell` (T0 / PD-016). The header
    /// sort/funnel click closures upgrade this and dispatch into
    /// `WorkspaceShell::on_sort_zone_click` / `on_funnel_click`. Weak so the
    /// delegate never keeps the shell alive; `None` only in unit tests that
    /// build a delegate without a shell.
    ws: WeakEntity<crate::window::WorkspaceShell>,
}

impl GridTableDelegate {
    /// Build a delegate over `source`, deriving column metadata from the active
    /// `ColumnView` (`column_view`) — its display label, order, and the deletes
    /// it excludes (P4c T5). Each column's name carries the type-badge suffix of
    /// its underlying Arrow field (e.g., `id (INT64)`); the badge is resolved by
    /// the column's SOURCE identity, so a display-only reorder keeps the right
    /// badge with the right column.
    ///
    /// When `column_view` is empty — no data source bound yet (pre-bind), a
    /// delegate built without a shell in unit tests, OR a genuine
    /// zero-visible-column state — the columns fall back to the raw VISIBLE
    /// schema order (which is itself empty in the zero-column case), the identity
    /// the pre-P4c delegate produced, so existing behaviour and tests are
    /// unchanged. The hidden `__dat0_rowid` surrogate is never a `ColumnView`
    /// source and never a visible field, so it never paints either way.
    ///
    /// `ws` is a weak handle to the owning `WorkspaceShell` so the header
    /// sort/funnel click closures (T0 / PD-016) can dispatch into it. Tests
    /// that build a delegate without a shell pass `WeakEntity::new_invalid()`
    /// (and `&[]` for `column_view`).
    pub fn new(
        source: Arc<GridDataSource>,
        ws: WeakEntity<crate::window::WorkspaceShell>,
        column_view: &[dat0_engine::transform::ProjectionColumn],
    ) -> Self {
        let schema = source.schema.clone();
        let columns: Vec<Column> = if column_view.is_empty() {
            // Identity fallback: derive from the VISIBLE schema fields in schema
            // order (the pre-P4c behaviour). The hidden `__dat0_rowid` surrogate
            // is excluded by `schema_index_for_visible`.
            (0..source.visible_column_count())
                .filter_map(|visible_ix| {
                    let schema_ix = source.schema_index_for_visible(visible_ix)?;
                    let f = schema.fields().get(schema_ix)?;
                    let badge = type_badge(f.data_type());
                    let name: SharedString = format!("{} ({})", f.name(), badge).into();
                    let key: SharedString = f.name().to_string().into();
                    Some(Column::new(key, name))
                })
                .collect()
        } else {
            // ColumnView-driven: display label + order from the fold; the badge
            // is resolved off the SOURCE field so a reorder keeps it aligned. The
            // `key` stays the SOURCE identity (the header renders `name`, the
            // display label). A source absent from the schema (defensive) is
            // skipped rather than panicking.
            column_view
                .iter()
                .filter_map(|c| {
                    let schema_ix = source.schema_index_for_source(&c.source)?;
                    let f = schema.fields().get(schema_ix)?;
                    let badge = type_badge(f.data_type());
                    let name: SharedString = format!("{} ({})", c.display, badge).into();
                    let key: SharedString = c.source.clone().into();
                    Some(Column::new(key, name))
                })
                .collect()
        };
        Self {
            source,
            columns,
            ws,
        }
    }

    /// `Arc::ptr_eq` shortcut for the parent view's "rebuild on data-source
    /// swap" check.
    pub fn source_ptr_eq(&self, other: &Arc<GridDataSource>) -> bool {
        Arc::ptr_eq(&self.source, other)
    }

    // ── Four-zone header sub-renderers ────────────────────────────────────────

    /// Renders the sort-icon zone (right-of-center).
    ///
    /// T0 (PD-016) live: the on_click closure reads the shift modifier from the
    /// GPUI `ClickEvent` and dispatches into
    /// [`crate::window::WorkspaceShell::on_sort_zone_click`], which runs the
    /// `current_sort_as_active → click/shift_click → set_sort` cycle and the
    /// `spawn_view_change` engine round-trip.
    ///
    /// The element must carry a unique `id` so GPUI can track click events
    /// across reframes; we encode `("th-sort", col_ix)`.
    fn render_sort_icon(&self, col_ix: usize) -> impl IntoElement {
        // "⇅" is the neutral sort indicator. A later P4b polish task can swap
        // in "↑"/"↓" once ActiveSort state is read back into the delegate.
        let ws = self.ws.clone();
        div()
            .id(("th-sort", col_ix))
            .w(gpui::px(HEADER_SORT_PX))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(move |ev, _window, cx| {
                let shift = ev.modifiers().shift;
                if let Some(h) = ws.upgrade() {
                    h.update(cx, |ws, cx| ws.on_sort_zone_click(col_ix, shift, cx));
                }
            })
            .child("⇅")
    }

    /// Renders the funnel-icon zone (rightmost).
    ///
    /// T0 (PD-016) live: clicking dispatches into
    /// [`crate::window::WorkspaceShell::on_funnel_click`], which mounts the
    /// filter popover for `col_ix` and routes its `Outcome` back into the
    /// ViewModel + engine round-trip.
    ///
    /// The element must carry a unique `id`; we encode `("th-funnel", col_ix)`.
    fn render_funnel_icon(&self, col_ix: usize) -> impl IntoElement {
        let ws = self.ws.clone();
        div()
            .id(("th-funnel", col_ix))
            .w(gpui::px(HEADER_FUNNEL_PX))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(move |_ev, window, cx| {
                if let Some(h) = ws.upgrade() {
                    h.update(cx, |ws, cx| ws.on_funnel_click(col_ix, window, cx));
                }
            })
            .child("⌄")
    }
}

impl TableDelegate for GridTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        // `GridDataSource::row_count` is a `u64`; the delegate API is `usize`.
        // Saturate on the (extremely unlikely) 32-bit overflow case rather
        // than panic — DuckDB tables larger than `usize::MAX` rows are not
        // representable in the gpui widget anyway.
        usize::try_from(self.source.row_count).unwrap_or(usize::MAX)
    }

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        &self.columns[col_ix]
    }

    /// Four-zone column header per P4a spec §6.
    ///
    /// Layout (left → right):
    ///   1. **Grip** — invisible `HEADER_GRIP_PX`-wide strip.  Live (P4c T6):
    ///      drag source + drop target for column reorder.
    ///   2. **Body** — column-name text, flex-grows to fill remaining space.
    ///      Single-click live (P4c T13): selects the whole column.
    ///      Double-click live (P4c T7): opens inline header rename.
    ///   3. **Sort icon** — `HEADER_SORT_PX`-wide `⇅` button.  Live (P4a):
    ///      dispatches to `WorkspaceShell::on_sort_zone_click` (Asc/Desc/None).
    ///   4. **Funnel icon** — `HEADER_FUNNEL_PX`-wide `⌄` button.  Live (P4a):
    ///      dispatches to `WorkspaceShell::on_funnel_click` (filter popover).
    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let col_name = self.columns[col_ix].name.clone();

        // ── Zone 1: Grip — drag source + drop target (T6) ────────────────────
        //
        // API: GPUI 0.2.2 internal-drag pattern from
        // `gpui-0.2.2/examples/drag_drop.rs`:
        //   `.on_drag(value, |val, position, _, cx| cx.new(|_| val.with_position(position)))`
        //   `.on_drop(cx.listener(|delegate, info: &ReorderDrag, _, _| ...))` on the target.
        //
        // The grip is both a drag SOURCE (starts a drag carrying `col_ix`) and a
        // drop TARGET (receives a drop from another header and applies the reorder).
        // Keeping them on the same zone means any header-to-header drag is handled
        // cleanly — the user grabs the left stub and drops over another header's stub.
        let ws_for_drop = self.ws.clone();
        let grip = div()
            .id(("th-grip", col_ix))
            .w(gpui::px(HEADER_GRIP_PX))
            .h_full()
            .cursor_move()
            // Drag source: carry this column's index as the payload.
            .on_drag(
                ReorderDrag::new(col_ix),
                |drag: &ReorderDrag, position, _, cx| cx.new(|_| drag.with_position(position)),
            )
            // Drop target: when a ReorderDrag lands here, call on_reorder_columns.
            .on_drop(cx.listener(move |_delegate, drag: &ReorderDrag, _, cx| {
                let from = drag.from;
                let to = col_ix;
                if let Some(h) = ws_for_drop.upgrade() {
                    h.update(cx, |ws, cx| ws.on_reorder_columns(from, to, cx));
                }
            }));

        // ── Zone 2: Body / column name ───────────────────────────────────────
        //
        // If a header-rename editor is active for this column, render the editor
        // in-place instead of the label (P4c T7). Otherwise render the label with
        // an on_click handler: single-click → select the whole column (P4c T13);
        // double-click → begin inline rename (P4c T7).
        let ws_for_body = self.ws.clone();
        let rename_editor: Option<gpui::AnyElement> = self
            .ws
            .upgrade()
            .and_then(|h| h.read(cx).header_rename_for(col_ix))
            .map(|e| e.into_any_element());

        let body = div()
            .id(("th-body", col_ix))
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .overflow_hidden()
            .on_click(move |ev, _window, cx| {
                // Single-click → select the whole column (P4c T13).
                // Double-click → begin inline rename (P4c T7).
                if ev.click_count() == 2 {
                    if let Some(h) = ws_for_body.upgrade() {
                        h.update(cx, |ws, cx| ws.begin_column_rename(col_ix, cx));
                    }
                } else if ev.click_count() == 1 {
                    if let Some(h) = ws_for_body.upgrade() {
                        h.update(cx, |ws, cx| ws.select_column_at(col_ix, cx));
                    }
                }
            });

        let body = if let Some(editor_el) = rename_editor {
            // Editor active for this column: render it in-place.
            body.child(editor_el)
        } else {
            // No editor: render the column label.
            body.child(col_name)
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h_full()
            .child(grip)
            .child(body)
            // ── Zone 3: Sort icon (live — T12 replaces closure) ───────────────
            .child(self.render_sort_icon(col_ix))
            // ── Zone 4: Funnel icon (live — T10 replaces closure) ─────────────
            .child(self.render_funnel_icon(col_ix))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // PD-018: `render_td` reads the real value from the LRU page cache
        // synchronously (the render never triggers a DuckDB fetch). When the
        // row's page is already cached — prefetched on bind / on scroll via the
        // background path — the cell paints its real value with numeric
        // right-alignment and NULL styling. When the page hasn't loaded yet, we
        // fall back to the em-dash placeholder for THAT cell only; the
        // virtualized-table pattern means real values appear as pages load.
        //
        // `col_ix` is a DISPLAY-order index into `self.columns` (the `ColumnView`
        // fold's order, P4c). We resolve it to the column's stable SOURCE identity
        // (`columns[col_ix].key`, set to `ProjectionColumn::source`) and read by
        // source via `cell_render_for_source`. This is the body-cell mirror of the
        // header/addressing reroute: under a `Reorder`/`DeleteColumn` the display
        // ordinal no longer equals the schema ordinal, so reading by `col_ix`
        // directly would paint the WRONG column's data under each header.
        //
        // Identity / pre-bind fallback: when `columns[col_ix]` is out of range or
        // carries no source (the empty-`column_view` identity build, or a delegate
        // built without a shell in unit tests), fall back to the index-based
        // `cell_render(row_ix, col_ix)` — under that build display ordinal ==
        // visible/schema ordinal, so behaviour is unchanged.
        let cell = match self.columns.get(col_ix) {
            Some(col) if !col.key.is_empty() => {
                self.source.cell_render_for_source(row_ix, col.key.as_ref())
            }
            _ => self.source.cell_render(row_ix, col_ix),
        };

        // PD-018 focus ring: now that cells paint real values we can anchor a
        // per-cell ring on the active selection cell (replacing the T11
        // bottom-left floating badge). Read the live selection through the weak
        // `WorkspaceShell` handle — a cheap `read` per cell; selection sizes are
        // small and this is the only place the active/selected state is known at
        // cell-render time. Tests build the delegate with an invalid `ws`, so
        // `upgrade` returns `None` and no ring is drawn.
        //
        // T12: also read `copied_range` for the marching-ants dashed border.
        let (is_active, is_selected, copied) = self
            .ws
            .upgrade()
            .map(|ws| {
                let shell = ws.read(cx);
                let (active, selected) = shell
                    .selection
                    .as_ref()
                    .map(|sel| {
                        let active = sel.active();
                        (
                            active.row == row_ix && active.col == col_ix,
                            sel.contains(row_ix, col_ix),
                        )
                    })
                    .unwrap_or((false, false));
                (active, selected, shell.copied_range)
            })
            .unwrap_or((false, false, None));

        // ElementId only accepts (&str, usize) tuples (gpui 0.2.2), not
        // 3-tuples. Encode (row_ix, col_ix) into a single index so each
        // cell still gets a unique id.
        let cell_ix = row_ix
            .saturating_mul(self.columns.len())
            .saturating_add(col_ix);

        let mut el = div().id(("td", cell_ix)).size_full().flex().items_center();

        // Selected-cell tint (lighter than the active ring) so a multi-cell
        // selection is visible for UAT.
        if is_selected && !is_active {
            el = el.bg(gpui::rgba(0x3b82f622));
        }

        // T12: Marching-ants dashed border on the boundary cells of the last
        // copied/cut range. `copied` is in screen-space (same space as
        // `col_ix`); we apply a 1-px dashed green border on each boundary
        // edge of the inclusive rectangle [r0..r1] × [c0..c1].
        //
        // GPUI 0.2.2 has `border_dashed()` which sets `BorderStyle::Dashed`
        // globally for all four edges. We control which edges are VISIBLE by
        // setting their width to 1 (visible) or leaving them at 0 (hidden).
        // The active-cell ring applied below overrides this with `border_2()`
        // so the focus ring wins when the active cell coincides with a boundary.
        if let Some(cr) = copied {
            // Normalise so r0 ≤ r1 and c0 ≤ c1 (the selection geometry
            // already normalises, but copied_range mirrors it verbatim).
            let (rmin, rmax) = (cr.r0.min(cr.r1), cr.r0.max(cr.r1));
            let (cmin, cmax) = (cr.c0.min(cr.c1), cr.c0.max(cr.c1));

            let on_top = row_ix == rmin && col_ix >= cmin && col_ix <= cmax;
            let on_bottom = row_ix == rmax && col_ix >= cmin && col_ix <= cmax;
            let on_left = col_ix == cmin && row_ix >= rmin && row_ix <= rmax;
            let on_right = col_ix == cmax && row_ix >= rmin && row_ix <= rmax;

            if on_top || on_bottom || on_left || on_right {
                // Dashed green accent, 1-px per visible boundary edge.
                // `.border_dashed()` sets the style; per-edge width methods
                // control which edges are visible (0-width edges are invisible).
                el = el.border_color(gpui::rgb(0x22c55e)).border_dashed();
                if on_top {
                    el = el.border_t_1();
                }
                if on_bottom {
                    el = el.border_b_1();
                }
                if on_left {
                    el = el.border_l_1();
                }
                if on_right {
                    el = el.border_r_1();
                }
            }
        }

        // Active-cell focus ring: 2-px blue border anchored on the exact cell.
        // Applied after marching-ants so it wins when both apply.
        if is_active {
            el = el
                .border_2()
                .border_color(gpui::rgb(0x3b82f6))
                .bg(gpui::rgba(0x3b82f611));
        }

        match cell {
            Some(display) => {
                // Numeric / big-int cells right-align; text + NULL left-align.
                el = match display.alignment {
                    CellAlignment::Right => el.justify_end(),
                    CellAlignment::Left => el.justify_start(),
                };
                if display.is_null {
                    // NULL renders dimmed so it reads as "absent value", not the
                    // literal string the user typed.
                    el = el.text_color(gpui::rgb(0x9ca3af));
                }
                el.child(display.text)
            }
            // Page not yet cached (or out-of-range): placeholder for this cell.
            None => el.justify_start().child("—".to_string()),
        }
    }

    /// Background-fetch the page(s) covering `visible_range` so the next render
    /// paints real values for the rows the user can see (PD-018 scroll-paging).
    ///
    /// Called by the gpui-component `Table` widget whenever the visible row
    /// range changes. The fetch runs OFF the GPUI main thread (`page_for` is
    /// async DuckDB I/O); the re-render notify is posted back onto the main
    /// thread via the `MainThreadDispatcher` (never `cx.update` from the tokio
    /// task — the canonical `spawn_view_change` discipline). Delegated to the
    /// owning `WorkspaceShell` (it holds the dispatcher-friendly weak entity).
    ///
    /// Routes through `prefetch_rows_for(&self.source, …)` so the page lands in
    /// THIS delegate's OWN source's LRU. The main grid's delegate carries the
    /// same `Arc` as the shell's `data_source`, so this is behavior-preserving
    /// for the main grid; the console results pane's delegate carries the PANE's
    /// source, so its scroll-paging loads the pane's cache instead of clobbering
    /// the main grid's (P5a T9 fix). Each `GridDataSource` owns a separate LRU.
    fn visible_rows_changed(
        &mut self,
        visible_range: std::ops::Range<usize>,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if let Some(ws) = self.ws.upgrade() {
            let source = Arc::clone(&self.source);
            ws.update(cx, |ws, cx| {
                ws.prefetch_rows_for(&source, visible_range.start, visible_range.end, cx);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::data_source::GridDataSource;
    use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
    use tempfile::TempDir;

    async fn build_source(rows: usize) -> (TempDir, Arc<GridDataSource>) {
        let tmp = TempDir::new().unwrap();
        let csv = tmp.path().join("t.csv");
        let mut s = String::from("a,b\n");
        for i in 0..rows {
            s.push_str(&format!("{},x{}\n", i, i));
        }
        std::fs::write(&csv, s).unwrap();
        let engine = DuckDBEngine::new(
            tmp.path().join("scratch.duckdb"),
            MemoryBudget {
                bytes: 256 * 1024 * 1024,
            },
        )
        .unwrap();
        engine.init().await.unwrap();
        engine
            .register_file(&csv, RegisterOpts::default())
            .await
            .unwrap();
        let engine = Arc::new(engine);
        let name = engine.get_tables().await.unwrap()[0].name.clone();
        let ds = Arc::new(GridDataSource::new(engine, name).await.unwrap());
        (tmp, ds)
    }

    #[tokio::test]
    async fn delegate_columns_match_schema() {
        let (_tmp, ds) = build_source(8).await;
        let delegate =
            GridTableDelegate::new(Arc::clone(&ds), gpui::WeakEntity::new_invalid(), &[]);
        // Delegate paints VISIBLE columns only (the `__dat0_rowid` surrogate, when
        // present, is hidden) — assert against the visible count, not the raw schema.
        assert_eq!(delegate.columns.len(), ds.visible_column_count());
        // Column name carries the type badge suffix.
        let first = &delegate.columns[0];
        assert!(
            first.name.contains('('),
            "expected type badge in column name, got {:?}",
            first.name
        );
    }

    #[tokio::test]
    async fn delegate_source_ptr_eq() {
        let (_tmp, ds) = build_source(4).await;
        let delegate =
            GridTableDelegate::new(Arc::clone(&ds), gpui::WeakEntity::new_invalid(), &[]);
        assert!(delegate.source_ptr_eq(&ds));
    }
}
