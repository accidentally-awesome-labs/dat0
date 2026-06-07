//! Behavioral: referenced_tables() resolves the base tables a SQL depends on.
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 128 * 1024 * 1024,
    }
}

async fn engine() -> (tempfile::TempDir, DuckDBEngine) {
    let tmp = tempfile::tempdir().unwrap();
    let e = DuckDBEngine::new(tmp.path().join("s.duckdb"), budget()).unwrap();
    e.init().await.unwrap();
    for t in ["sales", "customers"] {
        e.create_table(t, "SELECT 1 AS id", DerivedOrigin::Sql("seed".into()))
            .await
            .unwrap();
    }
    (tmp, e)
}

#[tokio::test]
async fn join_returns_both_base_tables() {
    let (_tmp, e) = engine().await;
    let mut got = e
        .referenced_tables("SELECT s.id FROM sales s JOIN customers c ON s.id=c.id")
        .await
        .unwrap();
    got.sort();
    assert_eq!(got, vec!["customers".to_string(), "sales".to_string()]);
}

#[tokio::test]
async fn cte_name_is_excluded_but_its_sources_are_kept() {
    let (_tmp, e) = engine().await;
    let got = e
        .referenced_tables("WITH c AS (SELECT * FROM customers) SELECT * FROM c")
        .await
        .unwrap();
    // `c` is a CTE, not a base table; `customers` (its source) is kept.
    assert_eq!(got, vec!["customers".to_string()]);
}

#[tokio::test]
async fn subquery_tables_are_included() {
    let (_tmp, e) = engine().await;
    let got = e
        .referenced_tables("SELECT id FROM sales WHERE id IN (SELECT id FROM customers)")
        .await
        .unwrap();
    assert!(got.contains(&"sales".to_string()) && got.contains(&"customers".to_string()));
}
