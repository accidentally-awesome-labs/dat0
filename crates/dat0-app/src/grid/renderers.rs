//! Per-DataType cell renderers — type badges, NULL highlighting,
//! numeric right-alignment, BigInt as lossless string.

use duckdb::arrow::array::{Array, Float64Array, Int32Array, Int64Array, StringArray, UInt64Array};
use duckdb::arrow::datatypes::DataType;
use duckdb::arrow::record_batch::RecordBatch;
use gpui::SharedString;

/// Render a single cell. Returns `(display, alignment, is_null)`.
/// Caller (Table delegate) wraps in the gpui-component cell widget.
pub fn render_cell(batch: &RecordBatch, col: usize, row: usize) -> CellDisplay {
    let array = batch.column(col);
    if array.is_null(row) {
        return CellDisplay::null();
    }
    match array.data_type() {
        DataType::Int32 => {
            let v = array
                .as_any()
                .downcast_ref::<Int32Array>()
                .expect("Int32Array")
                .value(row);
            CellDisplay::numeric(v.to_string())
        }
        DataType::Int64 => {
            let v = array
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("Int64Array")
                .value(row);
            CellDisplay::big_int(v.to_string())
        }
        DataType::UInt64 => {
            let v = array
                .as_any()
                .downcast_ref::<UInt64Array>()
                .expect("UInt64Array")
                .value(row);
            CellDisplay::big_int(v.to_string())
        }
        DataType::Float64 => {
            let v = array
                .as_any()
                .downcast_ref::<Float64Array>()
                .expect("Float64Array")
                .value(row);
            CellDisplay::numeric(format!("{:.6}", v))
        }
        DataType::Utf8 => {
            let v = array
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("StringArray")
                .value(row);
            CellDisplay::text(v.to_string())
        }
        other => CellDisplay::text(format!("({:?})", other)),
    }
}

/// Column-type badge for the header (e.g., "INT64", "TEXT", "FLOAT64").
pub fn type_badge(data_type: &DataType) -> SharedString {
    match data_type {
        DataType::Int32 => "INT32".into(),
        DataType::Int64 => "INT64".into(),
        DataType::UInt64 => "UINT64".into(),
        DataType::Float64 => "FLOAT64".into(),
        DataType::Utf8 => "TEXT".into(),
        other => format!("{:?}", other).into(),
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
