# gpui-component Table API Notes (P3a.T0 spike)

This document is the authoritative reference for the `gpui-component` `Table` widget API surface used by P3a. Tasks T2 (DataGrid view scaffold), T8 (GridDataSource), and T9 (render loop) MUST defer to this file when plan snippets contradict the actual API.

- **Verified date:** 2026-05-16
- **Verifier:** P3a.T0 spike (read-only inspection of vendored source in `~/.cargo/git/checkouts/gpui-component-*/0f0ab35*/`)
- **gpui-component pinned commit:** `0f0ab35233212f8f3277028995caf0c41e13ee6c` (tag `v0.5.1`)
- **gpui version:** `=0.2.2` (crates.io)
- **Source path inspected:** `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/crates/ui/src/table/`

---

## 1. Verbatim trait signature — `TableDelegate`

Source: `crates/ui/src/table/delegate.rs`

```rust
/// A delegate trait for providing data and rendering for a table.
#[allow(unused)]
pub trait TableDelegate: Sized + 'static {
    /// Return the number of columns in the table.
    fn columns_count(&self, cx: &App) -> usize;

    /// Return the number of rows in the table.
    fn rows_count(&self, cx: &App) -> usize;

    /// Returns the table column at the given index.
    ///
    /// This only call on Table prepare or refresh.
    fn column(&self, col_ix: usize, cx: &App) -> &Column;

    /// Perform sort on the column at the given index.
    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
    }

    /// Render the table head row.
    fn render_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id("header")
    }

    /// Render the header cell at the given column index, default to the column name.
    fn render_th(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .child(self.column(col_ix, cx).name.clone())
    }

    /// Render the row at the given row and column.
    ///
    /// Not include the table head row.
    fn render_tr(
        &mut self,
        row_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id(("row", row_ix))
    }

    /// Render the context menu for the row at the given row index.
    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        menu
    }

    /// Render cell at the given row and column.
    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement;

    /// Move the column at the given `col_ix` to insert before the column at `to_ix`.
    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
    }

    /// Return an Element to show when table is empty.
    fn render_empty(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement { ... }

    /// Return true to show the loading view.
    fn loading(&self, cx: &App) -> bool {
        false
    }

    /// Return an Element to show when table is loading.
    fn render_loading(
        &mut self,
        size: Size,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement { ... }

    /// Return true to enable load more data when scrolling to the bottom.
    ///
    /// Default: true
    fn is_eof(&self, cx: &App) -> bool {
        true
    }

    /// Returns the row threshold that triggers `load_more` when scrolling to bottom.
    ///
    /// Default: 20 rows
    fn load_more_threshold(&self) -> usize {
        20
    }

    /// Load more data when the table is scrolled to the bottom.
    ///
    /// This will performed in a background task.
    ///
    /// This is always called when the table is near the bottom,
    /// so you must check if there is more data to load or lock the loading state.
    fn load_more(&mut self, window: &mut Window, cx: &mut Context<TableState<Self>>) {}

    /// Render the last empty column, default to empty.
    fn render_last_empty_col(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        h_flex().w_3().h_full().flex_shrink_0()
    }

    /// Called when the visible range of rows changed. Must be fast.
    fn visible_rows_changed(
        &mut self,
        visible_range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
    }

    /// Called when the visible range of columns changed. Must be fast.
    fn visible_columns_changed(
        &mut self,
        visible_range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
    }
}
```

**Only `render_td` has no default body — it is the sole required method.**

The remaining methods (`columns_count`, `rows_count`, `column`) have no default but are required to satisfy the vtable. In practice all three must be implemented.

---

## 2. `Column` type (verbatim from `crates/ui/src/table/column.rs`)

```rust
#[derive(Debug, Clone)]
pub struct Column {
    pub key: SharedString,
    pub name: SharedString,
    pub align: TextAlign,
    pub sort: Option<ColumnSort>,
    pub paddings: Option<Edges<Pixels>>,
    pub width: Pixels,
    pub fixed: Option<ColumnFixed>,
    pub resizable: bool,
    pub movable: bool,
    pub selectable: bool,
}

impl Column {
    pub fn new(key: impl Into<SharedString>, name: impl Into<SharedString>) -> Self { ... }
    pub fn width(mut self, width: impl Into<Pixels>) -> Self { ... }
    pub fn sortable(mut self) -> Self { ... }
    pub fn fixed_left(mut self) -> Self { ... }
    pub fn resizable(mut self, resizable: bool) -> Self { ... }
    // ... builder chain, full source in column.rs
}
```

