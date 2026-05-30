//! DataGrid: gpui-component Table wrapper over duckdb::arrow batches.

pub mod data_source;
pub mod renderers;
pub mod selection;

pub use data_source::GridDataSource;

use std::sync::Arc;

use duckdb::arrow::datatypes::DataType;
use gpui::{
    App, Context, IntoElement, ParentElement, SharedString, Styled, WeakEntity, Window, div,
    prelude::*,
};
use gpui_component::table::{Column, TableDelegate, TableState};

use renderers::{CellAlignment, render_cell, type_badge};

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
// T9 stub: grip / body are invisible no-ops.
//   Grip → column-resize (P4c)
//   Body → row-selection toggle (P4b)

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
    /// Left-edge resize grip.  T9 stub: no-op (P4c will fill).
    Grip,
    /// Column-name / body click area.  T9 stub: no-op (P4b will fill).
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
/// is the only cross-thread handle; pages are still fetched off-thread
/// by P3a's async paging layer (later P3b tasks will wire `load_more`).
///
/// For T4 this is a minimum-viable delegate: columns are derived from the
/// `GridDataSource` schema at construction, and `render_td` renders the
/// in-memory page-0 batch via [`renderers::render_cell`]. `load_more` /
/// `visible_rows_changed` will land in a follow-up task that wires the
/// async paged fetch back into a per-page cache — until then any row
/// outside the initial page renders as an em-dash placeholder so the
/// widget mounts cleanly.
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
    /// Build a delegate over `source`, deriving column metadata from the
    /// Arrow schema. Columns get a small default width and a type-badge
    /// suffix in their display name (e.g., `id (INT64)`).
    ///
    /// `ws` is a weak handle to the owning `WorkspaceShell` so the header
    /// sort/funnel click closures (T0 / PD-016) can dispatch into it. Tests
    /// that build a delegate without a shell pass `WeakEntity::new_invalid()`.
    pub fn new(source: Arc<GridDataSource>, ws: WeakEntity<crate::window::WorkspaceShell>) -> Self {
        let schema = source.schema.clone();
        let columns = schema
            .fields()
            .iter()
            .map(|f| {
                let badge = type_badge(f.data_type());
                let name: SharedString = format!("{} ({})", f.name(), badge).into();
                let key: SharedString = f.name().to_string().into();
                Column::new(key, name)
            })
            .collect();
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
    ///   1. **Grip** — invisible `HEADER_GRIP_PX`-wide strip for future column
    ///      resize (P4c).  T9 stub: no-op + `cursor_col_resize` hint only.
    ///   2. **Body** — column-name text, flex-grows to fill remaining space.
    ///      T9 stub: no-op click (P4b row-selection will claim this zone).
    ///   3. **Sort icon** — `HEADER_SORT_PX`-wide `⇅` button.  Live in P4a:
    ///      clicking logs a placeholder that T12 wires to Asc/Desc/None cycle.
    ///   4. **Funnel icon** — `HEADER_FUNNEL_PX`-wide `⌄` button.  Live in P4a:
    ///      clicking logs a placeholder that T10 wires to the filter popover.
    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let col_name = self.columns[col_ix].name.clone();

        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h_full()
            // ── Zone 1: Grip (T9 stub — P4c column-resize will fill) ──────────
            .child(
                div()
                    .id(("th-grip", col_ix))
                    .w(gpui::px(HEADER_GRIP_PX))
                    .h_full()
                    .cursor_col_resize(),
                // T9 stub: no click handler; P4c (column resize) claims this zone.
            )
            // ── Zone 2: Body / column name (T9 stub — P4b selection will fill) ─
            .child(
                div()
                    .id(("th-body", col_ix))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(col_name),
                // T9 stub: no click handler; P4b (row selection) claims this zone.
            )
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
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // Per §4.4: `render_td` must return synchronously from in-memory
        // data. P3a's `GridDataSource::page_for` is async; we can't await
        // here. The proper fix (background prefetch + cached page lookup)
        // is the next task in P3b — for T4 we read the Arrow schema for
        // column type and, when the cache happens to hold the row's page,
        // render the cell. Otherwise we emit a placeholder.
        //
        // Page-0 is the only page that's "guaranteed" available because
        // `GridDataSource::new` probes the schema via `LIMIT 1` — that
        // call does not populate the LRU. Hence "—" is the expected
        // render for every cell at T4. The follow-up task replaces this
        // with a synchronous cache lookup.
        let text = self
            .source
            .schema
            .fields()
            .get(col_ix)
            .map(|f| match f.data_type() {
                DataType::Int32 | DataType::Int64 | DataType::UInt64 | DataType::Float64 => "—",
                _ => "—",
            })
            .unwrap_or("—");

        // Note: the explicit numeric/right-align branch will land alongside
        // the cache lookup; we keep the visual API (`render_cell` →
        // `CellDisplay`) live below to ensure dead-code linters do not strip
        // it, but the rendered output here is the placeholder.
        let _ = (render_cell, CellAlignment::Right);

        // ElementId only accepts (&str, usize) tuples (gpui 0.2.2), not
        // 3-tuples. Encode (row_ix, col_ix) into a single index so each
        // cell still gets a unique id.
        let cell_ix = row_ix
            .saturating_mul(self.columns.len())
            .saturating_add(col_ix);
        div().id(("td", cell_ix)).size_full().child(text)
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
        let delegate = GridTableDelegate::new(Arc::clone(&ds), gpui::WeakEntity::new_invalid());
        assert_eq!(delegate.columns.len(), ds.schema.fields().len());
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
        let delegate = GridTableDelegate::new(Arc::clone(&ds), gpui::WeakEntity::new_invalid());
        assert!(delegate.source_ptr_eq(&ds));
    }
}
