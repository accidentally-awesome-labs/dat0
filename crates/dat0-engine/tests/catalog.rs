use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn create_describe_drop_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let info = engine
        .create_table(
            "things",
            "SELECT 1::INTEGER AS id, 'a'::VARCHAR AS name UNION ALL SELECT 2, 'b'",
            DerivedOrigin::Sql("test".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(info.name, "things");
    assert_eq!(info.columns.len(), 2);

    let cols = engine.describe_table("things", None).await.unwrap();
    assert_eq!(cols.len(), 2);

    let tables = engine.get_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "things"));

    engine.rename_table("things", "stuff", None).await.unwrap();
    let cols2 = engine.describe_table("stuff", None).await.unwrap();
    assert_eq!(cols2.len(), 2);

    engine.drop_table("stuff", None).await.unwrap();
    let err = engine
        .describe_table("stuff", None)
        .await
        .expect_err("dropped");
    assert!(matches!(err, dat0_engine::EngineError::DuckDb(_)));

    engine.close().await.unwrap();
}

#[tokio::test]
async fn create_table_with_embedded_quote_in_name() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    // Hostile name: contains a literal `"`. Without quote_ident escaping,
    // this would close the identifier mid-format and inject SQL.
    let evil_name = r#"weird"name"#;
    let info = engine
        .create_table(
            evil_name,
            "SELECT 1::INTEGER AS id",
            DerivedOrigin::Sql("test".into()),
        )
        .await
        .unwrap();
    assert_eq!(info.name, evil_name);

    let cols = engine.describe_table(evil_name, None).await.unwrap();
    assert_eq!(cols.len(), 1);

    engine.drop_table(evil_name, None).await.unwrap();

    engine.close().await.unwrap();
}
