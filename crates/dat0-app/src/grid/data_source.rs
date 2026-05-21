//! Paged Arrow batch adapter feeding gpui-component's Table widget.

use anyhow::{Context, Result};
use dat0_engine::{DuckDBEngine, QueryEngine};
use duckdb::arrow::array::Int64Array;
use duckdb::arrow::datatypes::SchemaRef;
use duckdb::arrow::record_batch::RecordBatch;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::Mutex;

const PAGE_ROWS: u64 = 1024;
const LRU_CAPACITY: usize = 16;

/// Cache key: page start row, aligned to `PAGE_ROWS`.
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct PageKey {
    pub start: u64,
}

/// Paged Arrow adapter: serves one Arrow `RecordBatch` per 1 024-row page,
/// backed by an LRU cache of up to 16 pages.
pub struct GridDataSource {
    engine: Arc<DuckDBEngine>,
    table_name: String,
    /// Arrow schema of the backing table. Used to construct zero-row batches
    /// for beyond-EOF pages without hitting DuckDB.
    pub schema: SchemaRef,
    pub row_count: u64,
    cache: Mutex<LruCache<PageKey, Arc<RecordBatch>>>,
}

impl GridDataSource {
    /// Create a new `GridDataSource` for `table_name`, computing the initial
    /// row count via a `COUNT(*)` query and probing the schema via `LIMIT 1`.
    pub async fn new(engine: Arc<DuckDBEngine>, table_name: String) -> Result<Self> {
        let row_count = count_rows(&engine, &table_name)
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
            .execute_paged(&schema_sql, 0, 1)
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
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(LRU_CAPACITY).expect("LRU_CAPACITY is non-zero"),
            )),
        })
    }

    /// Return (or look up cached) the `RecordBatch` covering `row`.
    /// The batch is page-aligned to `PAGE_ROWS` boundaries.
    ///
    /// Cache hit: `O(1)` — returns `Arc::clone` of the cached batch.
    /// Cache miss: issues `SELECT * FROM "table"` and lets `execute_paged`
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
        if let Some(batch) = self.cache.lock().unwrap().get(&key) {
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
            self.cache
                .lock()
                .expect("grid cache poisoned")
                .put(key, Arc::clone(&empty));
            return Ok(empty);
        }

        // --- cache miss: fetch from DuckDB ---
        tracing::debug!(page_start = key.start, "grid page cache miss — fetching");

        // SQL-identifier safety: escape embedded double quotes by doubling them.
        // DuckDB does not support parameterized identifiers, so this is the
        // canonical injection safeguard.
        //
        // Do NOT embed LIMIT/OFFSET in this SQL — execute_paged wraps it as:
        //   SELECT * FROM (<sql>) sub LIMIT <limit> OFFSET <offset>
        // Adding them here would produce double pagination (offset applied
        // twice), returning empty results for every page beyond page 0.
        let safe_name = self.table_name.replace('"', "\"\"");
        let sql = format!("SELECT * FROM \"{}\"", safe_name);

        let result = self
            .engine
            .execute_paged(&sql, key.start, PAGE_ROWS)
            .await
            .context("execute_paged for grid page")?;

        let batch = result
            .batches
            .into_iter()
            .next()
            .context("empty result for grid page")?;

        let arc = Arc::new(batch);
        self.cache.lock().unwrap().put(key, Arc::clone(&arc));
        Ok(arc)
    }
}

/// Issue `SELECT COUNT(*)::BIGINT FROM "table"` and return the count.
///
/// Uses `engine.execute()` which does not wrap the query, avoiding the
/// double-subquery overhead of `execute_paged`. `COUNT(*)` returns exactly
/// one row with one column.
async fn count_rows(engine: &DuckDBEngine, table_name: &str) -> Result<u64> {
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
    use dat0_engine::{MemoryBudget, RegisterOpts};
    use tempfile::TempDir;

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
}