Default width: `px(100.)`. Columns are cloned inside `prepare_col_groups` — keep them cheap.

---

## 3. `Table` element and `TableState` entity (from `crates/ui/src/table/mod.rs` + `state.rs`)

```rust
// The stateful entity. Constructed via cx.new(|cx| TableState::new(delegate, window, cx)).
pub struct TableState<D: TableDelegate> { ... }

impl<D: TableDelegate> TableState<D> {
    pub fn new(delegate: D, _: &mut Window, cx: &mut Context<Self>) -> Self;
    pub fn delegate(&self) -> &D;
    pub fn delegate_mut(&mut self) -> &mut D;
    pub fn refresh(&mut self, cx: &mut Context<Self>);   // call after column/row changes
    pub fn scroll_to_row(&mut self, row_ix: usize, cx: &mut Context<Self>);
    pub fn set_selected_row(&mut self, row_ix: usize, cx: &mut Context<Self>);
    pub fn visible_range(&self) -> &TableVisibleRange;
    // ... config flags: loop_selection, col_selectable, row_selectable, sortable, col_movable, col_resizable
}

// The render element. Wraps the state entity.
#[derive(IntoElement)]
pub struct Table<D: TableDelegate> {
    state: Entity<TableState<D>>,
    options: TableOptions,
}

impl<D: TableDelegate> Table<D> {
    pub fn new(state: &Entity<TableState<D>>) -> Self;
    pub fn stripe(mut self, stripe: bool) -> Self;
    pub fn bordered(mut self, bordered: bool) -> Self;
    pub fn scrollbar_visible(mut self, vertical: bool, horizontal: bool) -> Self;
}
```

`Table<D>` implements `RenderOnce` — use it as a one-shot element inside a parent view's `render` method. `TableState<D>` implements `Render` (via `uniform_list` internally) and must be stored as an `Entity<TableState<D>>` on the parent view.

---

## 4. Key behavioral findings

### 4.1 Trait bounds — `Sized + 'static`, not `Send + Sync`

`TableDelegate: Sized + 'static`. **There are no `Send` or `Sync` bounds.** The delegate lives on the GPUI main thread (all rendering runs there) and is accessed exclusively through `&mut self` inside GPUI's single-threaded render loop. An `Arc<DuckDBEngine>` held inside the delegate is fine — the engine itself is `Send + Sync`, but the delegate wrapper does not need to be.

### 4.2 Row fetch is synchronous

There is no async method in `TableDelegate`. The `render_td` callback is called synchronously during the GPUI render pass. `cx.spawn_in` schedules a future on the GPUI **main-thread** executor — the future itself runs on the main thread, not a background pool (verified: `gpui-0.2.2/src/app/context.rs:659` doc-comment: "The returned future will be polled on the main thread."). For blocking I/O (e.g., `engine.execute_paged().await`), the `load_more` implementation must dispatch the actual work to a background executor via `cx.background_executor().spawn(...)` (or `smol::Task` / `tokio::task::spawn_blocking` if the consumer manages its own runtime). `load_more` returns `()` synchronously after kicking off the work; results land in the cache via `cx.notify()` from the spawned task. `load_more` is only for *initiating* a data fetch — the actual cell render (`render_td`) must return data that is already in-memory at render time.

**Implication for T8 (`GridDataSource`):** The delegate must maintain an in-memory row cache (a pre-fetched page of `RecordBatch` data). Background loading via `load_more` + `visible_rows_changed` populates that cache; `render_td` reads from it synchronously. A preloader thread / async task is required.

### 4.3 Virtualization is built-in

`TableState::render` uses `gpui::uniform_list` for the vertical axis — only visible rows are rendered. For the horizontal axis, `gpui_component::virtual_list::virtual_list` (used inside each row) virtualizes columns. Both are driven by the widget internally; the consumer does not manage windowing indices beyond what `visible_rows_changed` / `visible_columns_changed` report.

The scroll handle is `UniformListScrollHandle` (vertical) + `VirtualListScrollHandle` (horizontal). Both are public fields on `TableState`, accessible if the parent view needs to programmatically scroll.

### 4.4 Cell render contract — `impl IntoElement`, column-by-column per row

`render_td(row_ix, col_ix, window, cx) -> impl IntoElement`

