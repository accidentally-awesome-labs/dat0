//! Paged Arrow batch adapter feeding gpui-component's Table widget.

use anyhow::{Context, Result};
use dat0_engine::QueryEngine;
use duckdb::arrow::array::{Array, Int64Array};
use duckdb::arrow::datatypes::SchemaRef;
use duckdb::arrow::record_batch::RecordBatch;
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;

const PAGE_ROWS: u64 = 1024;

/// Resident-page ceiling per data source.
///
/// At `PAGE_ROWS = 1024` that is 64 × 1 024 = **65 536 rows resident** per grid
/// — bounded, and small next to the memory budget. It was 16 (16 384 rows),
/// which a fast scroll thrashes straight through: a 16-page window is roughly
/// eight viewports, so a flick evicts pages the scrollbar is about to come back
/// to and every one of them costs a DuckDB round-trip. 64 pages absorbs the
/// flick without unbounding the cache.
const LRU_CAPACITY: usize = 64;

/// [`LRU_CAPACITY`] as the `NonZeroUsize` `LruCache::new` wants.
///
/// Resolved by a const `match` rather than `.expect()` so constructing a data
/// source — a path a render reaches — carries no panic at all (EN5). The `None`
/// arm is unreachable for any non-zero literal above and is pinned by
/// `lru_capacity_nonzero_matches` so the fallback can never silently lie.
const LRU_CAPACITY_NZ: NonZeroUsize = match NonZeroUsize::new(LRU_CAPACITY) {
    Some(n) => n,
    None => NonZeroUsize::MIN,
};

/// Cache key: page start row, aligned to `PAGE_ROWS`.
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct PageKey {
    pub start: u64,
}

/// Paged Arrow adapter: serves one Arrow `RecordBatch` per 1 024-row page,
/// backed by an LRU cache of up to [`LRU_CAPACITY`] pages.
pub struct GridDataSource {
    /// Held as a trait object, not `Arc<DuckDBEngine>`: this type needs exactly
    /// two methods (`execute` for the one bind-time `COUNT(*)`, `execute_page`
    /// for every page), and the seam is what lets the inline tests substitute a
    /// query-counting engine to pin the "one COUNT per source" contract EN1
    /// established.
    engine: Arc<dyn QueryEngine>,
    table_name: String,
    /// Arrow schema of the backing table. Used to construct zero-row batches
    /// for beyond-EOF pages without hitting DuckDB.
    pub schema: SchemaRef,
    pub row_count: u64,
    /// `parking_lot::Mutex`, not `std::sync`: the eight lock sites below are all
    /// on render or page-fetch paths and every one of them used to be
    /// `.expect("grid cache poisoned")`. One panic anywhere in the process
    /// poisoned the lock and bricked EVERY subsequent page fetch for the life of
    /// the window (EN5). `parking_lot` does not poison, so there is nothing to
    /// unwrap.
    cache: Mutex<LruCache<PageKey, Arc<RecordBatch>>>,
}

impl GridDataSource {
    /// Create a new `GridDataSource` for `table_name`, computing the initial
    /// row count via a `COUNT(*)` query and probing the schema via `LIMIT 1`.
    ///
    /// Exactly ONE `COUNT(*)` runs here and none afterwards. The schema probe
    /// and every [`Self::page_for`] use `execute_page`, which does not re-count
    /// (EN1) — `execute_paged` wrapped each window in `SELECT COUNT(*) FROM
    /// (<sql>) sub`, so binding a grid cost three full scans before first paint
    /// and one more per LRU miss.
    ///
    /// Generic over the CONCRETE engine type, then erased into the
    /// `Arc<dyn QueryEngine>` field. Not `Arc<dyn QueryEngine>` in the signature:
    /// every caller passes `Arc::clone(&engine)`, and `Arc::clone`'s `T` unifies
    /// with the parameter type BEFORE any unsize coercion is considered, so a
    /// `dyn` parameter would reject `&Arc<DuckDBEngine>` at all 14 call sites.
    /// The generic makes the erasure happen one step later, where coercion does
    /// apply — so the seam costs callers nothing.
    pub async fn new<E: QueryEngine + 'static>(engine: Arc<E>, table_name: String) -> Result<Self> {
        let engine: Arc<dyn QueryEngine> = engine;
        let row_count = count_rows(engine.as_ref(), &table_name)
            .await
            .context("GridDataSource::new — count_rows failed")?;

        // Probe schema by fetching one row. LIMIT 0 in DuckDB returns no
        // batches at all, so we use LIMIT 1 which always yields a batch even
        // for empty tables (the batch may have 0 rows if the table is empty,
        // but it carries schema). For empty tables we fall back to an empty
        // schema and let callers treat all pages as empty.
        let safe_name = table_name.replace('"', "\"\"");
        let schema_sql = format!("SELECT * FROM \"{}\"", safe_name);
        let probe = engine
            .execute_page(&schema_sql, 0, 1)
            .await
            .context("GridDataSource::new — schema probe failed")?;
        let schema: SchemaRef = probe
            .batches
            .first()
            .context("schema probe yielded no batch")?
            .schema();

