//! Export a table to bytes via DuckDB COPY ... TO. Writes to a tempfile,
//! reads back the bytes, returns. Streaming export for files-larger-than-RAM
//! is deferred (spec §4 out-of-scope).

use std::io::Read;

use crate::Result;
use crate::catalog::quote_ident;
use crate::error::EngineError;
use crate::types::ExportFormat;

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
    let path = tmp.path().to_path_buf();
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.clone()))?
        .replace('\'', "''");

    let qtable = quote_ident(table);
    let copy_sql = match format {
        ExportFormat::Csv => format!(
            "COPY (SELECT * FROM {}) TO '{}' (FORMAT CSV, HEADER)",
            qtable, path_str
        ),
        ExportFormat::Json => format!(
            "COPY (SELECT * FROM {}) TO '{}' (FORMAT JSON, ARRAY)",
            qtable, path_str
        ),
        ExportFormat::Parquet => format!(
            "COPY (SELECT * FROM {}) TO '{}' (FORMAT PARQUET)",
            qtable, path_str
        ),
    };
    conn.execute_batch(&copy_sql)?;

    let mut bytes = Vec::new();
    let mut f = std::fs::File::open(&path).map_err(EngineError::Io)?;
    f.read_to_end(&mut bytes).map_err(EngineError::Io)?;
    Ok(bytes)
}