The call pattern is: for each visible row, for each visible column, one `render_td` call. The frame ordering is row-major — `row_ix` is the outer loop, `col_ix` is driven by the horizontal virtual list's visible range. The element returned must be cheap to construct from an in-memory cache (no blocking I/O).

### 4.5 `is_eof` / `load_more` logic — counter-intuitive polarity

`load_more_if_need` calls `load_more` when `is_eof(cx)` returns `true`. The default is `true`. The apparent semantic of `is_eof` is "no more data to load" but the widget inverts this: returning `true` **enables** load_more calls, not suppresses them. The likely intended idiom is:

- Return `false` while a background load is in-flight (suppresses re-entrancy).
- Return `true` (or the default) when the delegate is ready to accept another `load_more` call.

This is confirmed by the comment: "you must check if there is more data to load or lock the loading state."

### 4.6 Header render — `Stateful<Div>`

`render_header` returns `Stateful<Div>` (not `impl IntoElement`). The default implementation is `div().id("header")`. The widget further wraps this in a fixed-height row with border; the delegate can style or add children to the returned div but must not change its stateful-element nature.

---

## 5. Hello-world `TableDelegate` over `Vec<Vec<String>>`

This is an illustrative snippet — it lives in this doc only, not in a `.rs` file.

```rust
use std::ops::Range;
use gpui::{App, IntoElement, Window, div, prelude::*};
use gpui_component::{
    table::{Column, ColumnSort, TableDelegate, TableState},
};

struct SimpleDelegate {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
}

impl TableDelegate for SimpleDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut gpui::Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = self
            .rows
            .get(row_ix)
            .and_then(|row| row.get(col_ix))
            .cloned()
            .unwrap_or_default();
        div().size_full().child(text)
    }
}

// In a parent Render impl:
//
//   let table_state = cx.new(|cx| TableState::new(
//       SimpleDelegate {
//           columns: vec![Column::new("a", "Column A"), Column::new("b", "Column B")],
//           rows: vec![vec!["r0c0".into(), "r0c1".into()], vec!["r1c0".into(), "r1c1".into()]],
//       },
//       window,
//       cx,
//   ));
//
//   // In render():
//   Table::new(&self.table_state).stripe(true).bordered(true)
```

---

## 6. `GridDataSource` skeleton (T8 target shape)

This shows the intended shape of the `GridDataSource` that T8 will implement. It is a sketch — actual field types and error handling to be finalized in T8.

```rust
use std::{ops::Range, sync::Arc};
use gpui::{App, IntoElement, Window, div, prelude::*};
use gpui_component::table::{Column, TableDelegate, TableState};
use duckdb::arrow::record_batch::RecordBatch;

/// Page cache entry: one fetched page of Arrow data.
struct CachedPage {
    batch: RecordBatch,
    page_start_row: usize,
}

/// The delegate that GridDataSource (T8) will implement.
pub struct GridDataSource {
    /// Reference to the per-window DuckDB engine.
    engine: Arc<dat0_engine::DuckDBEngine>,
    /// The DuckDB table or view name being displayed.
    table_name: String,
    /// Schema: column definitions derived from the Arrow schema.
    columns: Vec<Column>,
    /// Total row count (fetched once on init, refreshed on reload).
    total_rows: usize,
    /// In-memory page cache: one or more recently-fetched Arrow RecordBatches.
    page_cache: Option<CachedPage>,
    /// True while a background `load_more` fetch is in-flight.
    loading: bool,
    /// True when the full dataset is known to have been consumed (all pages fetched).
    all_pages_loaded: bool,
}

impl TableDelegate for GridDataSource {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.total_rows
    }

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn loading(&self, _cx: &App) -> bool {
        self.loading
    }

    // `is_eof` — return false while loading to suppress re-entrant load_more.
    fn is_eof(&self, _cx: &App) -> bool {
        !self.loading
    }

    fn load_more(&mut self, window: &mut Window, cx: &mut gpui::Context<TableState<Self>>) {
        if self.loading || self.all_pages_loaded {
            return;
        }
        self.loading = true;

        let engine = Arc::clone(&self.engine);
        let table_name = self.table_name.clone();
        let next_offset = self
            .page_cache
            .as_ref()
            .map(|p| p.page_start_row + p.batch.num_rows())
            .unwrap_or(0);

        // Spawn a background task; update state on completion.
        // spawn_in runs on the main thread — wrap blocking work in cx.background_executor().spawn(...)
        cx.spawn_in(window, async move |view, window| {
            // engine.execute_paged(...) is async — call it here.
            // On result, view.update_in(window, |delegate, _, cx| { ... }) to store batch.
            let _ = view.update_in(window, |_state, _window, _cx| {
                // TODO T8: store batch into page_cache, set loading = false,
                // call cx.notify() to trigger re-render.
            });
        })
        .detach();
    }

    fn visible_rows_changed(
        &mut self,
        _visible_range: Range<usize>,
        _window: &mut Window,
        _cx: &mut gpui::Context<TableState<Self>>,
    ) {
        // TODO T8: use visible_range to prefetch the next page if the visible
        // window is approaching the edge of the current cached batch.
        // Must be fast — called on every scroll event.
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut gpui::Context<TableState<Self>>,
    ) -> impl IntoElement {
        // Synchronous read from the in-memory cache.
        // If the row is not in the cache, return a placeholder.
        let text = self
            .page_cache
            .as_ref()
            .and_then(|page| {
                let local_ix = row_ix.checked_sub(page.page_start_row)?;
                let col_arr = page.batch.column(col_ix);
                // Arrow array → String (use arrow::array::cast or as_string_array).
                // TODO T8: implement Arrow scalar → SharedString formatting helper.
                Some(format!("r{row_ix}c{col_ix}"))
            })
            .unwrap_or_else(|| "…".to_string());

        div().size_full().child(text)
    }
}
```

