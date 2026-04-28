//! Parquet registration via DuckDB `read_parquet`.

use std::path::Path;

use crate::Result;
use crate::catalog::quote_ident;
use crate::error::EngineError;

pub(crate) fn build_parquet_view_sql(path: &Path, table_name: &str) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.to_path_buf()))?;
    let escaped_path = path_str.replace('\'', "''");
    Ok(format!(
        "CREATE OR REPLACE VIEW {} AS SELECT * FROM read_parquet('{}');",
        quote_ident(table_name),
        escaped_path
    ))
}
