//! Paged Arrow batch adapter feeding gpui-component's Table widget.

use anyhow::{Context, Result};
use dat0_engine::{DuckDBEngine, QueryEngine};
use duckdb::arrow::array::Int64Array;
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
    pub row_count: u64,
    cache: Mutex<LruCache<PageKey, Arc<RecordBatch>>>,
}

impl GridDataSource {
    /// Create a new `GridDataSource` for `table_name`, computing the initial
    /// row count via a `COUNT(*)` query.
    pub async fn new(engine: Arc<DuckDBEngine>, table_name: String) -> Result<Self> {
        let row_count = count_rows(&engine, &table_name)
            .await
            .context("GridDataSource::new — count_rows failed")?;
        Ok(Self {
            engine,
            table_name,
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
    /// Cache miss: issues `SELECT * FROM "table" LIMIT PAGE_ROWS OFFSET start`.
    pub async fn page_for(&self, row: u64) -> Result<Arc<RecordBatch>> {
        let key = PageKey {
            start: (row / PAGE_ROWS) * PAGE_ROWS,
        };

        // --- cache lookup ---
        if let Some(batch) = self.cache.lock().unwrap().get(&key) {
            tracing::trace!(page_start = key.start, "grid page cache hit");
            return Ok(Arc::clone(batch));
        }

        // --- cache miss: fetch from DuckDB ---
        tracing::debug!(page_start = key.start, "grid page cache miss — fetching");

        // SQL-identifier safety: escape embedded double quotes by doubling them.
        // DuckDB does not support parameterized identifiers, so this is the
        // canonical injection safeguard.
        let safe_name = self.table_name.replace('"', "\"\"");
        let sql = format!(
            "SELECT * FROM \"{}\" LIMIT {} OFFSET {}",
            safe_name, PAGE_ROWS, key.start,
        );

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
async fn count_rows(engine: &DuckDBEngine, table_name: &str) -> Result<u64> {
    let safe_name = table_name.replace('"', "\"\"");
    let sql = format!("SELECT COUNT(*)::BIGINT AS c FROM \"{}\"", safe_name);

    // offset=0, limit=1 — the COUNT query returns exactly one row.
    let result = engine
        .execute_paged(&sql, 0, 1)
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
}
