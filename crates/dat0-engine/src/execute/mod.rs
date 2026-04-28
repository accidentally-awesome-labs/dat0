//! `execute()` — materialized result. T8 adds paged + streaming.

pub mod paged;
pub mod streaming;

use crate::Result;
use crate::error::EngineError;
use crate::types::{ColumnInfo, QueryResult};

/// Drive DuckDB's Arrow batch iterator and materialize the full result set.
/// Used by the trait `execute()` for "small" results consumed by inspector /
/// charts. Streaming + paged variants live in T8.
pub(crate) fn run_materialized(conn: &duckdb::Connection, sql: &str) -> Result<QueryResult> {
    let mut stmt = conn.prepare(sql).map_err(translate_duckdb_err)?;
    let arrow_iter = stmt.query_arrow([]).map_err(translate_duckdb_err)?;
    // Capture the schema BEFORE consuming the iterator — `Arrow<'_>::get_schema`
    // borrows the iterator, and post-consumption it's gone.
    let schema = arrow_iter.get_schema();
    // T0 finding (docs/internal/duckdb-arrow-api-notes.md gotcha #2): the Arrow
    // iterator yields bare `RecordBatch`, not `Result<RecordBatch, _>`. Mid-stream
    // errors collapse silently into end-of-stream. Acceptable for T7's
    // materialized path: prepare-time errors are caught above; results are small
    // per spec; T8 streaming + T16 cancellation are where mid-stream errors matter.
    let mut batches: Vec<duckdb::arrow::record_batch::RecordBatch> = Vec::new();
    for batch in arrow_iter {
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
