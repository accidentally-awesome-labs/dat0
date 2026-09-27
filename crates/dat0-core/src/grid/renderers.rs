//! Per-DataType cell renderers — type badges, NULL highlighting,
//! numeric right-alignment, BigInt as lossless string.

use dat0_engine::Scalar;
use duckdb::arrow::array::{
    Array, Float32Array, Float64Array, Int32Array, Int64Array, StringArray, UInt64Array,
};
use duckdb::arrow::datatypes::DataType;
use duckdb::arrow::record_batch::RecordBatch;
use duckdb::arrow::util::display::{ArrayFormatter, FormatOptions};
use std::borrow::Cow;

/// Render a single cell. Returns `(display, alignment, is_null)`.
/// Caller (Table delegate) wraps in the gpui-component cell widget.
///
/// Every downcast below sits inside the arm that already matched its
/// `DataType`, so it cannot fail — Arrow's type tag and its concrete array are
/// the same fact. These used to be `.expect("Int32Array")` anyway (EN5): this
/// runs once per painted cell, so a hypothetical mismatch would take the whole
/// window down mid-frame. `debug_assert!` keeps it loud in dev; release paints
/// a NULL cell, which is the same degradation an unmapped type already gets.
pub fn render_cell(batch: &RecordBatch, col: usize, row: usize) -> CellDisplay {
    let array = batch.column(col);
    if array.is_null(row) {
        return CellDisplay::null();
    }
    match array.data_type() {
        DataType::Int32 => {
            let Some(a) = array.as_any().downcast_ref::<Int32Array>() else {
                debug_assert!(false, "DataType::Int32 column is not an Int32Array");
                return CellDisplay::null();
            };
            CellDisplay::numeric(a.value(row).to_string())
        }
        DataType::Int64 => {
            let Some(a) = array.as_any().downcast_ref::<Int64Array>() else {
                debug_assert!(false, "DataType::Int64 column is not an Int64Array");
                return CellDisplay::null();
            };
            CellDisplay::big_int(a.value(row).to_string())
        }
        DataType::UInt64 => {
            let Some(a) = array.as_any().downcast_ref::<UInt64Array>() else {
                debug_assert!(false, "DataType::UInt64 column is not a UInt64Array");
                return CellDisplay::null();
            };
            CellDisplay::big_int(a.value(row).to_string())
        }
        DataType::Float64 => {
            let Some(a) = array.as_any().downcast_ref::<Float64Array>() else {
                debug_assert!(false, "DataType::Float64 column is not a Float64Array");
                return CellDisplay::null();
            };
            CellDisplay::numeric(format!("{:.6}", a.value(row)))
        }
        DataType::Float32 => {
            let Some(a) = array.as_any().downcast_ref::<Float32Array>() else {
                debug_assert!(false, "DataType::Float32 column is not a Float32Array");
                return CellDisplay::null();
            };
            CellDisplay::numeric(format!("{:.6}", a.value(row)))
        }
        DataType::Utf8 => {
            let Some(a) = array.as_any().downcast_ref::<StringArray>() else {
                debug_assert!(false, "DataType::Utf8 column is not a StringArray");
                return CellDisplay::null();
            };
            CellDisplay::text(a.value(row).to_string())
        }
        // The other numbers, exactly and right-aligned.
        DataType::Int8
        | DataType::Int16
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::Float16
        | DataType::Decimal128(_, _)
        | DataType::Decimal256(_, _) => CellDisplay::numeric(formatted(array.as_ref(), row)),
        // Booleans, dates, timestamps, lists, structs, and the rest. These
        // used to paint their type's name — `(Date32)`, `(Boolean)` — in
        // every cell, and a copy or a fill down carried that name as the
        // value.
        _ => CellDisplay::text(formatted(array.as_ref(), row)),
    }
}

