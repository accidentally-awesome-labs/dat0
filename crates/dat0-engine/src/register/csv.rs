//! CSV/TSV registration via DuckDB `read_csv`.

use std::path::Path;

use crate::Result;
use crate::catalog::quote_ident;
use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts};

pub(crate) fn build_csv_view_sql(
    path: &Path,
    opts: &RegisterOpts,
    table_name: &str,
    format: FileFormat,
) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.to_path_buf()))?;
    let escaped_path = path_str.replace('\'', "''");

    let delim = match (format, opts.delimiter) {
        (_, Some(c)) => Some(c),
        (FileFormat::Tsv, None) => Some('\t'),
        _ => None, // CSV: let DuckDB sniff
    };

    let mut params: Vec<String> = Vec::new();
    if let Some(d) = delim {
        params.push(format!(
            "delim='{}'",
            escape_for_sql_literal(&d.to_string())
        ));
    }
    if let Some(q) = opts.quote_char {
        params.push(format!(
            "quote='{}'",
            escape_for_sql_literal(&q.to_string())
        ));
    }
    if let Some(e) = opts.escape_char {
        params.push(format!(
            "escape='{}'",
            escape_for_sql_literal(&e.to_string())
        ));
    }
    if let Some(h) = opts.has_header {
        params.push(format!("header={}", h));
    }
    if let Some(s) = opts.sample_rows {
        if s == 0 {
            return Err(EngineError::InvalidOption {
                field: "sample_rows",
                reason: "must be > 0 when set; use None for default".into(),
            });
        }
        params.push(format!("sample_size={}", s));
    }
    if !opts.type_overrides.is_empty() {
        let mut entries = opts.type_overrides.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0)); // deterministic SQL
        let inner = entries
            .iter()
            .map(|(col, typ)| {
                format!(
                    "'{}': '{}'",
                    escape_for_sql_literal(col),
                    escape_for_sql_literal(typ)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        params.push(format!("types={{{}}}", inner));
    }

    let read_args = if params.is_empty() {
        format!("'{}'", escaped_path)
    } else {
        format!("'{}', {}", escaped_path, params.join(", "))
    };

    Ok(format!(
        "CREATE OR REPLACE VIEW {} AS SELECT * FROM read_csv({});",
        quote_ident(table_name),
        read_args
    ))
}

fn escape_for_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}
