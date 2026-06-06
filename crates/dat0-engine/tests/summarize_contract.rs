//! T0 spike (P6a): pin the SUMMARIZE output schema we map in profile.rs.
//!
//! VERIFIED SUMMARIZE columns (duckdb-rs 1.4.4):
//!   column_name, column_type, min, max, approx_unique, avg, std,
//!   q25, q50, q75, count, null_percentage
//!
//! All 12 assumed names matched exactly — no deviations from the plan.
//! column_name is index 0 and arrives as StringArray (Arrow Utf8).

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use duckdb::arrow::array::StringArray;

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 128 * 1024 * 1024,
    }
}

#[tokio::test]
async fn summarize_emits_expected_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(tmp.path().join("s.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    engine
        .create_table(
            "t",
            "SELECT * FROM (VALUES (1, 'a'), (2, 'b'), (NULL::INTEGER, 'c'), (4, 'b')) AS v(num, str)",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();

    // Base-table form (profile_table) and subquery form (profile_query).
    for sql in [
        "SUMMARIZE t",
        "SUMMARIZE (SELECT * FROM t WHERE num IS NOT NULL)",
    ] {
        let res = engine.execute(sql).await.expect("summarize runs");
        let names: Vec<String> = res.columns.iter().map(|c| c.name.clone()).collect();

        // Print the actual column list so the spike can record ground truth.
        println!("[summarize_contract] sql={sql:?}  columns={names:?}");

        // The columns profile.rs depends on MUST all be present.
        for needed in [
            "column_name",
            "column_type",
            "min",
            "max",
            "approx_unique",
            "avg",
            "std",
            "q25",
            "q50",
            "q75",
            "count",
            "null_percentage",
        ] {
            assert!(
                names.contains(&needed.to_string()),
                "SUMMARIZE missing `{needed}`; got {names:?}"
            );
        }

        // column_name is the first projected column and is a string.
        let b = res.batches.first().expect("one batch");
        assert!(
            b.column(0).as_any().downcast_ref::<StringArray>().is_some(),
            "column_name expected StringArray"
        );
    }
    engine.close().await.unwrap();
}