/// The cell's value as a [`Scalar`]: what a verb that writes a value
/// elsewhere writes. Fill down copies the value rather than its display text,
/// so a float keeps its digits past the six the grid shows.
pub fn cell_scalar(batch: &RecordBatch, col: usize, row: usize) -> Scalar {
    let array = batch.column(col);
    if array.is_null(row) {
        return Scalar::Null;
    }
    let text = formatted(array.as_ref(), row);
    match array.data_type() {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => match text.parse::<i64>() {
            Ok(i) => Scalar::Int(i),
            // A UInt64 past `i64::MAX`.
            Err(_) => text.parse::<f64>().map_or(Scalar::Str(text), Scalar::Float),
        },
        DataType::Float16
        | DataType::Float32
        | DataType::Float64
        | DataType::Decimal128(_, _)
        | DataType::Decimal256(_, _) => {
            text.parse::<f64>().map_or(Scalar::Str(text), Scalar::Float)
        }
        DataType::Boolean => Scalar::Bool(text == "true"),
        DataType::Date32 | DataType::Date64 => Scalar::Date(text),
        DataType::Timestamp(_, _) => Scalar::Timestamp(text),
        _ => Scalar::Str(text),
    }
}

/// Arrow's rendering of one cell, with SQL's spelling of a date and a
/// timestamp (`2024-01-02 03:04:05`, not ISO's `T`), which is also what
/// [`Scalar::validate_timestamp`] reads back.
fn formatted(array: &dyn Array, row: usize) -> String {
    let options = FormatOptions::new()
        .with_date_format(Some("%Y-%m-%d"))
        .with_timestamp_format(Some("%Y-%m-%d %H:%M:%S%.f"))
        .with_timestamp_tz_format(Some("%Y-%m-%d %H:%M:%S%.f%:z"));
    match ArrayFormatter::try_new(array, &options) {
        Ok(f) => f.value(row).to_string(),
        Err(_) => format!("({:?})", array.data_type()),
    }
}

/// Column-type badge for the header (e.g., "INT64", "TEXT", "FLOAT64").
pub fn type_badge(data_type: &DataType) -> Cow<'static, str> {
    match data_type {
        DataType::Int32 => Cow::Borrowed("INT32"),
        DataType::Int64 => Cow::Borrowed("INT64"),
        DataType::UInt64 => Cow::Borrowed("UINT64"),
        DataType::Float64 => Cow::Borrowed("FLOAT64"),
        DataType::Utf8 => Cow::Borrowed("TEXT"),
        other => Cow::Owned(format!("{other:?}")),
    }
}

