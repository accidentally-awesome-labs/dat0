//! `register_file` dispatch by `FileFormat`.

pub mod csv;
pub mod json;
pub mod parquet;

use std::path::Path;

use crate::Result;
use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts};

/// Compute the table name from a path: stem in lowercase, with non-alphanum
/// replaced by `_`. Caller can override via SQL once the table exists.
pub(crate) fn derive_table_name(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("table");
    let mut name = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if name.is_empty() || name.chars().next().unwrap().is_ascii_digit() {
        name.insert(0, 't');
    }
    name
}

pub(crate) fn resolve_format(path: &Path, opts: &RegisterOpts) -> Result<FileFormat> {
    if let Some(f) = opts.format {
        return Ok(f);
    }
    FileFormat::from_extension(path).ok_or_else(|| {
        EngineError::UnsupportedFormat(format!(
            "cannot determine format from extension; pass RegisterOpts.format explicitly: {}",
            path.display()
        ))
    })
}

pub(crate) fn dispatch_register_sql(
    path: &Path,
    opts: &RegisterOpts,
    table_name: &str,
) -> Result<String> {
    let format = resolve_format(path, opts)?;
    match format {
        FileFormat::Csv | FileFormat::Tsv => {
            csv::build_csv_view_sql(path, opts, table_name, format)
        }
        FileFormat::Json | FileFormat::Jsonl | FileFormat::Ndjson => {
            json::build_json_view_sql(path, opts, table_name, format)
        }
        FileFormat::Parquet => parquet::build_parquet_view_sql(path, table_name),
    }
}
