//! SQL render for typed `Transformation` ops → DuckDB SELECT fragment.
//!
//! - All filters fold into one parenthesised `AND`-joined WHERE clause.
//! - Sort folds into one ORDER BY (last `Sort` op wins per UI semantics; render
//!   is defensive — if two `Sort`s appear, the later one fully replaces).
//! - All identifiers go through `catalog::quote_ident` (no raw concat).
//! - All string values are single-quote escaped (`'` → `''`).
//! - Floats render via `{:?}` debug for round-trip precision, not `{}`.
//! - Nulls + `Eq`/`Neq` ops are rewritten to `IS NULL` / `IS NOT NULL`.

use crate::catalog::quote_ident;
use crate::transform::{FilterOp, FilterValue, Scalar, SortDirection, SortKey, Transformation};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RenderError {
    #[error("FilterOp::In requires at least one value")]
    EmptyInList,
    #[error("FilterOp::Regex value failed to compile: {0}")]
    InvalidRegex(String),
    #[error("FilterOp::Between requires Range value, got {0}")]
    MismatchedRange(&'static str),
    #[error("FilterOp::{op:?} is not supported on value shape {value_shape}")]
    UnsupportedOpForType {
        op: FilterOp,
        value_shape: &'static str,
    },
}

/// Render the active op stack against `base` into a DuckDB-executable SELECT.
///
/// `base` must already be quoted (e.g. `"main"."orders"`). Caller owns the
/// quoting because it knows the schema scope (main vs an attached alias).
pub fn compile_view_sql(base: &str, ops: &[Transformation]) -> Result<String, RenderError> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut sort: Option<&[SortKey]> = None;

    for op in ops {
        match op {
            Transformation::Filter { column, op, value } => {
                where_clauses.push(render_filter(column, op, value)?);
            }
            Transformation::Sort { keys } => {
                sort = Some(keys.as_slice());
            }
        }
    }

    let mut sql = format!("SELECT * FROM {}", base);
    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }
    if let Some(keys) = sort {
        if !keys.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&render_sort(keys));
        }
    }
    Ok(sql)
}

