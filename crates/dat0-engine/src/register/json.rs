//! JSON/JSONL/NDJSON registration. T5 fills this in.

use std::path::Path;

use crate::Result;
use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts};

pub(crate) fn build_json_view_sql(
    _path: &Path,
    _opts: &RegisterOpts,
    _table_name: &str,
    _format: FileFormat,
) -> Result<String> {
    Err(EngineError::NotImplemented {
        feature: "register_file json (T5)",
    })
}
