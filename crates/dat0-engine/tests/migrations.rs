#![allow(deprecated)] // __debug_query_scalar is intentionally test-only

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[tokio::test]
async fn migrations_apply_on_fresh_db() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("a.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget()).unwrap();
    engine.init().await.unwrap();

    let v = engine
        .__debug_query_scalar("SELECT COALESCE(MAX(version), 0)::TEXT FROM __dat0_meta_migrations")
        .await
        .unwrap();
    assert_eq!(v, "1", "first migration should be applied");

    let workspace_v = engine
        .__debug_query_scalar("SELECT value FROM __dat0_meta WHERE key = 'dat0_workspace_version'")
        .await
        .unwrap();
    assert_eq!(workspace_v, "1");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn migrations_idempotent_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("a.duckdb");
    {
        let engine = DuckDBEngine::new(scratch.clone(), budget()).unwrap();
        engine.init().await.unwrap();
        engine.close().await.unwrap();
    }
    // Second open: no new rows in __dat0_meta_migrations
    {
        let engine = DuckDBEngine::new(scratch.clone(), budget()).unwrap();
        engine.init().await.unwrap();
        let count = engine
            .__debug_query_scalar("SELECT COUNT(*)::TEXT FROM __dat0_meta_migrations")
            .await
            .unwrap();
        assert_eq!(count, "1");
        engine.close().await.unwrap();
    }
}

#[tokio::test]
async fn failed_migration_rolls_back() {
    use dat0_engine::migrations::{Migration, apply_migrations};
    fn boom(_: &duckdb::Connection) -> std::result::Result<(), duckdb::Error> {
        Err(duckdb::Error::ToSqlConversionFailure(
            "intentional test failure".into(),
        ))
    }
    let migrations = &[
        Migration {
            version: 1,
            name: "init",
            up: dat0_engine::migrations::__test_only_m001_init,
        },
        Migration {
            version: 2,
            name: "boom",
            up: boom,
        },
    ];

    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("a.duckdb");
    let conn = duckdb::Connection::open(&scratch).unwrap();

    let err = apply_migrations(&conn, migrations).expect_err("must fail at v2");
    let dat0_engine::EngineError::Migration { version, .. } = err else {
        panic!("expected Migration error, got {err:?}");
    };
    assert_eq!(version, 2);

    // v1 should remain applied; v2 row not present
    let count: u32 = conn
        .query_row("SELECT COUNT(*) FROM __dat0_meta_migrations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1, "v1 stays applied; v2 rolled back");
}