fn render_filter(column: &str, op: &FilterOp, value: &FilterValue) -> Result<String, RenderError> {
    let col = quote_ident(column);
    match (op, value) {
        // Null short-circuits for Eq / Neq
        (
            FilterOp::Eq,
            FilterValue::Scalar {
                value: Scalar::Null,
            },
        ) => Ok(format!("({} IS NULL)", col)),
        (
            FilterOp::Neq,
            FilterValue::Scalar {
                value: Scalar::Null,
            },
        ) => Ok(format!("({} IS NOT NULL)", col)),

        // Comparison ops on scalars
        (FilterOp::Eq, FilterValue::Scalar { value: s }) => {
            Ok(format!("({} = {})", col, render_scalar(s)))
        }
        (FilterOp::Neq, FilterValue::Scalar { value: s }) => {
            Ok(format!("({} <> {})", col, render_scalar(s)))
        }
        (FilterOp::Lt, FilterValue::Scalar { value: s }) => {
            Ok(format!("({} < {})", col, render_scalar(s)))
        }
        (FilterOp::Lte, FilterValue::Scalar { value: s }) => {
            Ok(format!("({} <= {})", col, render_scalar(s)))
        }
        (FilterOp::Gt, FilterValue::Scalar { value: s }) => {
            Ok(format!("({} > {})", col, render_scalar(s)))
        }
        (FilterOp::Gte, FilterValue::Scalar { value: s }) => {
            Ok(format!("({} >= {})", col, render_scalar(s)))
        }

        // Between requires a Range
        (FilterOp::Between, FilterValue::Range { lo, hi, inclusive }) => {
            // BETWEEN is always inclusive in SQL; for exclusive bounds emit
            // an explicit `> lo AND < hi` pair.
            let (lo_s, hi_s) = (render_scalar(lo), render_scalar(hi));
            if *inclusive {
                Ok(format!("({} BETWEEN {} AND {})", col, lo_s, hi_s))
            } else {
                Ok(format!("({} > {} AND {} < {})", col, lo_s, col, hi_s))
            }
        }
        (FilterOp::Between, _) => Err(RenderError::MismatchedRange("expected Range")),

        // String ops on Str values
        (
            FilterOp::Contains,
            FilterValue::Scalar {
                value: Scalar::Str(s),
            },
        ) => Ok(format!("({} LIKE '%{}%' ESCAPE '\\')", col, escape_like(s))),
        (
            FilterOp::NotContains,
            FilterValue::Scalar {
                value: Scalar::Str(s),
            },
        ) => Ok(format!(
            "({} NOT LIKE '%{}%' ESCAPE '\\')",
            col,
            escape_like(s)
        )),
        (
            FilterOp::StartsWith,
            FilterValue::Scalar {
                value: Scalar::Str(s),
            },
        ) => Ok(format!("({} LIKE '{}%' ESCAPE '\\')", col, escape_like(s))),
        (
            FilterOp::EndsWith,
            FilterValue::Scalar {
                value: Scalar::Str(s),
            },
        ) => Ok(format!("({} LIKE '%{}' ESCAPE '\\')", col, escape_like(s))),

        // IN list
        (FilterOp::In, FilterValue::List { values: items }) => {
            if items.is_empty() {
                return Err(RenderError::EmptyInList);
            }
            let rendered: Vec<String> = items.iter().map(render_scalar).collect();
            Ok(format!("({} IN ({}))", col, rendered.join(", ")))
        }

        // Regex
        (
            FilterOp::Regex,
            FilterValue::Scalar {
                value: Scalar::Str(s),
            },
        ) => {
            // Compile-and-discard for syntax validation only; DuckDB uses its own
            // regex engine at query time. Cheap for typical patterns. Caller (T13)
            // must debounce input bursts so this is not re-run per keystroke.
            regex::Regex::new(s).map_err(|e| RenderError::InvalidRegex(e.to_string()))?;
            Ok(format!(
                "regexp_matches({}, '{}')",
                col,
                escape_single_quote(s)
            ))
        }

        // Nullary ops
        (FilterOp::IsEmpty, FilterValue::None) => Ok(format!("({} IS NULL)", col)),
        (FilterOp::IsNotEmpty, FilterValue::None) => Ok(format!("({} IS NOT NULL)", col)),
        (FilterOp::IsTrue, FilterValue::None) => Ok(format!("({} = TRUE)", col)),
        (FilterOp::IsFalse, FilterValue::None) => Ok(format!("({} = FALSE)", col)),

        // Anything else is a shape mismatch
        (other_op, other_value) => Err(RenderError::UnsupportedOpForType {
            op: *other_op,
            value_shape: filter_value_shape(other_value),
        }),
    }
}

fn render_sort(keys: &[SortKey]) -> String {
    keys.iter()
        .map(|k| {
            let dir = match k.direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            format!("{} {}", quote_ident(&k.column), dir)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render a `Scalar` value as a DuckDB SQL literal.
///
/// Assumes `Scalar::Date` and `Scalar::Timestamp` strings have already been
/// validated by `Scalar::validate_date` / `Scalar::validate_timestamp` at
/// input time (filter popover, T10). Malformed date/timestamp strings will
/// produce SQL that DuckDB rejects at query execution, not at render time.
/// Render does not duplicate validation — keep input-time and execution-time
/// concerns separate.
fn render_scalar(s: &Scalar) -> String {
    match s {
        Scalar::Null => "NULL".to_string(),
        Scalar::Bool(b) => {
            if *b {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Scalar::Int(i) => i.to_string(),
        Scalar::Float(f) => format!("{:?}", f),
        Scalar::Str(s) => format!("'{}'", escape_single_quote(s)),
        Scalar::Date(s) => format!("DATE '{}'", escape_single_quote(s)),
        Scalar::Timestamp(s) => format!("TIMESTAMP '{}'", escape_single_quote(s)),
    }
}

fn escape_single_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Escape characters meaningful to SQL LIKE so a user-typed `5%` matches the
/// literal `"5%"` not "fifty-percent of anything starting with 5". `\` is the
/// escape char per the `ESCAPE '\'` clause callers attach.
fn escape_like(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '%' => vec!['\\', '%'],
            '_' => vec!['\\', '_'],
            '\'' => vec!['\'', '\''],
            other => vec![other],
        })
        .collect()
}

fn filter_value_shape(v: &FilterValue) -> &'static str {
    match v {
        FilterValue::Scalar { .. } => "Scalar",
        FilterValue::Range { .. } => "Range",
        FilterValue::List { .. } => "List",
        FilterValue::None => "None",
    }
}
