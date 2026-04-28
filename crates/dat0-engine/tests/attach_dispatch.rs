use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn attach_unknown_scheme_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine
        .attach("redis://localhost:6379", "x", AttachOpts::default())
        .await
        .expect_err("unknown scheme");
    assert!(matches!(
        err,
        dat0_engine::EngineError::UnknownAttachScheme(_)
    ));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn attach_md_returns_not_implemented() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine
        .attach("md:my_db", "md", AttachOpts::default())
        .await
        .expect_err("md: deferred to P5 (D-007)");
    match err {
        dat0_engine::EngineError::NotImplemented { feature } => {
            assert_eq!(feature, "MotherDuck");
        }
        other => panic!("expected NotImplemented, got {other:?}"),
    }
    engine.close().await.unwrap();
}

#[tokio::test]
async fn detach_unknown_alias_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine.detach("never_attached").await.expect_err("");
    assert!(matches!(err, dat0_engine::EngineError::DuckDb(_)));
    engine.close().await.unwrap();
}
