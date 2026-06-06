//! Column/table profiling (P6a). Built on DuckDB `SUMMARIZE` (one scan, all
//! columns) per design D2. Distinct% is approximate (HLL `approx_unique`).

use duckdb::Connection;
use duckdb::arrow::array::{Array, Decimal128Array, Float64Array, Int64Array, StringArray};

use crate::{EngineError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct NumericStats {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub std: f64,
    pub q25: f64,
    pub median: f64,
    pub q75: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LengthStats {
    pub min: u64,
    pub max: u64,
    pub avg: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnProfile {
    pub name: String,
    pub ty: String,
    pub null_pct: f64,
    pub approx_distinct: u64,
    pub count: u64,
    pub numeric: Option<NumericStats>,
    pub length: Option<LengthStats>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableProfile {
    pub rows: u64,
    pub columns: Vec<ColumnProfile>,
}

/// Helper: parse one cell as f64 across the numeric shapes SUMMARIZE emits.
/// Verified against duckdb-rs 1.4.4 (T0 spike + probe): `min`/`max`/`avg`/`std`/
/// quartiles arrive as VARCHAR (Utf8); `approx_unique`/`count` as Int64;
/// `null_percentage` as DECIMAL(9,2) (Arrow `Decimal128`). For decimals we use
/// `value_as_string`, which applies the column scale (e.g. `25.00`) so the
/// parse is scale-correct without manual `10^scale` math.
fn cell_f64(col: &dyn Array, row: usize) -> Option<f64> {
    if col.is_null(row) {
        return None;
    }
    if let Some(a) = col.as_any().downcast_ref::<Float64Array>() {
        return Some(a.value(row));
    }
    if let Some(a) = col.as_any().downcast_ref::<Int64Array>() {
        return Some(a.value(row) as f64);
    }
    if let Some(a) = col.as_any().downcast_ref::<Decimal128Array>() {
        return a.value_as_string(row).parse().ok();
    }
    if let Some(a) = col.as_any().downcast_ref::<StringArray>() {
        return a.value(row).parse().ok();
    }
    None
}

fn cell_str(col: &dyn Array, row: usize) -> Option<String> {
    if col.is_null(row) {
        return None;
    }
    col.as_any()
        .downcast_ref::<StringArray>()
        .map(|a| a.value(row).to_string())
}

/// Run SUMMARIZE over `target` (already-quoted table name OR a `(SELECT …)` subquery)
/// and map to a TableProfile. Numeric stats are populated when avg parses as f64.
pub(crate) fn profile_blocking(conn: &Connection, target: &str) -> Result<TableProfile> {
    let sql = format!("SUMMARIZE {target}");
    let mut stmt = conn.prepare(&sql).map_err(EngineError::DuckDb)?;
    // Column index by name (robust to ordering changes; verified in T0).
    let rb_iter = stmt.query_arrow([]).map_err(EngineError::DuckDb)?;
    let mut columns = Vec::new();
    let mut rows_total: u64 = 0;
    for batch in rb_iter {
        let idx = |name: &str| -> usize {
            batch
                .schema()
                .fields()
                .iter()
                .position(|f| f.name() == name)
                .unwrap_or_else(|| panic!("SUMMARIZE missing column `{name}`"))
        };
        let c_name = batch.column(idx("column_name"));
        let c_type = batch.column(idx("column_type"));
        let c_min = batch.column(idx("min"));
        let c_max = batch.column(idx("max"));
        let c_uniq = batch.column(idx("approx_unique"));
        let c_avg = batch.column(idx("avg"));
        let c_std = batch.column(idx("std"));
        let c_q25 = batch.column(idx("q25"));
        let c_q50 = batch.column(idx("q50"));
        let c_q75 = batch.column(idx("q75"));
        let c_count = batch.column(idx("count"));
        let c_null = batch.column(idx("null_percentage"));
        for row in 0..batch.num_rows() {
            let count = cell_f64(c_count, row).unwrap_or(0.0) as u64;
            rows_total = rows_total.max(count); // upper bound across cols; exact total via count(*) below
            let avg = cell_f64(c_avg, row);
            let numeric = avg.map(|avg| NumericStats {
                min: cell_f64(c_min, row).unwrap_or(0.0),
                max: cell_f64(c_max, row).unwrap_or(0.0),
                avg,
                std: cell_f64(c_std, row).unwrap_or(0.0),
                q25: cell_f64(c_q25, row).unwrap_or(0.0),
                median: cell_f64(c_q50, row).unwrap_or(0.0),
                q75: cell_f64(c_q75, row).unwrap_or(0.0),
            });
            columns.push(ColumnProfile {
                name: cell_str(c_name, row).unwrap_or_default(),
                ty: cell_str(c_type, row).unwrap_or_default(),
                null_pct: cell_f64(c_null, row).unwrap_or(0.0),
                approx_distinct: cell_f64(c_uniq, row).unwrap_or(0.0) as u64,
                count,
                numeric,
                length: None, // lazy (Task 3)
            });
        }
    }
    // Total row count = exact via count(*).
    let rows: u64 = conn
        .query_row(
            &format!("SELECT count(*)::BIGINT FROM {target}"),
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as u64)
        .unwrap_or(rows_total);
    Ok(TableProfile { rows, columns })
}
