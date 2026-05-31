//! T7 — paste coercion tests.
//!
//! `coerce_cell` takes the column's real Arrow `DataType` (not the coarse
//! `ColumnType` enum) so int/float fidelity is preserved: pasting "42" into an
//! Int column yields `Scalar::Int(42)` (SQL literal `42`), into a Float column
//! `Scalar::Float(42.0)` (SQL literal `42.0`). The edit-overlay literal differs
//! per the engine's `render_scalar`, and a `42.0` literal into an INT column can
//! mis-type — hence the Arrow `DataType` is the right parameter (it's also the
//! type the paste handler already has from the schema). Invalid parses coerce
//! to `CoerceResult::Skip` (coerce-or-skip).

use dat0_app::grid::clipboard::{CoerceResult, coerce_cell};
use dat0_engine::Scalar;
use duckdb::arrow::datatypes::{DataType, TimeUnit};

#[test]
fn coerce_int_ok_and_reject() {
    assert!(matches!(
        coerce_cell("42", &DataType::Int64),
        CoerceResult::Ok(Scalar::Int(42))
    ));
    assert!(matches!(
        coerce_cell("abc", &DataType::Int64),
        CoerceResult::Skip
    ));
}

#[test]
fn coerce_str_always_ok() {
    assert!(matches!(
        coerce_cell("anything", &DataType::Utf8),
        CoerceResult::Ok(Scalar::Str(_))
    ));
}

#[test]
fn coerce_float_into_float_column_keeps_float_type() {
    // "42" into a Float column must become Scalar::Float(42.0), NOT Scalar::Int.
    // The engine renders Float as `42.0` and Int as `42`; the column type
    // decides, not the lexical form of the pasted text.
    assert!(matches!(
        coerce_cell("42", &DataType::Float64),
        CoerceResult::Ok(Scalar::Float(f)) if (f - 42.0).abs() < f64::EPSILON
    ));
    assert!(matches!(
        coerce_cell("2.5", &DataType::Float64),
        CoerceResult::Ok(Scalar::Float(f)) if (f - 2.5).abs() < f64::EPSILON
    ));
    assert!(matches!(
        coerce_cell("not-a-number", &DataType::Float64),
        CoerceResult::Skip
    ));
}

#[test]
fn coerce_int_with_decimal_point_is_skipped() {
    // "42.0" into an INT column must NOT silently truncate; coerce-or-skip
    // rejects it (it isn't a valid i64 literal).
    assert!(matches!(
        coerce_cell("42.0", &DataType::Int64),
        CoerceResult::Skip
    ));
}

#[test]
fn coerce_bool_accepts_true_false_and_one_zero() {
    assert!(matches!(
        coerce_cell("true", &DataType::Boolean),
        CoerceResult::Ok(Scalar::Bool(true))
    ));
    assert!(matches!(
        coerce_cell("FALSE", &DataType::Boolean),
        CoerceResult::Ok(Scalar::Bool(false))
    ));
    assert!(matches!(
        coerce_cell("1", &DataType::Boolean),
        CoerceResult::Ok(Scalar::Bool(true))
    ));
    assert!(matches!(
        coerce_cell("0", &DataType::Boolean),
        CoerceResult::Ok(Scalar::Bool(false))
    ));
    assert!(matches!(
        coerce_cell("maybe", &DataType::Boolean),
        CoerceResult::Skip
    ));
}

#[test]
fn coerce_date_validates_iso() {
    assert!(matches!(
        coerce_cell("2026-05-30", &DataType::Date32),
        CoerceResult::Ok(Scalar::Date(_))
    ));
    assert!(matches!(
        coerce_cell("not-a-date", &DataType::Date32),
        CoerceResult::Skip
    ));
}

#[test]
fn coerce_timestamp_validates_iso() {
    let ts_type = DataType::Timestamp(TimeUnit::Microsecond, None);
    assert!(matches!(
        coerce_cell("2026-05-30 12:34:56", &ts_type),
        CoerceResult::Ok(Scalar::Timestamp(_))
    ));
    assert!(matches!(coerce_cell("nope", &ts_type), CoerceResult::Skip));
}

#[test]
fn coerce_unknown_type_falls_back_to_str() {
    // An unhandled Arrow type coerces to Str (always Ok) — the safe,
    // non-destructive default, mirroring `GridDataSource::column_type`.
    assert!(matches!(
        coerce_cell("anything", &DataType::LargeUtf8),
        CoerceResult::Ok(Scalar::Str(_))
    ));
}
