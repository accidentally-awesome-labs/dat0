//! Exit-criterion tests gated on `tests/fixtures/large/`. Run with
//! `cargo test -- --include-ignored` after `dat0-fixtures` has populated
//! the directory.

#![allow(deprecated)]

use std::path::PathBuf;

use dat0_engine::extension_bootstrap::__test_install_sqlite_scanner;
use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use futures::StreamExt;

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 4 * 1024 * 1024 * 1024,
    } // 4 GB
}
fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/large")
}

fn skip_if_no_fixtures() -> bool {
    let p = fixtures_root().join("generated.csv");
    if !p.exists() {
        eprintln!(
            "SKIP: {} not present; run `cargo run -p dat0-fixtures -- --out tests/fixtures/large` first.",
            p.display()
        );
        return true;
    }
    false
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn one_gb_csv_streams() {
    if skip_if_no_fixtures() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(
            &fixtures_root().join("generated.csv"),
            RegisterOpts::default(),
        )
        .await
        .unwrap();
    assert!(!info.columns.is_empty());

    let mut stream = engine
        .execute_streaming(&format!("SELECT * FROM \"{}\"", info.name))
        .await
        .unwrap();
    let mut total = 0_usize;
    let mut batches = 0_usize;
    while let Some(batch) = stream.next().await {
        let b = batch.unwrap();
        total += b.num_rows();
        batches += 1;
    }
    assert!(total > 1_000_000, "expected millions of rows; got {total}");
    assert!(batches > 1, "expected streamed batches");
    engine.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn five_hundred_mb_parquet_streams() {
    if skip_if_no_fixtures() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(
            &fixtures_root().join("generated.parquet"),
            RegisterOpts::default(),
        )
        .await
        .unwrap();
    let mut stream = engine
        .execute_streaming(&format!("SELECT * FROM \"{}\"", info.name))
        .await
        .unwrap();
    let mut total = 0_usize;
    while let Some(b) = stream.next().await {
        total += b.unwrap().num_rows();
    }
    assert!(total > 1_000_000);
    engine.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn one_hundred_mb_sqlite_attach() {
    if skip_if_no_fixtures() {
        return;
    }
    __test_install_sqlite_scanner().expect("ext install");

    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let dsn = format!(
        "sqlite:{}",
        fixtures_root().join("generated.sqlite").display()
    );
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
    let n: u64 = v.parse().unwrap();
    assert!(
        n > 100_000,
        "expected hundreds of thousands of rows in 100 MB SQLite; got {n}"
    );
    engine.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn streaming_emits_arrow_recordbatch_type_chain() {
    // The streaming exit criterion claims "verified zero-copy from engine to
    // consumer (no JSON serialization in path)". The type-chain assertion
    // here proves the WEAKER property: the consumer receives
    // `duckdb::arrow::record_batch::RecordBatch` directly — no `Value`/`String`/JSON
    // intermediation is possible without a transformation step the type system
    // would surface. Genuine zero-copy verification (peak RSS bounded
    // independently of fixture size, batch buffers shared between DuckDB and
    // the consumer's address space) is deferred to a P3 perf bench because it
    // requires RSS measurement instrumentation we don't have in P2.
    if skip_if_no_fixtures() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(
            &fixtures_root().join("generated.csv"),
            RegisterOpts::default(),
        )
        .await
        .unwrap();
    let mut stream = engine
        .execute_streaming(&format!("SELECT * FROM \"{}\" LIMIT 100", info.name))
        .await
        .unwrap();
    let batch = stream.next().await.unwrap().unwrap();
    // Type assertion: if this compiles, the chain is RecordBatch through and
    // through. No JSON path possible without an explicit transform step.
    let _: &duckdb::arrow::record_batch::RecordBatch = &batch;
    assert!(batch.num_rows() > 0);
    engine.close().await.unwrap();
}
