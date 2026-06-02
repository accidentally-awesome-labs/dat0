//! Export a table to bytes via DuckDB COPY ... TO. Streaming export to a
//! destination path lands in P4c via `export_query_to_path`;
//! `export_table_bytes` delegates to it through a tempfile for the in-memory
//! API.

use std::io::Read;
use std::path::Path;

use crate::Result;
use crate::catalog::quote_ident;
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

pub(crate) fn export_table_bytes(
    conn: &duckdb::Connection,
    table: &str,
    format: ExportFormat,
) -> Result<Vec<u8>> {
    let tmp = tempfile::Builder::new()
        .prefix("dat0-export-")
        .suffix(match format {
            ExportFormat::Csv => ".csv",
            ExportFormat::Json => ".json",
            ExportFormat::Parquet => ".parquet",
        })
        .tempfile()
        .map_err(EngineError::Io)?;
    let select = format!("SELECT * FROM {}", quote_ident(table));
    export_query_to_path(conn, &select, format, tmp.path())?;
    let mut bytes = Vec::new();
    let mut f = std::fs::File::open(tmp.path()).map_err(EngineError::Io)?;
    f.read_to_end(&mut bytes).map_err(EngineError::Io)?;
    Ok(bytes)
}
