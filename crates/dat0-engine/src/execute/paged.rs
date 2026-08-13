//! Paged execution: two forms of the same window.
//!
//! [`run_paged`] pays for a `COUNT(*)` and gets a reconciled, self-checking
//! result; [`run_page`] pays nothing and gets neither. See the trait docs on
//! `QueryEngine::execute_paged` / `execute_page` for which one a caller wants.

use crate::Result;
use crate::error::EngineError;
use crate::execute::translate_duckdb_err;
use crate::types::PagedQueryResult;

/// Window `sql` AND compute the exact total row count.
///
/// The `COUNT(*)` wrap is a full scan of the source per call — that is the cost
/// contract, and it is the reason [`run_page`] exists for callers that already
/// hold the count.
///
/// The count buys one thing beyond a number: it is the ONLY way this driver can
/// detect a truncated result. See the reconcile block below and D-030.
pub(crate) fn run_paged(
    conn: &duckdb::Connection,
    sql: &str,
    offset: u64,
    limit: u64,
) -> Result<PagedQueryResult> {
    // Compute total via wrapping COUNT(*). DuckDB optimizes; on large queries
    // this still walks the source — that is the cost contract.
    let count_sql = format!("SELECT COUNT(*) FROM ({}) sub", sql);
    let total_rows: u64 = conn
        .query_row(&count_sql, [], |r| r.get::<_, u64>(0))
        .map_err(translate_duckdb_err)?;

    let batches = window_batches(conn, sql, offset, limit)?;

    // Reconcile the stream against the count (EN3 / D-030). `Arrow::next` yields
    // a bare `RecordBatch`, so a mid-stream DuckDB error ends the loop exactly
    // as EOF does; comparing what arrived against what the count says should
    // have arrived is the only truncation detector available at duckdb-rs 1.4.4.
    // Only the counted path can do this — `run_page` has no expectation to
    // compare against.
    let expected = limit.min(total_rows.saturating_sub(offset));
    let got: u64 = batches.iter().map(|b| b.num_rows() as u64).sum();
    if got != expected {
        return Err(EngineError::EngineFailed(format!(
            "result stream ended early: expected {expected} rows, got {got}"
        )));
    }

    Ok(PagedQueryResult {
        total_rows: Some(total_rows),
        offset,
        batches,
    })
}

/// Window `sql` with no `COUNT(*)`. `total_rows` is `None`.
///
/// This is the grid's page-fetch path: `GridDataSource` counts once at bind and
/// then scrolls, so re-counting per page would make every page O(N).
pub(crate) fn run_page(
    conn: &duckdb::Connection,
    sql: &str,
    offset: u64,
    limit: u64,
) -> Result<PagedQueryResult> {
    Ok(PagedQueryResult {
        total_rows: None,
        offset,
        batches: window_batches(conn, sql, offset, limit)?,
    })
}

/// `SELECT * FROM (<sql>) sub LIMIT <limit> OFFSET <offset>`, drained to a
/// `Vec`. Shared by both forms so the windowing SQL exists in one place.
///
/// Errors route through [`translate_duckdb_err`] so an interrupted page surfaces
/// as [`EngineError::Interrupted`] and not a generic `DuckDb(_)` — a bare `?`
/// here (which is what `run_paged` used to do) converts via the `#[from]` impl
/// and loses that discriminator, which the SQL console's Cmd+. UX reads.
///
/// # An empty result still carries its schema
///
/// `Arrow` yields **no batches at all** when a query matches no rows, so a
/// caller that needs the column set — rather than the values — got nothing back
/// and could not tell "no rows" from "no such shape". `GridDataSource::new`
/// probes with `LIMIT 1` for exactly that reason and used to fail outright on a
/// zero-row table, which made a header-only CSV unopenable and left
/// `GridDataSource::is_empty()` unreachable.
///
/// The schema is available the whole time — `Arrow::get_schema` reads the
/// prepared statement, not the stream — so it is captured before the drain and
/// returned as one zero-row `RecordBatch`. Row-counting callers are unaffected:
/// an empty batch contributes 0 to `run_paged`'s reconcile sum, and to every
/// `num_rows()` fold downstream.
fn window_batches(
    conn: &duckdb::Connection,
    sql: &str,
    offset: u64,
    limit: u64,
) -> Result<Vec<duckdb::arrow::record_batch::RecordBatch>> {
    let windowed_sql = format!(
        "SELECT * FROM ({}) sub LIMIT {} OFFSET {}",
        sql, limit, offset
    );
    let mut stmt = conn.prepare(&windowed_sql).map_err(translate_duckdb_err)?;
    let arrow_iter = stmt.query_arrow([]).map_err(translate_duckdb_err)?;
    // Before the drain: the iterator borrows the statement, and the schema is
    // the one thing an empty result still has to offer.
    let schema = arrow_iter.get_schema();
    // D-030: this iterator yields a bare `RecordBatch`, not a `Result`, so a
    // mid-stream error is indistinguishable from end-of-stream. `run_paged`
    // reconciles against its count; `run_page` cannot.
    let mut batches = Vec::new();
    for b in arrow_iter {
        batches.push(b);
    }
    if batches.is_empty() {
        batches.push(duckdb::arrow::record_batch::RecordBatch::new_empty(schema));
    }
    Ok(batches)
}
