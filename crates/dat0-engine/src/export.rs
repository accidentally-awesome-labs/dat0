//! Export a SELECT to a destination path via DuckDB `COPY … TO`.
//!
//! There is deliberately no bytes-returning form. `export_table_bytes` existed
//! for a trait method (`export_table`) that no production caller ever used —
//! `dat0_format::writer` goes straight to [`export_query_to_path`]
//! (`writer.rs:66-68`) — and it `read_to_end`'d an entire export into a `Vec<u8>`,
//! which defeats the whole point of streaming to disk. Both were deleted in EN3.

use std::path::Path;

use crate::Result;
use crate::error::EngineError;
use crate::types::ExportFormat;

/// Stream a SELECT to `dest` via DuckDB `COPY (…) TO '<dest>' (FORMAT …)`.
/// No read-back — writes straight to disk, so it scales past RAM. `select_sql`
/// is a complete SELECT (caller owns its quoting / surrogate-strip).
pub(crate) fn export_query_to_path(
    conn: &duckdb::Connection,
    select_sql: &str,
    format: ExportFormat,
    dest: &Path,
) -> Result<()> {
    let path_str = dest
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(dest.to_path_buf()))?
        .replace('\'', "''");
    let opts = match format {
        ExportFormat::Csv => "FORMAT CSV, HEADER",
        ExportFormat::Json => "FORMAT JSON, ARRAY",
        ExportFormat::Parquet => "FORMAT PARQUET",
    };
    let copy_sql = format!("COPY ({}) TO '{}' ({})", select_sql, path_str, opts);
    conn.execute_batch(&copy_sql)?;
    Ok(())
}
