//! Spreadsheet-TSV clipboard codec + paste coercion (T7).
//!
//! This is the **pure-logic** half of P4b cut/copy/paste: a hand-rolled
//! tab-separated codec that round-trips with the de-facto Excel / Google-Sheets
//! clipboard dialect, plus a coerce-or-skip helper for pasting strings into a
//! typed column. None of it touches GPUI or the database, so the whole module
//! is unit-tested (`tests/tsv_codec.rs`, `tests/paste_coerce.rs`). The thin
//! GPUI glue (the copy/cut/paste handlers on `WorkspaceShell`) lives in
//! `window.rs` and is build+clippy verified — the real Excel/Sheets round-trip
//! is the T14 manual UAT gate (GPUI has no headless clipboard-to-Excel test).
//!
//! ## Dialect (decision 6)
//! - Columns separated by `\t`.
//! - Rows separated by `\r\n` on serialize; parse accepts BOTH `\r\n` and `\n`
//!   (Sheets emits LF on some platforms, Excel emits CRLF).
//! - A cell is **quoted** iff it contains a `\t`, `\n`, `\r`, or `"`. Quoting
//!   wraps the cell in double-quotes and doubles every internal `"` (CSV-style).
//!
//! ## Coercion (coerce-or-skip)
//! [`coerce_cell`] takes the column's real Arrow [`DataType`] so int/float
//! fidelity is preserved (`Scalar::Int(42)` renders as `42`, `Scalar::Float`
//! as `42.0`; a `42.0` literal into an INT column can mis-type). A cell that
//! does not parse for its column type is [`CoerceResult::Skip`]ped, not coerced
//! into a wrong type.

use dat0_engine::Scalar;
use duckdb::arrow::datatypes::DataType;

/// Serialize a row-major grid into the spreadsheet-TSV dialect.
///
/// Rows are joined by `\r\n`, cells within a row by `\t`. A cell containing any
/// of `\t` / `\n` / `\r` / `"` is wrapped in double-quotes with each internal
/// `"` doubled (`"` → `""`), exactly matching what Excel / Sheets emit.
pub fn tsv_serialize(grid: &[Vec<String>]) -> String {
    grid.iter()
        .map(|row| {
            row.iter()
                .map(|cell| quote_cell(cell))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Wrap `cell` in CSV-style quotes iff it contains a tab, CR, LF, or quote.
fn quote_cell(cell: &str) -> String {
    if cell.contains(['\t', '\n', '\r', '"']) {
        let escaped = cell.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        cell.to_string()
    }
}

/// Parse a spreadsheet-TSV blob into a row-major grid.
///
/// A hand-rolled state machine: `\t` ends a cell, `\n` ends a row (a bare `\r`
/// immediately before an unquoted `\n` is swallowed so CRLF and LF both work).
/// Quoted cells (opened by a `"` at the start of a field) carry literal tabs /
/// newlines, and `""` inside a quoted cell decodes to a single `"`.
///
/// Invariant: always returns at least one row containing at least one cell —
/// `tsv_parse("")` is `[[""]]`, never `[]`. Callers that must reject an empty
/// payload should test for an all-empty grid, not `is_empty()`.
pub fn tsv_parse(tsv: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    // `true` while we are inside a quoted field (between the opening `"` and its
    // matching closing `"`).
    let mut in_quotes = false;
    // `true` only at the very start of a field, where a leading `"` opens a
    // quoted field rather than being literal content.
    let mut at_field_start = true;

    let mut chars = tsv.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    // A doubled quote (`""`) is a literal `"`; a lone quote
                    // closes the quoted field.
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        cell.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                other => cell.push(other),
            }
            continue;
        }

        match c {
            '"' if at_field_start => {
                in_quotes = true;
                at_field_start = false;
            }
            '\t' => {
                row.push(std::mem::take(&mut cell));
                at_field_start = true;
            }
            '\r' => {
                // Swallow a CR that terminates a row (CRLF). A lone CR (no
                // following LF) also ends the row, matching old-Mac line ends.
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                at_field_start = true;
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                at_field_start = true;
            }
            other => {
                cell.push(other);
                at_field_start = false;
            }
        }
    }

    // Flush the trailing cell / row (the input has no trailing row separator).
    row.push(cell);
    rows.push(row);
    rows
}

/// Outcome of coercing one pasted string for a target column type.
pub enum CoerceResult {
    /// The string coerced cleanly into a typed [`Scalar`].
    Ok(Scalar),
    /// The string does not parse for this column type — skip it (coerce-or-skip),
    /// leaving the existing cell value untouched. Counts toward the paste-reject
    /// banner.
    Skip,
}

/// Coerce a pasted cell string into a [`Scalar`] for the column's Arrow type.
///
/// Takes the real Arrow [`DataType`] (not the coarse `ColumnType`) so int/float
/// fidelity is preserved — the paste handler has this type from the grid's Arrow
/// schema. Mapping:
/// - integer types (`Int8..Int64`, `UInt8..UInt64`) → `Scalar::Int` (parse `i64`)
/// - float / decimal types (`Float16/32/64`, `Decimal128/256`) → `Scalar::Float`
/// - `Boolean` → `Scalar::Bool` (accepts `true`/`false` case-insensitively, `1`/`0`)
/// - `Date32`/`Date64` → `validate_date` → `Scalar::Date`
/// - `Timestamp(..)` → `validate_timestamp` → `Scalar::Timestamp`
/// - everything else (`Utf8`/`LargeUtf8`/unknown) → `Scalar::Str` (always `Ok`)
///
/// Any parse/validation failure returns [`CoerceResult::Skip`].
pub fn coerce_cell(s: &str, ty: &DataType) -> CoerceResult {
    match ty {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => match s.trim().parse::<i64>() {
            Ok(i) => CoerceResult::Ok(Scalar::Int(i)),
            Err(_) => CoerceResult::Skip,
        },
        DataType::Float16
        | DataType::Float32
        | DataType::Float64
        | DataType::Decimal128(_, _)
        | DataType::Decimal256(_, _) => match s.trim().parse::<f64>() {
            Ok(f) => CoerceResult::Ok(Scalar::Float(f)),
            Err(_) => CoerceResult::Skip,
        },
        DataType::Boolean => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => CoerceResult::Ok(Scalar::Bool(true)),
            "false" | "0" => CoerceResult::Ok(Scalar::Bool(false)),
            _ => CoerceResult::Skip,
        },
        DataType::Date32 | DataType::Date64 => match Scalar::validate_date(s.trim()) {
            Ok(valid) => CoerceResult::Ok(Scalar::Date(valid.to_string())),
            Err(_) => CoerceResult::Skip,
        },
        DataType::Timestamp(_, _) => match Scalar::validate_timestamp(s.trim()) {
            Ok(valid) => CoerceResult::Ok(Scalar::Timestamp(valid.to_string())),
            Err(_) => CoerceResult::Skip,
        },
        // Utf8 / LargeUtf8 / anything else: strings always coerce (the safe,
        // non-destructive default, mirroring `GridDataSource::column_type`).
        _ => CoerceResult::Ok(Scalar::Str(s.to_string())),
    }
}
