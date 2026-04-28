#![allow(deprecated)]

use std::path::PathBuf;

use dat0_engine::extension_bootstrap::__test_install_sqlite_scanner;
use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

#[tokio::test]
async fn attach_sqlite_exposes_tables() {
    __test_install_sqlite_scanner().expect("ext install");

    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let dsn = format!("sqlite:{}", fixture("simple.sqlite").display());
    engine
        .attach(
            &dsn,
            "sq",
            AttachOpts {
                read_only: true,
                schema_filter: None,
            },
        )
        .await
        .unwrap();

    let v = engine
        .__debug_query_scalar("SELECT COUNT(*)::TEXT FROM sq.items")
        .await
        .unwrap();
    assert_eq!(v, "3");

    engine.detach("sq").await.unwrap();
    engine.close().await.unwrap();
}