#[derive(Debug, Clone)]
pub struct CellDisplay {
    pub text: String,
    pub alignment: CellAlignment,
    pub is_null: bool,
    pub is_big_int: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum CellAlignment {
    Left,
    Right,
}

impl CellDisplay {
    pub fn null() -> Self {
        Self {
            text: "NULL".to_string(),
            alignment: CellAlignment::Left,
            is_null: true,
            is_big_int: false,
        }
    }
    pub fn text(s: String) -> Self {
        Self {
            text: s,
            alignment: CellAlignment::Left,
            is_null: false,
            is_big_int: false,
        }
    }
    pub fn numeric(s: String) -> Self {
        Self {
            text: s,
            alignment: CellAlignment::Right,
            is_null: false,
            is_big_int: false,
        }
    }
    pub fn big_int(s: String) -> Self {
        Self {
            text: s,
            alignment: CellAlignment::Right,
            is_null: false,
            is_big_int: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::arrow::array::{Int64Builder, StringBuilder};
    use duckdb::arrow::datatypes::{Field, Schema};
    use std::sync::Arc;

    fn batch_with(int_col: Vec<Option<i64>>, str_col: Vec<Option<&str>>) -> RecordBatch {
        let mut ib = Int64Builder::new();
        for v in int_col {
            match v {
                Some(x) => ib.append_value(x),
                None => ib.append_null(),
            }
        }
        let mut sb = StringBuilder::new();
        for v in str_col {
            match v {
                Some(s) => sb.append_value(s),
                None => sb.append_null(),
            }
        }
        let schema = Arc::new(Schema::new(vec![
            Field::new("i", DataType::Int64, true),
            Field::new("s", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(schema, vec![Arc::new(ib.finish()), Arc::new(sb.finish())]).unwrap()
    }

    #[test]
    fn int64_renders_as_big_int_right_aligned() {
        let b = batch_with(vec![Some(42)], vec![Some("x")]);
        let c = render_cell(&b, 0, 0);
        assert_eq!(c.text, "42");
        assert!(c.is_big_int);
        assert!(matches!(c.alignment, CellAlignment::Right));
        assert!(!c.is_null);
    }

    #[test]
    fn null_renders_with_null_flag() {
        let b = batch_with(vec![None], vec![None]);
        let c = render_cell(&b, 0, 0);
        assert!(c.is_null);
        assert_eq!(c.text, "NULL");
    }

    #[test]
    fn utf8_renders_left_aligned() {
        let b = batch_with(vec![Some(1)], vec![Some("hello")]);
        let c = render_cell(&b, 1, 0);
        assert_eq!(c.text, "hello");
        assert!(matches!(c.alignment, CellAlignment::Left));
        assert!(!c.is_null);
    }

    fn one(field: Field, array: duckdb::arrow::array::ArrayRef) -> RecordBatch {
        RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![array]).unwrap()
    }

    #[test]
    fn every_type_renders_its_value_not_its_name() {
        use duckdb::arrow::array::{
            BooleanArray, Date32Array, Decimal128Array, Int16Array, TimestampMicrosecondArray,
        };
        use duckdb::arrow::datatypes::TimeUnit;

        let b = one(
            Field::new("b", DataType::Boolean, true),
            Arc::new(BooleanArray::from(vec![true])),
        );
        assert_eq!(render_cell(&b, 0, 0).text, "true");

        // 2024-01-02 is day 19724 of the epoch.
        let d = one(
            Field::new("d", DataType::Date32, true),
            Arc::new(Date32Array::from(vec![19_724])),
        );
        assert_eq!(render_cell(&d, 0, 0).text, "2024-01-02");

        let micros = 1_704_164_645_000_000_i64; // 2024-01-02 03:04:05 UTC
        let t = one(
            Field::new("t", DataType::Timestamp(TimeUnit::Microsecond, None), true),
            Arc::new(TimestampMicrosecondArray::from(vec![micros])),
        );
        assert_eq!(render_cell(&t, 0, 0).text, "2024-01-02 03:04:05");

        let dec = Decimal128Array::from(vec![12_345_i128])
            .with_precision_and_scale(10, 2)
            .unwrap();
        let n = one(
            Field::new("n", DataType::Decimal128(10, 2), true),
            Arc::new(dec),
        );
        let c = render_cell(&n, 0, 0);
        assert_eq!(c.text, "123.45");
        assert!(matches!(c.alignment, CellAlignment::Right));

        let s = one(
            Field::new("s", DataType::Int16, true),
            Arc::new(Int16Array::from(vec![-7_i16])),
        );
        assert_eq!(render_cell(&s, 0, 0).text, "-7");
    }

    #[test]
    fn a_cells_scalar_is_its_typed_value() {
        use duckdb::arrow::array::{BooleanArray, Date32Array, Float64Array};

        let f = one(
            Field::new("f", DataType::Float64, true),
            Arc::new(Float64Array::from(vec![Some(0.123_456_789), None])),
        );
        // The grid shows six places; the value keeps all of them.
        assert_eq!(render_cell(&f, 0, 0).text, "0.123457");
        assert_eq!(cell_scalar(&f, 0, 0), Scalar::Float(0.123_456_789));
        assert_eq!(cell_scalar(&f, 0, 1), Scalar::Null);

        let b = batch_with(vec![Some(42)], vec![Some("x")]);
        assert_eq!(cell_scalar(&b, 0, 0), Scalar::Int(42));
        assert_eq!(cell_scalar(&b, 1, 0), Scalar::Str("x".into()));

        let t = one(
            Field::new("t", DataType::Boolean, true),
            Arc::new(BooleanArray::from(vec![false])),
        );
        assert_eq!(cell_scalar(&t, 0, 0), Scalar::Bool(false));

        let d = one(
            Field::new("d", DataType::Date32, true),
            Arc::new(Date32Array::from(vec![19_724])),
        );
        assert_eq!(cell_scalar(&d, 0, 0), Scalar::Date("2024-01-02".into()));
    }

    #[test]
    fn type_badge_int64() {
        assert_eq!(type_badge(&DataType::Int64).as_ref(), "INT64");
    }
}
