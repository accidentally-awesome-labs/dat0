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

/// P5c T3: a token-less `md:` ATTACH no longer returns `NotImplemented`
/// (D-007 closed) — it must fail fast with `MotherDuckAuth` BEFORE any
/// network/extension work, since the arm requires `opts.token`.
#[tokio::test]
async fn attach_md_without_token_is_auth_error() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine
        .attach("md:", "md", AttachOpts::default())
        .await
        .expect_err("missing token");
    assert!(matches!(err, dat0_engine::EngineError::MotherDuckAuth));
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
