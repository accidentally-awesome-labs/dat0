//! Pure-ish: map an engine QueryResult into typed, row-aligned plot columns.
//! Reuses the downcast strategy from `dat0-engine` profile.rs (Float64/Int64/
//! Decimal128/Utf8 + null). Numeric nulls become f64::NAN; text nulls become "".

use dat0_engine::types::QueryResult;
use duckdb::arrow::array::{
    Array, Decimal128Array, Float32Array, Float64Array, Int8Array, Int16Array, Int32Array,
    Int64Array, StringArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};

#[derive(Clone)]
pub struct PlotColumn {
    pub name: String,
    pub num: Option<Vec<f64>>,
    pub text: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct PlotTable {
    pub columns: Vec<PlotColumn>,
    pub rows: usize,
}

fn col_f64(col: &dyn Array, row: usize) -> f64 {
    if col.is_null(row) {
        return f64::NAN;
    }
    let any = col.as_any();
    if let Some(a) = any.downcast_ref::<Float64Array>() {
        return a.value(row);
    }
    if let Some(a) = any.downcast_ref::<Float32Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<Int64Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<Int32Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<Int16Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<Int8Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<UInt64Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<UInt32Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<UInt16Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<UInt8Array>() {
        return a.value(row) as f64;
    }
    if let Some(a) = any.downcast_ref::<Decimal128Array>() {
        return a.value_as_string(row).parse().unwrap_or(f64::NAN);
    }
    f64::NAN
}

fn col_str(col: &dyn Array, row: usize) -> String {
    if col.is_null(row) {
        return String::new();
    }
    if let Some(a) = col.as_any().downcast_ref::<StringArray>() {
        return a.value(row).to_string();
    }
    // Defensive numeric-as-label fallback; unreachable in normal use because
    // `is_numeric_array` gates kind-detection and routes numeric columns through col_f64.
    let f = col_f64(col, row);
    if f.is_nan() {
        String::new()
    } else {
        f.to_string()
    }
}

fn is_numeric_array(col: &dyn Array) -> bool {
    let any = col.as_any();
    any.is::<Float64Array>()
        || any.is::<Float32Array>()
        || any.is::<Int64Array>()
        || any.is::<Int32Array>()
        || any.is::<Int16Array>()
        || any.is::<Int8Array>()
        || any.is::<UInt64Array>()
        || any.is::<UInt32Array>()
        || any.is::<UInt16Array>()
        || any.is::<UInt8Array>()
        || any.is::<Decimal128Array>()
}

impl PlotTable {
    pub fn from_query_result(qr: &QueryResult) -> Self {
        let rows: usize = qr.batches.iter().map(|b| b.num_rows()).sum();
        let ncols = qr.columns.len();
        let mut columns: Vec<PlotColumn> = qr
            .columns
            .iter()
            .map(|c| PlotColumn {
                name: c.name.clone(),
                num: None,
                text: None,
            })
            .collect();

        for (ci, pcol) in columns.iter_mut().enumerate().take(ncols) {
            // Determine kind from the first non-empty batch's column.
            let numeric = qr
                .batches
                .iter()
                .find(|b| b.num_rows() > 0)
                .map(|b| is_numeric_array(b.column(ci).as_ref()))
                .unwrap_or(true);
            if numeric {
                let mut v = Vec::with_capacity(rows);
                for b in &qr.batches {
                    let col = b.column(ci);
                    for r in 0..b.num_rows() {
                        v.push(col_f64(col.as_ref(), r));
                    }
                }
                pcol.num = Some(v);
            } else {
                let mut v = Vec::with_capacity(rows);
                for b in &qr.batches {
                    let col = b.column(ci);
                    for r in 0..b.num_rows() {
                        v.push(col_str(col.as_ref(), r));
                    }
                }
                pcol.text = Some(v);
            }
        }
        PlotTable { columns, rows }
    }

