//! Paged execution: total_rows + windowed batches.

use crate::Result;
use crate::types::PagedQueryResult;

pub(crate) fn run_paged(
    conn: &duckdb::Connection,
    sql: &str,
    offset: u64,
    limit: u64,
) -> Result<PagedQueryResult> {
    // Compute total via wrapping COUNT(*). DuckDB optimizes; on large queries
    // this still walks the source — that is the cost contract.
    let count_sql = format!("SELECT COUNT(*) FROM ({}) sub", sql);
    let total_rows: u64 = conn.query_row(&count_sql, [], |r| r.get::<_, u64>(0))?;

    let windowed_sql = format!(
        "SELECT * FROM ({}) sub LIMIT {} OFFSET {}",
        sql, limit, offset
    );
    let mut stmt = conn.prepare(&windowed_sql)?;
    let arrow_iter = stmt.query_arrow([])?;
    let mut batches = Vec::new();
    for b in arrow_iter {
        batches.push(b);
    }
    Ok(PagedQueryResult {
        total_rows,
        offset,
        batches,
    })
}