        Ok(Self {
            engine,
            table_name,
            schema,
            row_count,
            cache: Mutex::new(LruCache::new(LRU_CAPACITY_NZ)),
        })
    }

    /// Names of the columns the grid paints, in render order — every Arrow
    /// schema field EXCEPT the hidden surrogate [`dat0_engine::ROWID_COL`].
    ///
    /// The surrogate `__dat0_rowid` is plumbed through the schema (views/tables
    /// `SELECT *` it) so [`Self::row_key`] can resolve a screen row to its
    /// `RowKey`, but it is NEVER a user-visible column. A user's renamed
    /// collision column `__dat0_rowid__src` (moved aside by `ensure_rowid` when
    /// the user's own data already had that name) stays VISIBLE — it was the
    /// user's data, and only the exact `__dat0_rowid` sentinel is hidden.
    ///
    /// When the surrogate is absent (e.g. a plain VIEW, or a table predating
    /// `ensure_rowid`), this simply has nothing to filter and returns every
    /// column — graceful degradation. (App file imports now materialize to
    /// rowid-bearing base tables; PD-017 closed.)
    pub fn visible_column_names(&self) -> Vec<String> {
        self.schema
            .fields()
            .iter()
            .map(|f| f.name())
            .filter(|name| name.as_str() != dat0_engine::ROWID_COL)
            .cloned()
            .collect()
    }

    /// Map a VISIBLE-column index (as used by the grid delegate's rendered
    /// columns and the header click handlers) to its index in the underlying
    /// Arrow schema, which still contains the hidden surrogate. Returns `None`
    /// if `visible_ix` is out of range.
    ///
    /// This is the single source of truth for the visible→schema mapping so
    /// that a click on visible column N, the rendered cell at column N, and
    /// [`Self::column_name(N)`] all refer to the SAME user-visible column even
    /// after `__dat0_rowid` is hidden.
    pub fn schema_index_for_visible(&self, visible_ix: usize) -> Option<usize> {
        self.schema
            .fields()
            .iter()
            .enumerate()
            .filter(|(_, f)| f.name().as_str() != dat0_engine::ROWID_COL)
            .map(|(schema_ix, _)| schema_ix)
            .nth(visible_ix)
    }

    /// Map a VISIBLE column's SOURCE NAME to its index in the underlying Arrow
    /// schema (which still contains the hidden surrogate). Returns `None` if no
    /// visible field has that exact name.
    ///
    /// Mirror of [`Self::schema_index_for_visible`], keyed on the column's
    /// stable source identity rather than its screen position. The grid's
    /// `ColumnView` (P4c) can reorder/delete columns so that a screen index no
    /// longer matches the schema index; the mutating paths resolve `screen →
    /// source` through the `ColumnView` and then `source → schema index` through
    /// this method, so a cell read/write always hits the right Arrow column even
    /// after a display-only reorder. The hidden `__dat0_rowid` surrogate is
    /// excluded from the match (it is never a `ColumnView` source).
    pub fn schema_index_for_source(&self, source: &str) -> Option<usize> {
        if source == dat0_engine::ROWID_COL {
            return None;
        }
        self.schema
            .fields()
            .iter()
            .position(|f| f.name().as_str() == source)
    }

    /// Number of VISIBLE columns the grid paints (schema columns minus the
    /// hidden surrogate). Convenience for the delegate's `columns_count`.
    pub fn visible_column_count(&self) -> usize {
        self.schema
            .fields()
            .iter()
            .filter(|f| f.name().as_str() != dat0_engine::ROWID_COL)
            .count()
    }

    /// Name of the VISIBLE column at `ix`, or `None` if `ix` is out of range.
    /// Used by the grid header click handlers (T0 / PD-016) to resolve a clicked
    /// column index to the bare column name that the `ViewModel` /
    /// filter-popover work against.
    ///
    /// Indexes over VISIBLE columns (the hidden `__dat0_rowid` is skipped) so
    /// the `col_ix` the delegate hands the click closure is consistent with the
    /// rendered/visible column it sits under.
    pub fn column_name(&self, ix: usize) -> Option<String> {
        let schema_ix = self.schema_index_for_visible(ix)?;
        self.schema
            .fields()
            .get(schema_ix)
            .map(|f| f.name().to_string())
    }

    /// Surrogate `__dat0_rowid` of the row at `screen_row`, or `None` when the
    /// row's page is not cached or the surrogate column is absent (PD-017 —
    /// graceful degradation, no panic).
    ///
    /// Reads the cached page batch for `screen_row` (a synchronous LRU lookup —
    /// it never triggers a DuckDB fetch, mirroring the synchronous
    /// `render_td` contract). The surrogate lives in the Arrow schema because
    /// views/tables `SELECT *` it. Later edit/delete/copy work maps a grid
    /// coordinate → `RowKey::Surrogate(row_key(screen_row))`.
    pub fn row_key(&self, screen_row: usize) -> Option<i64> {
        let rowid_ix = self
            .schema
            .fields()
            .iter()
            .position(|f| f.name().as_str() == dat0_engine::ROWID_COL)?;

        let screen_row = u64::try_from(screen_row).ok()?;
        let key = PageKey {
            start: (screen_row / PAGE_ROWS) * PAGE_ROWS,
        };
        let offset = usize::try_from(screen_row - key.start).ok()?;

        let batch = {
            let mut cache = self.cache.lock();
            Arc::clone(cache.get(&key)?)
        };

        if offset >= batch.num_rows() {
            return None;
        }
        let arr = batch
            .column(rowid_ix)
            .as_any()
            .downcast_ref::<Int64Array>()?;
        if arr.is_null(offset) {
            return None;
        }
        Some(arr.value(offset))
    }

    /// Coarse [`crate::view::filter_popover::ColumnType`] of the VISIBLE column
    /// at `ix`, derived from its Arrow `DataType`. Used by the funnel-zone click
    /// handler to construct the filter popover with the right operator surface.
    ///
    /// Indexes over VISIBLE columns (consistent with [`Self::column_name`]).
    /// Unknown / unhandled Arrow types fall back to `String` (the safe,
    /// non-destructive default — Contains/Regex rather than numeric ops).
    pub fn column_type(&self, ix: usize) -> Option<crate::view::filter_popover::ColumnType> {
        let schema_ix = self.schema_index_for_visible(ix)?;
        self.column_type_at_schema_ix(schema_ix)
    }

    /// Coarse [`crate::view::filter_popover::ColumnType`] of the visible column
    /// whose SOURCE NAME is `source`, or `None` when no such column exists.
    ///
    /// Source-keyed twin of [`Self::column_type`] (P4c). After a `ColumnView`
    /// reorder the screen-col order no longer matches the schema order, so the
    /// edit/clipboard paths resolve `screen → source` through the `ColumnView`
    /// and call this rather than the index-based [`Self::column_type`].
    pub fn column_type_for_source(
        &self,
        source: &str,
    ) -> Option<crate::view::filter_popover::ColumnType> {
        let schema_ix = self.schema_index_for_source(source)?;
        // Reuse the index-based mapping via the schema position. We already
        // hold the schema index, so we read the field directly here to avoid a
        // second visible→schema translation.
        self.column_type_at_schema_ix(schema_ix)
    }

    /// Real Arrow [`DataType`] of the visible column whose SOURCE NAME is
    /// `source`, or `None` when no such column exists (P4c — source-keyed twin
    /// of [`Self::column_arrow_type`], used by paste/fill coercion so the
    /// precise int/float type survives a display-only reorder).
    pub fn column_arrow_type_for_source(
        &self,
        source: &str,
    ) -> Option<duckdb::arrow::datatypes::DataType> {
        let schema_ix = self.schema_index_for_source(source)?;
        self.schema
            .fields()
            .get(schema_ix)
            .map(|f| f.data_type().clone())
    }

    /// Synchronously read the rendered DISPLAY string of the cell at
    /// (`screen_row`, source column `source`) for copy/fill (P4c), or `None`
    /// when the row's page is not cached or no column has that source name.
    ///
    /// Source-keyed twin of [`Self::cell_display`]: identical semantics (NULL →
    /// empty string, synchronous LRU-only read) but addresses the column by its
    /// `ColumnView` source identity rather than its screen index, so a reorder
    /// can't make `fill_down` read the wrong column.
    pub fn cell_display_for_source(&self, screen_row: usize, source: &str) -> Option<String> {
        let schema_ix = self.schema_index_for_source(source)?;

        let screen_row_u64 = u64::try_from(screen_row).ok()?;
        let key = PageKey {
            start: (screen_row_u64 / PAGE_ROWS) * PAGE_ROWS,
        };
        let offset = usize::try_from(screen_row_u64 - key.start).ok()?;

        let batch = {
            let mut cache = self.cache.lock();
            Arc::clone(cache.get(&key)?)
        };

        if offset >= batch.num_rows() {
            return None;
        }

        let display = crate::grid::renderers::render_cell(&batch, schema_ix, offset);
        if display.is_null {
            Some(String::new())
        } else {
            Some(display.text)
        }
    }

    /// Coarse [`crate::view::filter_popover::ColumnType`] of the Arrow field at
    /// raw `schema_ix`. Shared body for [`Self::column_type`] (visible-index
    /// keyed) and [`Self::column_type_for_source`] (source keyed).
    fn column_type_at_schema_ix(
        &self,
        schema_ix: usize,
    ) -> Option<crate::view::filter_popover::ColumnType> {
        use crate::view::filter_popover::ColumnType;
        use duckdb::arrow::datatypes::DataType;
        self.schema
            .fields()
            .get(schema_ix)
            .map(|f| match f.data_type() {
                DataType::Int8
                | DataType::Int16
                | DataType::Int32
                | DataType::Int64
                | DataType::UInt8
                | DataType::UInt16
                | DataType::UInt32
                | DataType::UInt64
                | DataType::Float16
                | DataType::Float32
                | DataType::Float64
                | DataType::Decimal128(_, _)
                | DataType::Decimal256(_, _) => ColumnType::Numeric,
                DataType::Boolean => ColumnType::Bool,
                DataType::Date32 | DataType::Date64 => ColumnType::Date,
                DataType::Timestamp(_, _) => ColumnType::Timestamp,
                _ => ColumnType::String,
            })
    }

    /// Real Arrow [`DataType`] of the VISIBLE column at `ix`, or `None` when
    /// `ix` is out of range (T7 — paste coercion needs the precise type, not
    /// the coarse [`crate::view::filter_popover::ColumnType`], so int/float
    /// fidelity survives: `42` into an Int column → `Scalar::Int`, into a Float
    /// column → `Scalar::Float`).
    ///
    /// Indexes over VISIBLE columns (consistent with [`Self::column_name`]).
    pub fn column_arrow_type(&self, ix: usize) -> Option<duckdb::arrow::datatypes::DataType> {
        let schema_ix = self.schema_index_for_visible(ix)?;
        self.schema
            .fields()
            .get(schema_ix)
            .map(|f| f.data_type().clone())
    }

    /// Synchronously read the rendered DISPLAY string of the cell at
    /// (`screen_row`, `visible_col`) for copy (T7), or `None` when the row's
    /// page is not cached or the indices are out of range (graceful — never
    /// blocks the render loop / never triggers a DuckDB fetch).
    ///
    /// Mirrors the synchronous LRU lookup [`Self::row_key`] uses: only reads a
    /// page already promoted into the cache by the paged render path. NULL cells
    /// return an empty string (the spreadsheet convention — Excel / Sheets paste
    /// an empty cell, not the literal text "NULL"). Non-NULL values use the same
    /// [`crate::grid::renderers::render_cell`] display string the grid paints, so
    /// a copy reflects exactly what the user sees.
    ///
    /// `visible_col` indexes over VISIBLE columns (the hidden `__dat0_rowid`
    /// surrogate is skipped) — consistent with [`Self::column_name`].
    pub fn cell_display(&self, screen_row: usize, visible_col: usize) -> Option<String> {
        let schema_ix = self.schema_index_for_visible(visible_col)?;

        let screen_row_u64 = u64::try_from(screen_row).ok()?;
        let key = PageKey {
            start: (screen_row_u64 / PAGE_ROWS) * PAGE_ROWS,
        };
        let offset = usize::try_from(screen_row_u64 - key.start).ok()?;

        let batch = {
            let mut cache = self.cache.lock();
            Arc::clone(cache.get(&key)?)
        };

        if offset >= batch.num_rows() {
            return None;
        }

        let display = crate::grid::renderers::render_cell(&batch, schema_ix, offset);
        if display.is_null {
            // Spreadsheet round-trip convention: a NULL cell copies as empty.
            Some(String::new())
        } else {
            Some(display.text)
        }
    }

    /// Synchronously read the full [`crate::grid::renderers::CellDisplay`] of the
    /// cell at (`screen_row`, `visible_col`) for the paged render path (PD-018),
    /// or `None` when the row's page is not cached or the indices are out of
    /// range (graceful — never blocks the render loop / never triggers a DuckDB
    /// fetch).
    ///
    /// Unlike [`Self::cell_display`] (which returns the bare copy string and maps
    /// NULL → empty for the spreadsheet round-trip), this returns the structured
    /// `CellDisplay` so the grid delegate can apply numeric right-alignment and
    /// NULL styling. The two share the same underlying [`crate::grid::renderers::render_cell`]
    /// so an on-screen cell and a copied cell reflect the same value.
    ///
    /// `visible_col` indexes over VISIBLE columns (the hidden `__dat0_rowid`
    /// surrogate is skipped) — consistent with [`Self::column_name`].
    ///
    /// Retained alongside the source-keyed [`Self::cell_render_for_source`] for
    /// callers that genuinely hold a visible index (e.g. the `edit_lifecycle`
    /// cache-contract test). The grid's paint path (`render_td`) addresses by
    /// SOURCE, so a `ColumnView` reorder/delete can't paint the wrong column.
    pub fn cell_render(
        &self,
        screen_row: usize,
        visible_col: usize,
    ) -> Option<crate::grid::renderers::CellDisplay> {
        let schema_ix = self.schema_index_for_visible(visible_col)?;
        self.cell_render_at_schema_ix(screen_row, schema_ix)
    }

    /// Synchronously read the full [`crate::grid::renderers::CellDisplay`] of the
    /// cell at (`screen_row`, source column `source`) for the paged render path,
    /// or `None` when the row's page is not cached or no visible column has that
    /// source name.
    ///
    /// Source-keyed twin of [`Self::cell_render`] (P4c): identical page/Arrow
    /// render logic (shared via [`Self::cell_render_at_schema_ix`]) but resolves
    /// the column by its `ColumnView` SOURCE identity rather than its screen
    /// index. The grid's `render_td` calls this so that after a display-only
    /// reorder/delete the body cell painted under a header reflects THAT header's
    /// column, not whatever schema column happens to sit at the same ordinal.
    /// The hidden `__dat0_rowid` surrogate is never a `ColumnView` source and so
    /// is excluded by [`Self::schema_index_for_source`].
    pub fn cell_render_for_source(
        &self,
        screen_row: usize,
        source: &str,
    ) -> Option<crate::grid::renderers::CellDisplay> {
        let schema_ix = self.schema_index_for_source(source)?;
        self.cell_render_at_schema_ix(screen_row, schema_ix)
    }

    /// Synchronously read the [`crate::grid::renderers::CellDisplay`] of the cell
    /// at (`screen_row`, raw Arrow `schema_ix`). Shared body for the
    /// visible-index-keyed [`Self::cell_render`] and the source-keyed
    /// [`Self::cell_render_for_source`] — mirrors how
    /// [`Self::column_type_at_schema_ix`] is shared. Returns `None` when the
    /// row's page is not resident in the LRU (graceful — never blocks the render
    /// loop / never triggers a DuckDB fetch) or the row is past the page's rows.
    fn cell_render_at_schema_ix(
        &self,
        screen_row: usize,
        schema_ix: usize,
    ) -> Option<crate::grid::renderers::CellDisplay> {
        let screen_row_u64 = u64::try_from(screen_row).ok()?;
        let key = PageKey {
            start: (screen_row_u64 / PAGE_ROWS) * PAGE_ROWS,
        };
        let offset = usize::try_from(screen_row_u64 - key.start).ok()?;

        let batch = {
            let mut cache = self.cache.lock();
            Arc::clone(cache.get(&key)?)
        };

        if offset >= batch.num_rows() {
            return None;
        }

        Some(crate::grid::renderers::render_cell(
            &batch, schema_ix, offset,
        ))
    }

    /// Returns `true` when the backing table has zero rows. Used by
    /// [`crate::window::WorkspaceShell::render`] (P3b T7) to fall back to
    /// the empty-state hero when a source is technically mounted but has
    /// no data (e.g., the user opened a freshly-created empty table).
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    /// Returns `true` when the LRU cache already contains the pages covering
    /// both `start_row` and `last_row` (the first and last visible rows of the
    /// current viewport), meaning the prefetch task can be skipped entirely.
    ///
    /// Uses [`LruCache::contains`], which checks residency WITHOUT bumping LRU
    /// eviction order — a pure non-mutating probe.  When both boundary pages
    /// are the same page (viewport fits inside a single 1 024-row page), only
    /// one lookup is needed.
    ///
    /// Called by [`crate::window::WorkspaceShell::prefetch_visible_rows`] as a
    /// cheap guard: if this returns `true`, the tokio spawn and subsequent
    /// `cx.notify()` are skipped, eliminating the gratuitous task-storm on fast
    /// scroll over already-loaded data.
    pub fn pages_resident(&self, start_row: usize, last_row: usize) -> bool {
        let start_key = PageKey {
            start: (start_row as u64 / PAGE_ROWS) * PAGE_ROWS,
        };
        let last_key = PageKey {
            start: (last_row as u64 / PAGE_ROWS) * PAGE_ROWS,
        };
        let cache = self.cache.lock();
        cache.contains(&start_key) && cache.contains(&last_key)
    }

    /// `(pages resident, LRU capacity)` for MX1's perf HUD.
    ///
    /// [`pages_resident`](Self::pages_resident) answers a yes/no question about
    /// one viewport; the HUD needs the occupancy of the whole cache, which is
    /// the number that shows a fast scroll thrashing against the ceiling.
    /// Non-mutating: `len` does not touch LRU order.
    pub fn cache_occupancy(&self) -> (usize, usize) {
        let cache = self.cache.lock();
        (cache.len(), LRU_CAPACITY)
    }

    /// Return (or look up cached) the `RecordBatch` covering `row`.
    /// The batch is page-aligned to `PAGE_ROWS` boundaries.
    ///
    /// Cache hit: `O(1)` — returns `Arc::clone` of the cached batch.
    /// Cache miss: issues `SELECT * FROM "table"` and lets `execute_page`
    ///   apply `LIMIT PAGE_ROWS OFFSET start` via the engine wrapper.
    ///
    /// Past-EOF rows (key.start >= row_count) return a cached empty batch
    /// immediately, avoiding redundant DuckDB round-trips from the 60 fps
    /// render loop.
    pub async fn page_for(&self, row: u64) -> Result<Arc<RecordBatch>> {
        let key = PageKey {
            start: (row / PAGE_ROWS) * PAGE_ROWS,
        };

        // --- cache lookup ---
        if let Some(batch) = self.cache.lock().get(&key) {
            tracing::trace!(page_start = key.start, "grid page cache hit");
            return Ok(Arc::clone(batch));
        }

        // --- beyond-EOF: return empty batch without hitting DuckDB ---
        if key.start >= self.row_count {
            tracing::trace!(
                page_start = key.start,
                row_count = self.row_count,
                "grid page beyond EOF — returning empty batch"
            );
            let empty = Arc::new(RecordBatch::new_empty(Arc::clone(&self.schema)));
            self.cache.lock().put(key, Arc::clone(&empty));
            return Ok(empty);
        }

        // --- cache miss: fetch from DuckDB ---
        tracing::debug!(page_start = key.start, "grid page cache miss — fetching");

        // SQL-identifier safety: escape embedded double quotes by doubling them.
        // DuckDB does not support parameterized identifiers, so this is the
        // canonical injection safeguard.
        //
        // Do NOT embed LIMIT/OFFSET in this SQL — execute_page wraps it as:
        //   SELECT * FROM (<sql>) sub LIMIT <limit> OFFSET <offset>
        // Adding them here would produce double pagination (offset applied
        // twice), returning empty results for every page beyond page 0.
        let safe_name = self.table_name.replace('"', "\"\"");
        let sql = format!("SELECT * FROM \"{}\"", safe_name);

        // `execute_page`, NOT `execute_paged`: the row count was taken once in
        // `new` and has not changed, so re-counting per page would make every
        // scroll page an O(N) scan (EN1).
        let result = self
            .engine
            .execute_page(&sql, key.start, PAGE_ROWS)
            .await
            .context("execute_page for grid page")?;

        let batch = result
            .batches
            .into_iter()
            .next()
            .context("empty result for grid page")?;

        let arc = Arc::new(batch);
        self.cache.lock().put(key, Arc::clone(&arc));
        Ok(arc)
    }
}

