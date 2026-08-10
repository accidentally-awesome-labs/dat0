//! Per-DataType cell renderers — type badges, NULL highlighting,
//! numeric right-alignment, BigInt as lossless string.

use duckdb::arrow::array::{Array, Float64Array, Int32Array, Int64Array, StringArray, UInt64Array};
use duckdb::arrow::datatypes::DataType;
use duckdb::arrow::record_batch::RecordBatch;
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
        DataType::Utf8 => {
            let Some(a) = array.as_any().downcast_ref::<StringArray>() else {
                debug_assert!(false, "DataType::Utf8 column is not a StringArray");
                return CellDisplay::null();
            };
            CellDisplay::text(a.value(row).to_string())
        }
        other => CellDisplay::text(format!("({:?})", other)),
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

    #[test]
    fn type_badge_int64() {
        assert_eq!(type_badge(&DataType::Int64).as_ref(), "INT64");
    }
}
