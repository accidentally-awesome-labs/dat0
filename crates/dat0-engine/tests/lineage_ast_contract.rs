//! T0 spike (P6b): pin the json_serialize_sql AST shape we walk in lineage.rs.
//! Mirrors summarize_contract.rs. If duckdb-rs changes the shape, fix the
//! extractor in src/lineage.rs and update the asserts here.
//!
//! VERIFIED SHAPE (duckdb-rs 1.4.4 / DuckDB 1.4.x) — drives the T1 walker:
//!
//!   Top level: `{"error":false,"statements":[ { "node": <SELECT_NODE> } ]}`
//!
//!   A referenced base table is a node:
//!     {"type":"BASE_TABLE","table_name":"<name>","schema_name":"<s>",
//!      "catalog_name":"<c>","alias":"<a>", ...}
//!   => the base-table NAME lives at the JSON key `table_name` on any object
//!      whose `type` == "BASE_TABLE". (schema_name / catalog_name are ""
//!      when unqualified; the walker should also read those for qualified refs.)
//!
//!   CTE definitions live under each SELECT_NODE's `cte_map`:
//!     "cte_map":{"map":[ {"key":"<cte_name>","value":{...}} , ... ]}
//!   => a CTE's DEFINED NAME lives at `cte_map.map[].key`. Walk every
//!      `cte_map.map` array in the tree (nested SELECT_NODEs each carry their
//!      own `cte_map`, empty as `{"map":[]}`) and collect the `key` strings.
//!
//!   IMPORTANT for the walker: a *reference* to a CTE (e.g. `JOIN c`) ALSO
//!      appears as a BASE_TABLE node with `table_name:"c"`. So real-table
//!      lineage = { all BASE_TABLE.table_name } MINUS { all cte_map.map[].key }.
//!      (`collect_cte_names` gathers the keys; `collect_base_tables` gathers the
//!      table_names; subtract.)
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 128 * 1024 * 1024,
    }
}

#[tokio::test]
async fn json_serialize_sql_exposes_base_tables_and_ctes() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(tmp.path().join("s.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    for t in ["sales", "customers"] {
        engine
            .create_table(t, "SELECT 1 AS id", DerivedOrigin::Sql("seed".into()))
            .await
            .unwrap();
    }

    // A query with a JOIN, a CTE, and a subquery — every shape the walker handles.
    let sql = "WITH c AS (SELECT * FROM customers) \
               SELECT s.id FROM sales s JOIN c ON s.id = c.id \
               WHERE s.id IN (SELECT id FROM customers)";
    let json: String = engine
        .execute(&format!("SELECT json_serialize_sql('{sql}') AS ast"))
        .await
        .unwrap()
        .batches
        .first()
        .map(|b| {
            use duckdb::arrow::array::StringArray;
            b.column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .value(0)
                .to_string()
        })
        .unwrap();

    // Dump the real shape so the spike records ground truth.
    println!("[lineage_ast_contract] json_serialize_sql output:\n{json}");

    // Contract pins (adjust to the REAL shape the spike reveals, then keep):
    assert!(
        json.contains("\"type\":\"BASE_TABLE\""),
        "base tables present: {json}"
    );
    assert!(
        json.contains("\"table_name\":\"sales\""),
        "sales ref: {json}"
    );
    assert!(
        json.contains("\"table_name\":\"customers\""),
        "customers ref: {json}"
    );
    assert!(
        json.contains("cte_map"),
        "cte_map present so CTE names are excludable: {json}"
    );

    engine.close().await.unwrap();
}
