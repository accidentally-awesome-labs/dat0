//! End-to-end MotherDuck integration. `#[ignore]` so `cargo test` skips it;
//! CI runs it explicitly with `--include-ignored` after asserting the token
//! env var is present (see .github workflow, P5c T14).
//!
// SPIKE FINDINGS (P5c T0):
//   S1: PASS (2026-06-04). `INSTALL motherduck; LOAD motherduck;` succeeds on
//       bundled duckdb-rs 1.4.4 — token-free, verified by
//       `spike_s1_install_load_only`. Gate GREEN → slice proceeds to T1.
//   S2: PENDING (needs MOTHERDUCK_TOKEN). Until verified, T9 ships the
//       documented `md.` string-heuristic fallback (the plan default); if the
//       full spike below shows `duckdb_databases()` is a reliable per-query
//       catalog-touch signal, T9 may switch to plan-inspection.

/// Reads the MotherDuck token from the environment. DuckDB itself also honours
/// the `motherduck_token` env var, but we read it explicitly so the test can
/// `SET motherduck_token` on the connection and skip-document when absent.
fn md_token() -> Option<String> {
    std::env::var("MOTHERDUCK_TOKEN")
        .or_else(|_| std::env::var("motherduck_token"))
        .ok()
}

/// S1 gate (token-free): does the `motherduck` extension install + load at all
/// on bundled duckdb-rs 1.4.4? This is the true go/no-go for the whole slice —
/// it requires no credentials (INSTALL/LOAD only downloads + links the ext).
#[test]
#[ignore = "network: downloads the motherduck extension; run with --ignored"]
fn spike_s1_install_load_only() {
    let scratch = std::env::temp_dir().join(format!("dat0-md-s1-{}.duckdb", std::process::id()));
    let conn = duckdb::Connection::open(&scratch).expect("open scratch");
    conn.execute_batch("INSTALL motherduck; LOAD motherduck;")
        .expect("INSTALL/LOAD motherduck — S1 gate");
    eprintln!("S1 OK: motherduck extension installed + loaded on duckdb-rs 1.4.4");
}

use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine};

fn it_budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

/// Extract first-column/first-row as String (cast the column to ::TEXT in SQL).
async fn scalar(engine: &DuckDBEngine, sql: &str) -> String {
    let res = engine.execute(sql).await.expect("execute");
    let batch = res.batches.first().expect("at least one batch");
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<duckdb::arrow::array::StringArray>()
        .expect("StringArray");
    arr.value(0).to_string()
}

/// Canonical end-to-end engine integration test (P5c T3): exercises the real
/// `DuckDBEngine::attach` MotherDuck arm — lazy extension install, `LOAD
/// motherduck` on the live connection, `SET motherduck_token` + `ATTACH 'md:'`,
/// a catalog query against the attached `md` database, then `detach`/`close`.
/// `#[ignore]`d so the default suite skips it; CI runs it with `--include-ignored`
/// after asserting `MOTHERDUCK_TOKEN` is present.
#[tokio::test]
#[ignore = "requires MOTHERDUCK_TOKEN; CI runs with --include-ignored"]
async fn engine_attaches_md_and_queries_then_detaches() {
    let token = md_token().expect("MOTHERDUCK_TOKEN must be set");
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("md-it.duckdb"), it_budget()).unwrap();
    engine.init().await.expect("init");

    let opts = AttachOpts {
        token: Some(token.clone()),
        ..Default::default()
    };
    engine.attach("md:", "md", opts).await.expect("attach md");

    let n = scalar(
        &engine,
        "SELECT count(*)::TEXT FROM md.information_schema.schemata;",
    )
    .await;
    assert!(n.parse::<i64>().unwrap() >= 0);

    engine.detach("md").await.expect("detach");
    engine.close().await.expect("close");
    // Token must never appear in any output this test produced.
}
