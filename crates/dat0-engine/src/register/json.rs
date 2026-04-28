//! JSON/JSONL/NDJSON registration via DuckDB `read_json` / `read_json_auto`.

use std::path::Path;

use crate::Result;
use crate::catalog::quote_ident;
use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts};

pub(crate) fn build_json_view_sql(
    path: &Path,
    opts: &RegisterOpts,
    table_name: &str,
    format: FileFormat,
) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.to_path_buf()))?;
    let escaped_path = path_str.replace('\'', "''");

    // DuckDB `read_json` `format` param:
    //   'auto'                 — sniff
    //   'array'                — single JSON array (.json)
    //   'newline_delimited'    — JSONL/NDJSON
    //   'unstructured'         — non-array, non-NDJSON
    let format_clause = match format {
        FileFormat::Json => "format='auto'",
        FileFormat::Jsonl | FileFormat::Ndjson => "format='newline_delimited'",
        _ => return Err(EngineError::UnsupportedFormat(format!("{:?}", format))),
    };

    let mut params: Vec<String> = vec![format_clause.to_string()];
    if let Some(s) = opts.sample_rows {
        if s == 0 {
            return Err(EngineError::InvalidOption {
                field: "sample_rows",
                reason: "must be > 0 when set; use None for default".into(),
            });
        }
        params.push(format!("sample_size={}", s));
    }
    // SEMANTIC NOTE: DuckDB's read_json `columns={...}` parameter has SUBSET
    // semantics — when set, only the listed columns are exposed (other columns
    // are dropped). This is materially different from read_csv's `types={...}`
    // which is a partial override leaving non-listed columns auto-detected.
    // Applying RegisterOpts.type_overrides as a `columns={}` clause would
    // therefore silently drop columns the user didn't list — a contract bug.
    // For P2, JSON registration ignores type_overrides. P3 import wizard or
    // a later phase wires JSON column-type overrides via a different shape
    // (likely a separate full-schema field). If type_overrides is non-empty
    // for a JSON file, we surface a clear error rather than silently dropping
    // columns.
    if !opts.type_overrides.is_empty() {
        return Err(EngineError::InvalidOption {
            field: "type_overrides",
            reason: "not yet supported for JSON formats in P2 (DuckDB read_json's `columns` \
                     param has subset, not partial-override, semantics — applying it would \
                     silently drop other columns). Use the column-typed result via a \
                     follow-up SQL CAST instead."
                .into(),
        });
    }
    let args = format!("'{}', {}", escaped_path, params.join(", "));
    Ok(format!(
        "CREATE OR REPLACE VIEW {} AS SELECT * FROM read_json({});",
        quote_ident(table_name),
        args
    ))
}
