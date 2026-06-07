//! P6b lineage: extract the base-table names a derived table's SQL depends on,
//! via DuckDB's `json_serialize_sql`. CTE-defined names are excluded (they are
//! not real tables); their underlying sources are still collected. Table
//! functions (read_csv/read_parquet/…) are NOT BASE_TABLE nodes, so file-import
//! views never appear here — those edges come from `TableOrigin::File`.
use crate::Result;
use crate::error::EngineError;
use std::collections::HashSet;

/// Base-table names referenced by `sql`, de-duplicated, in first-seen order.
pub(crate) fn referenced_tables_blocking(
    conn: &duckdb::Connection,
    sql: &str,
) -> Result<Vec<String>> {
    // The `::VARCHAR` cast is required: `json_serialize_sql` rejects an
    // untyped bound parameter ("first argument must be a VARCHAR"). Binding
    // (rather than inlining) keeps the SQL text out of the query string.
    let json: String =
        conn.query_row("SELECT json_serialize_sql(?::VARCHAR)", [sql], |r| r.get(0))?;
    let v: serde_json::Value = serde_json::from_str(&json)
        // json_serialize_sql output is DuckDB-produced and effectively always
        // valid JSON; treat the impossible parse failure as bad input data.
        .map_err(|e| EngineError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    let mut ctes = HashSet::new();
    collect_cte_names(&v, &mut ctes);

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    collect_base_tables(&v, &ctes, &mut out, &mut seen);
    Ok(out)
}

/// Collect every name defined by a CTE so it can be excluded from base tables.
/// Verified shape (T0 spike): DuckDB nests CTE definitions under a `cte_map`
/// object as `{ "map": [ { "key": "<name>", ... }, ... ] }`; the defined name is
/// the `key` string. This walks for any object keyed `cte_map` and harvests its
/// entry names.
fn collect_cte_names(v: &serde_json::Value, out: &mut HashSet<String>) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(cte) = map.get("cte_map") {
                if let Some(entries) = cte.get("map").and_then(|m| m.as_array()) {
                    for e in entries {
                        if let Some(k) = e.get("key").and_then(|k| k.as_str()) {
                            out.insert(k.to_string());
                        }
                    }
                }
            }
            for val in map.values() {
                collect_cte_names(val, out);
            }
        }
        serde_json::Value::Array(arr) => arr.iter().for_each(|x| collect_cte_names(x, out)),
        _ => {}
    }
}

fn collect_base_tables(
    v: &serde_json::Value,
    ctes: &HashSet<String>,
    out: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    match v {
        serde_json::Value::Object(map) => {
            let is_base = map.get("type").and_then(|t| t.as_str()) == Some("BASE_TABLE");
            if is_base {
                if let Some(name) = map.get("table_name").and_then(|n| n.as_str()) {
                    if !ctes.contains(name) && seen.insert(name.to_string()) {
                        out.push(name.to_string());
                    }
                }
            }
            for val in map.values() {
                collect_base_tables(val, ctes, out, seen);
            }
        }
        serde_json::Value::Array(arr) => arr
            .iter()
            .for_each(|x| collect_base_tables(x, ctes, out, seen)),
        _ => {}
    }
}