    pub fn num(&self, name: &str) -> Option<&[f64]> {
        self.columns
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.num.as_deref())
    }
    pub fn text(&self, name: &str) -> Option<&[String]> {
        self.columns
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.text.as_deref())
    }
    /// Positional accessors — render.rs reads by the per-type column CONTRACT
    /// (see charts/query.rs), which is independent of user column names.
    pub fn num_at(&self, i: usize) -> Option<&[f64]> {
        self.columns.get(i).and_then(|c| c.num.as_deref())
    }
    pub fn text_at(&self, i: usize) -> Option<&[String]> {
        self.columns.get(i).and_then(|c| c.text.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

    fn budget() -> MemoryBudget {
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        }
    }

    #[tokio::test]
    async fn extracts_numeric_and_text_columns() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = DuckDBEngine::new(tmp.path().join("d.duckdb"), budget()).unwrap();
        engine.init().await.unwrap();
        engine
            .create_table(
                "t",
                "SELECT * FROM (VALUES ('West', 10.0), ('East', 20.0), ('West', 5.0)) v(region, amt)",
                DerivedOrigin::Sql("seed".into()),
            )
            .await
            .unwrap();
        let qr = engine
            .execute("SELECT region AS k, SUM(amt) AS v FROM t GROUP BY region ORDER BY k")
            .await
            .unwrap();
        let pt = PlotTable::from_query_result(&qr);
        assert_eq!(pt.rows, 2);
        assert_eq!(
            pt.text("k").unwrap(),
            &["East".to_string(), "West".to_string()]
        );
        assert_eq!(pt.num("v").unwrap(), &[20.0, 15.0]);
        engine.close().await.unwrap();
    }

    // Proves the Scatter/Histogram `USING SAMPLE`-after-WHERE SQL from query.rs is
    // valid DuckDB at runtime (build_plot_sql emits it; Task 2 flagged it for live check).
    #[tokio::test]
    async fn sampled_plot_sql_is_valid_against_engine() {
        use crate::charts::query::build_plot_sql;
        use crate::charts::spec::{ChartSpec, ChartType};
        let tmp = tempfile::tempdir().unwrap();
        let engine = DuckDBEngine::new(tmp.path().join("s.duckdb"), budget()).unwrap();
        engine.init().await.unwrap();
        engine
            .create_table(
                "pts",
                "SELECT * FROM (VALUES (1.0, 2.0), (3.0, 4.0), (5.0, 6.0)) v(a, b)",
                DerivedOrigin::Sql("seed".into()),
            )
            .await
            .unwrap();
        let mk = |t, x: &str, y: Option<&str>| ChartSpec {
            chart_type: t,
            source: "\"pts\"".into(),
            x: Some(x.into()),
            y: y.map(str::to_string),
            group: None,
            color: None,
            title: String::new(),
        };
        let scatter_sql = build_plot_sql(&mk(ChartType::Scatter, "a", Some("b"))).unwrap();
        let qr = engine
            .execute(&scatter_sql)
            .await
            .expect("scatter USING SAMPLE must be valid DuckDB");
        assert!(PlotTable::from_query_result(&qr).rows <= 3);
        let hist_sql = build_plot_sql(&mk(ChartType::Histogram, "a", None)).unwrap();
        engine
            .execute(&hist_sql)
            .await
            .expect("histogram USING SAMPLE must be valid DuckDB");
        engine.close().await.unwrap();
    }

    #[tokio::test]
    async fn handles_integer_widths_and_nulls() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = DuckDBEngine::new(tmp.path().join("i.duckdb"), budget()).unwrap();
        engine.init().await.unwrap();
        // x is INTEGER (Int32), y is DOUBLE with a NULL middle value.
        engine
            .create_table(
                "ints",
                "SELECT * FROM (VALUES (1::INTEGER, 10.0), (2::INTEGER, NULL), (3::INTEGER, 30.0)) v(x, y)",
                DerivedOrigin::Sql("seed".into()),
            )
            .await
            .unwrap();
        let qr = engine
            .execute("SELECT x, y FROM ints ORDER BY x")
            .await
            .unwrap();
        let pt = PlotTable::from_query_result(&qr);
        // INTEGER (Int32) column maps to real f64 values, NOT NaN.
        assert_eq!(pt.num("x").unwrap(), &[1.0, 2.0, 3.0]);
        // NULL numeric cell becomes NaN; non-null neighbors intact.
        let y = pt.num("y").unwrap();
        assert_eq!(y[0], 10.0);
        assert!(y[1].is_nan());
        assert_eq!(y[2], 30.0);
        engine.close().await.unwrap();
    }
}
