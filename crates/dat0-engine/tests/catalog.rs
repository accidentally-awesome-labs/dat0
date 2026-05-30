use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, ROWID_COL};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

/// True if `cols` contains the eagerly-injected `__dat0_rowid` surrogate.
/// `create_table` (CTAS) injects it at create time (P4b T3, design §5), so the
/// physical schema reported by `describe_table` carries it (the grid hides it
/// at the UI layer — design §8).
fn has_rowid(cols: &[dat0_engine::ColumnInfo]) -> bool {
    cols.iter().any(|c| c.name == ROWID_COL)
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
    // 2 user columns (id, name) + the eagerly-injected __dat0_rowid surrogate.
    assert_eq!(info.columns.len(), 3);
    assert!(has_rowid(&info.columns), "create_table must inject surrogate");

    let cols = engine.describe_table("things", None).await.unwrap();
    assert_eq!(cols.len(), 3);
    assert!(has_rowid(&cols));

    let tables = engine.get_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "things"));

    engine.rename_table("things", "stuff", None).await.unwrap();
    let cols2 = engine.describe_table("stuff", None).await.unwrap();
    assert_eq!(cols2.len(), 3);
    assert!(has_rowid(&cols2));

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

    // 1 user column (id) + the surrogate. This also proves ensure_rowid's
    // ALTER/UPDATE SQL correctly quotes a table name containing a literal `"`.
    let cols = engine.describe_table(evil_name, None).await.unwrap();
    assert_eq!(cols.len(), 2);
    assert!(has_rowid(&cols));

    engine.drop_table(evil_name, None).await.unwrap();

    engine.close().await.unwrap();
}
