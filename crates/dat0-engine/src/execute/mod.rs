//! `execute()` — materialized result. T8 adds paged + streaming.

pub mod paged;
pub mod streaming;

use crate::Result;
use crate::error::EngineError;
use crate::types::{ColumnInfo, QueryResult};

/// Hard ceiling on rows [`run_materialized`] will accumulate in RAM.
///
/// `execute()` buffers the ENTIRE result before returning, so an accidental
/// `SELECT * FROM taxi` (1.2 bn rows) is an OOM, not a slow query. One million
/// rows is comfortably above every legitimate `execute()` consumer (inspector
/// profiles, chart series, scalar probes) and far below a budget-busting
/// buffer.
pub const MAX_MATERIALIZED_ROWS: u64 = 1_000_000;

/// Drive DuckDB's Arrow batch iterator and materialize the full result set.
/// Used by the trait `execute()` for "small" results consumed by inspector /
/// charts. Streaming + paged variants live in T8.
pub(crate) fn run_materialized(conn: &duckdb::Connection, sql: &str) -> Result<QueryResult> {
    let mut stmt = conn.prepare(sql).map_err(translate_duckdb_err)?;
    let arrow_iter = stmt.query_arrow([]).map_err(translate_duckdb_err)?;
    // Capture the schema BEFORE consuming the iterator — `Arrow<'_>::get_schema`
    // borrows the iterator, and post-consumption it's gone.
    let schema = arrow_iter.get_schema();
    // D-030: the Arrow iterator yields a bare `RecordBatch`, not a
    // `Result<RecordBatch, _>`, so a mid-stream error collapses silently into
    // end-of-stream. Not detectable here — this path has no row count to
    // reconcile against (only `execute::paged::run_paged` does). Prepare-time
    // errors ARE caught above.
    let mut batches: Vec<duckdb::arrow::record_batch::RecordBatch> = Vec::new();
    let mut rows: u64 = 0;
    for batch in arrow_iter {
        rows += batch.num_rows() as u64;
        if rows > MAX_MATERIALIZED_ROWS {
            // Stop accumulating: the point is to not hold the result, so the
            // partial `batches` is dropped with the error rather than returned.
            return Err(EngineError::EngineFailed(format!(
                "result exceeds {MAX_MATERIALIZED_ROWS} rows; use execute_page"
            )));
        }
        batches.push(batch);
    }
    let columns: Vec<ColumnInfo> = schema
        .fields()
        .iter()
        .map(|f| ColumnInfo {
            name: f.name().clone(),
            data_type: format!("{:?}", f.data_type()),
            nullable: f.is_nullable(),
        })
        .collect();
    Ok(QueryResult { columns, batches })
}

/// Translate a `duckdb::Error` into the appropriate `EngineError`. Specifically:
/// when the underlying DuckDB call was interrupted (because a sibling task
/// called `Engine::interrupt()`), surface as `EngineError::Interrupted` rather
/// than a generic `DuckDb(_)`. P5 SQL Console depends on this discriminator
/// for Cmd+. UX (D-008). The substring match on `"INTERRUPT"` is heuristic —
/// D-008 will revisit at P5 once the cancellation UX is wired.
pub(crate) fn translate_duckdb_err(e: duckdb::Error) -> EngineError {
    if let duckdb::Error::DuckDBFailure(_, ref msg) = e
        && msg
            .as_deref()
            .map(|s| s.contains("INTERRUPT"))
            .unwrap_or(false)
    {
        return EngineError::Interrupted;
    }
    EngineError::DuckDb(e)
}
