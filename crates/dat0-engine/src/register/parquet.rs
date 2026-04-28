//! Parquet registration. T6 fills this in.

use std::path::Path;

use crate::Result;
use crate::error::EngineError;

pub(crate) fn build_parquet_view_sql(_path: &Path, _table_name: &str) -> Result<String> {
    Err(EngineError::NotImplemented {
        feature: "register_file parquet (T6)",
    })
}