/// Issue `SELECT COUNT(*)::BIGINT FROM "table"` and return the count.
///
/// Uses `engine.execute()` which does not wrap the query, avoiding the
/// double-subquery overhead of `execute_paged`. `COUNT(*)` returns exactly
/// one row with one column.
///
/// This is the ONLY `COUNT(*)` a data source ever issues — `count_rows_issued`
/// in the tests below pins that.
async fn count_rows(engine: &dyn QueryEngine, table_name: &str) -> Result<u64> {
    let safe_name = table_name.replace('"', "\"\"");
    let sql = format!("SELECT COUNT(*)::BIGINT AS c FROM \"{}\"", safe_name);

    // Use execute() — no subquery wrapping, no LIMIT/OFFSET overhead.
    // COUNT(*) returns exactly one row; .batches[0].column(0)[0] is the count.
    let result = engine
        .execute(&sql)
        .await
        .context("count(*) for row_count")?;

    let batch = result
        .batches
        .into_iter()
        .next()
        .context("empty count(*) result")?;

    let col = batch.column(0);
    let arr = col
        .as_any()
        .downcast_ref::<Int64Array>()
        .context("COUNT(*) column not Int64")?;

    Ok(arr.value(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{DuckDBEngine, MemoryBudget, RegisterOpts};
    use tempfile::TempDir;

    #[test]
    fn lru_capacity_nonzero_matches() {
        // The const `match` in `LRU_CAPACITY_NZ` has an unreachable `None` arm.
        // If someone ever sets `LRU_CAPACITY = 0`, the cache would silently
        // become one page deep instead of failing — this is the tripwire.
        assert_eq!(LRU_CAPACITY_NZ.get(), LRU_CAPACITY);
    }

    /// Build an in-memory DuckDB engine with a single CSV table of `rows` rows.
    async fn build_engine_with_csv(tmp: &TempDir, rows: usize) -> Arc<DuckDBEngine> {
        let csv = tmp.path().join("t.csv");
        let mut s = String::from("a,b\n");
        for i in 0..rows {
            s.push_str(&format!("{},x{}\n", i, i));
        }
        std::fs::write(&csv, s).unwrap();

        let engine = DuckDBEngine::new(
            tmp.path().join("scratch.duckdb"),
            MemoryBudget {
                bytes: 128 * 1024 * 1024,
            },
        )
        .unwrap();
        engine.init().await.unwrap();
        engine
            .register_file(&csv, RegisterOpts::default())
            .await
            .unwrap();
        Arc::new(engine)
    }

    #[tokio::test]
    async fn row_count_matches_csv() {
        let tmp = TempDir::new().unwrap();
        let engine = build_engine_with_csv(&tmp, 100).await;
        // Drift fix: engine.get_tables().await, not engine.catalog().get_tables().await
        let tables = engine.get_tables().await.unwrap();
        let name = tables[0].name.clone();
        let ds = GridDataSource::new(engine, name).await.unwrap();
        assert_eq!(ds.row_count, 100);
    }

    #[tokio::test]
    async fn page_for_caches_on_second_call() {
        let tmp = TempDir::new().unwrap();
        let engine = build_engine_with_csv(&tmp, 4096).await;
        // Drift fix: engine.get_tables().await, not engine.catalog().get_tables().await
        let tables = engine.get_tables().await.unwrap();
        let name = tables[0].name.clone();
        let ds = GridDataSource::new(engine, name).await.unwrap();
        let a = ds.page_for(500).await.unwrap();
        let b = ds.page_for(500).await.unwrap();
        assert!(Arc::ptr_eq(&a, &b), "second call must hit cache");
    }

    #[tokio::test]
    async fn page_for_different_pages_returns_distinct_arcs() {
        let tmp = TempDir::new().unwrap();
        let engine = build_engine_with_csv(&tmp, 4096).await;
        let name = engine.get_tables().await.unwrap()[0].name.clone();
        let ds = GridDataSource::new(engine, name).await.unwrap();
        let p0 = ds.page_for(0).await.unwrap(); // page key.start = 0
        let p1 = ds.page_for(1500).await.unwrap(); // page key.start = 1024
        let p2 = ds.page_for(2500).await.unwrap(); // page key.start = 2048
        assert!(!Arc::ptr_eq(&p0, &p1));
        assert!(!Arc::ptr_eq(&p1, &p2));
        // Confirm row count per page is correct.
        assert_eq!(p0.num_rows(), 1024);
        assert_eq!(p1.num_rows(), 1024);
        // Last page: 4096 is an exact multiple of 1024, so 1024 rows on page 2.
        assert_eq!(p2.num_rows(), 1024);
    }

    #[tokio::test]
    async fn page_for_beyond_row_count_returns_empty_batch() {
        let tmp = TempDir::new().unwrap();
        let engine = build_engine_with_csv(&tmp, 100).await;
        let name = engine.get_tables().await.unwrap()[0].name.clone();
        let ds = GridDataSource::new(engine, name).await.unwrap();
        // Row 5000 is far past row_count=100.
        let batch = ds.page_for(5000).await.unwrap();
        assert_eq!(
            batch.num_rows(),
            0,
            "beyond-EOF page must be empty, not error"
        );
    }

    #[tokio::test]
    async fn page_for_first_row_returns_real_data() {
        let tmp = TempDir::new().unwrap();
        let engine = build_engine_with_csv(&tmp, 4096).await;
        let name = engine.get_tables().await.unwrap()[0].name.clone();
        let ds = GridDataSource::new(engine, name).await.unwrap();
        let p1 = ds.page_for(1024).await.unwrap(); // First row of page 1
        assert_eq!(p1.num_rows(), 1024);
        // Column 0 first value should be 1024 (the CSV is 0..rows).
        let col = p1.column(0);
        let arr = col
            .as_any()
            .downcast_ref::<duckdb::arrow::array::Int64Array>()
            .expect("int64");
        assert_eq!(arr.value(0), 1024, "page 1 must start at row index 1024");
    }

    /// P5a T9 regression: each `GridDataSource` owns a SEPARATE LRU. Prefetching
    /// a page into source A must NOT populate source B's cache.
    ///
    /// This is the invariant the T9 pane-scroll bug violated: the pane delegate's
    /// `visible_rows_changed` was hardcoded to prefetch the MAIN grid's source,
    /// so pages the user scrolled to in the PANE landed in the wrong cache and
    /// the pane's `render_td` never found them. The fix routes each delegate's
    /// prefetch through `prefetch_rows_for(&self.source, …)`; this test pins the
    /// underlying per-source cache isolation that makes that routing correct.
    #[tokio::test]
    async fn prefetch_one_source_leaves_another_untouched() {
        let tmp = TempDir::new().unwrap();
        // One engine, two independent sources over the same table — exactly the
        // main-grid-vs-pane shape (two `GridDataSource`s, each its own LRU).
        let engine = build_engine_with_csv(&tmp, 4096).await;
        let name = engine.get_tables().await.unwrap()[0].name.clone();
        let source_a = GridDataSource::new(Arc::clone(&engine), name.clone())
            .await
            .unwrap();
        let source_b = GridDataSource::new(engine, name).await.unwrap();

        // Sanity: neither source has page 1 (rows 1024..2047) resident yet.
        assert!(!source_a.pages_resident(1024, 2047));
        assert!(!source_b.pages_resident(1024, 2047));

        // Scroll-page source A to a deep row (page key 1024).
        source_a.page_for(1500).await.unwrap();

        // A now caches that page; B's cache MUST be unaffected.
        assert!(
            source_a.pages_resident(1024, 2047),
            "fetched page must be resident in the source it was fetched into"
        );
        assert!(
            !source_b.pages_resident(1024, 2047),
            "prefetching source A must NOT populate source B's independent LRU"
        );
    }

    // ---------------------------------------------------------------------
    // EN1: exactly one COUNT(*) per data source
    // ---------------------------------------------------------------------

    /// A [`QueryEngine`] that forwards to a real engine while tallying which
    /// SQL it was asked to run.
    ///
    /// The point of the fake is the tally, not fake data: every query goes to a
    /// live DuckDB so the assertions below run against the real paging
    /// behaviour. Only the three methods `GridDataSource` actually calls are
    /// implemented; the rest are unreachable through this type and say so
    /// loudly rather than returning a plausible-looking lie.
    struct CountingEngine {
        inner: Arc<DuckDBEngine>,
        counts: Mutex<QueryTally>,
    }

    #[derive(Default, Debug, PartialEq, Eq)]
    struct QueryTally {
        /// Statements containing `COUNT(*)` — the O(N) scans EN1 removed.
        counts: usize,
        /// `execute_page` calls (schema probe + one per LRU miss).
        pages: usize,
        /// `execute_paged` calls. MUST stay 0: every one would re-count.
        paged: usize,
    }

    impl CountingEngine {
        fn new(inner: Arc<DuckDBEngine>) -> Self {
            Self {
                inner,
                counts: Mutex::new(QueryTally::default()),
            }
        }

        fn tally(&self) -> QueryTally {
            let t = self.counts.lock();
            QueryTally {
                counts: t.counts,
                pages: t.pages,
                paged: t.paged,
            }
        }
    }

    /// Every method that is not on `GridDataSource`'s path. Reaching one means
    /// the data source grew a dependency this test is no longer measuring.
    macro_rules! unreached {
        ($name:literal) => {
            unimplemented!(concat!(
                "CountingEngine serves only execute / execute_page / execute_paged; ",
                $name,
                " is off GridDataSource's path"
            ))
        };
    }

    #[async_trait::async_trait]
    impl QueryEngine for CountingEngine {
        async fn execute(&self, sql: &str) -> dat0_engine::Result<dat0_engine::QueryResult> {
            if sql.contains("COUNT(*)") {
                self.counts.lock().counts += 1;
            }
            self.inner.execute(sql).await
        }

        async fn execute_paged(
            &self,
            sql: &str,
            offset: u64,
            limit: u64,
        ) -> dat0_engine::Result<dat0_engine::PagedQueryResult> {
            self.counts.lock().paged += 1;
            self.inner.execute_paged(sql, offset, limit).await
        }

        async fn execute_page(
            &self,
            sql: &str,
            offset: u64,
            limit: u64,
        ) -> dat0_engine::Result<dat0_engine::PagedQueryResult> {
            self.counts.lock().pages += 1;
            self.inner.execute_page(sql, offset, limit).await
        }

        async fn init(&self) -> dat0_engine::Result<()> {
            unreached!("init")
        }
        async fn close(&self) -> dat0_engine::Result<()> {
            unreached!("close")
        }
        fn status(&self) -> dat0_engine::EngineStatus {
            unreached!("status")
        }
        async fn register_file(
            &self,
            _: &std::path::Path,
            _: RegisterOpts,
        ) -> dat0_engine::Result<dat0_engine::TableInfo> {
            unreached!("register_file")
        }
        async fn register_file_as_table(
            &self,
            _: &std::path::Path,
            _: RegisterOpts,
        ) -> dat0_engine::Result<dat0_engine::TableInfo> {
            unreached!("register_file_as_table")
        }
        async fn create_table(
            &self,
            _: &str,
            _: &str,
            _: dat0_engine::DerivedOrigin,
        ) -> dat0_engine::Result<dat0_engine::TableInfo> {
            unreached!("create_table")
        }
        async fn drop_table(&self, _: &str, _: Option<&str>) -> dat0_engine::Result<()> {
            unreached!("drop_table")
        }
        async fn rename_table(&self, _: &str, _: &str, _: Option<&str>) -> dat0_engine::Result<()> {
            unreached!("rename_table")
        }
        async fn create_or_replace_view(&self, _: &str, _: &str) -> dat0_engine::Result<()> {
            unreached!("create_or_replace_view")
        }
        async fn drop_view(&self, _: &str) -> dat0_engine::Result<()> {
            unreached!("drop_view")
        }
        async fn execute_streaming(
            &self,
            _: &str,
        ) -> dat0_engine::Result<dat0_engine::ArrowRecordBatchStream> {
            unreached!("execute_streaming")
        }
        async fn describe_table(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> dat0_engine::Result<Vec<dat0_engine::ColumnInfo>> {
            unreached!("describe_table")
        }
        async fn get_tables(&self) -> dat0_engine::Result<Vec<dat0_engine::TableInfo>> {
            unreached!("get_tables")
        }
        async fn referenced_tables(&self, _: &str) -> dat0_engine::Result<Vec<String>> {
            unreached!("referenced_tables")
        }
        async fn profile_table(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> dat0_engine::Result<dat0_engine::profile::TableProfile> {
            unreached!("profile_table")
        }
        async fn profile_query(
            &self,
            _: &str,
        ) -> dat0_engine::Result<dat0_engine::profile::TableProfile> {
            unreached!("profile_query")
        }
        async fn column_topn(
            &self,
            _: &str,
            _: &str,
            _: u64,
        ) -> dat0_engine::Result<Vec<(String, u64)>> {
            unreached!("column_topn")
        }
        async fn column_length_stats(
            &self,
            _: &str,
            _: &str,
        ) -> dat0_engine::Result<dat0_engine::profile::LengthStats> {
            unreached!("column_length_stats")
        }
        async fn export_query_to_path(
            &self,
            _: &str,
            _: dat0_engine::ExportFormat,
            _: &std::path::Path,
        ) -> dat0_engine::Result<()> {
            unreached!("export_query_to_path")
        }
        async fn attach(
            &self,
            _: &str,
            _: &str,
            _: dat0_engine::AttachOpts,
        ) -> dat0_engine::Result<()> {
            unreached!("attach")
        }
        async fn detach(&self, _: &str) -> dat0_engine::Result<()> {
            unreached!("detach")
        }
        async fn ensure_rowid(&self, _: &str) -> dat0_engine::Result<()> {
            unreached!("ensure_rowid")
        }
    }

    /// EN1's contract: binding a source plus fetching three distinct pages
    /// issues EXACTLY ONE `COUNT(*)`.
    ///
    /// Before EN1 this was four — `count_rows` (1), the schema probe via
    /// `execute_paged` (2), then one per page because `run_paged` wrapped every
    /// window in `SELECT COUNT(*) FROM (<sql>) sub` (3, 4, 5). On a 1 GB CSV
    /// each of those is a full scan, so scrolling was O(N) per page.
    #[tokio::test]
    async fn one_count_star_per_source_and_none_per_page() {
        let tmp = TempDir::new().unwrap();
        let real = build_engine_with_csv(&tmp, 4096).await;
        let name = real.get_tables().await.unwrap()[0].name.clone();

        let counting = Arc::new(CountingEngine::new(real));
        let ds = GridDataSource::new(Arc::clone(&counting), name)
            .await
            .unwrap();
        assert_eq!(ds.row_count, 4096);

        // Three distinct pages → three LRU misses → three fetches.
        ds.page_for(0).await.unwrap();
        ds.page_for(1500).await.unwrap();
        ds.page_for(2500).await.unwrap();

        assert_eq!(
            counting.tally(),
            QueryTally {
                counts: 1,
                // 1 schema probe + 3 page fetches.
                pages: 4,
                paged: 0,
            }
        );
    }
}