---

## 7. Open questions

### OQ-1 — `load_more` runs via `cx.spawn_in` but `render_td` is synchronous

The `load_more` mechanism dispatches a background task. There is no built-in way for the task completion to call back into `render_td`; instead the delegate must call `cx.notify()` inside the `update_in` closure to trigger a re-render of the parent `TableState`. The `render_td` then reads the newly-populated cache synchronously.

**Design consequence for T8:** `GridDataSource` needs an explicit `page_cache` field (or similar) that `load_more`'s background task populates. The cache must be accessible from `render_td` without async. This matches the planned design — no divergence.

### OQ-2 — `is_eof` polarity is counter-intuitive

The plan (§3.4 DataGrid design) describes `is_eof` as "return false to signal 'we have more rows to load.'" The actual API behavior: `load_more_if_need` calls `load_more` when `is_eof` returns `true` (the default). The guard against re-entrant loads must be implemented inside `load_more` itself (check `self.loading`), not by flipping `is_eof`.

**Recommended T8 pattern:** Always return `!self.loading` from `is_eof`. This allows `load_more` to be triggered when not already loading, and suppresses a second call while one is in-flight. Checking `self.all_pages_loaded` additionally suppresses calls once the full dataset is exhausted.

**No plan defect is filed** — the plan's §3.4 description of `is_eof` is at the design intent level and does not cite a specific polarity for the implementation. The concrete `GridDataSource` implementation in T8 owns this detail.

### OQ-3 — `visible_rows_changed` receives `Range<usize>` absolute row indices

The plan's T8 description says "the delegate gets the visible row window." Confirmed: `visible_rows_changed` passes `Range<usize>` of absolute (not relative) row indices. The delegate's preloading logic in T8 uses these absolute indices to decide whether to kick off a background fetch.

### OQ-4 — No `Send + Sync` bound on `TableDelegate`

The plan's §3.4 asks: "Does the trait require `Send + Sync`?" Answer: **no**. The delegate is fully main-thread-resident. The `Arc<DuckDBEngine>` field on `GridDataSource` is `Send + Sync` on its own (the engine's blocking I/O runs on dedicated threads). No special wiring is needed to satisfy trait bounds.

---

## 8. Source pointer (for re-verification)

At pinned commit `0f0ab35233212f8f3277028995caf0c41e13ee6c`:

- `crates/ui/src/table/delegate.rs` — `TableDelegate` trait (full source)
- `crates/ui/src/table/mod.rs` — `Table<D>` element, `TableOptions`
- `crates/ui/src/table/state.rs` — `TableState<D>`, `TableVisibleRange`, `TableEvent`, scroll handles
- `crates/ui/src/table/column.rs` — `Column`, `ColumnSort`, `ColumnFixed`, `DragColumn`, `ResizeColumn`
- `crates/ui/src/table/loading.rs` — `Loading` skeleton view (used by default `render_loading`)
