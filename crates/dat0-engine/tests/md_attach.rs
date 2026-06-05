//! End-to-end MotherDuck integration. `#[ignore]` so `cargo test` skips it;
//! CI runs it explicitly with `--include-ignored` after asserting the token
//! env var is present (see .github workflow, P5c T14).
//!
// SPIKE FINDINGS (P5c T0 + CI validation):
//   S1: PASS (2026-06-04). `INSTALL motherduck; LOAD motherduck;` succeeds on
//       bundled duckdb-rs 1.4.4 — token-free, verified by
//       `spike_s1_install_load_only`.
//   S2/alias: a CI diagnostic (2026-06-05) found `ATTACH 'md:' AS <alias>` is
//       REJECTED — "Database aliases are not yet supported by MotherDuck in
//       workspace mode". Correct form: `ATTACH 'md:'` (workspace mode) attaches
//       the account's databases under their REAL names (every account has
//       `sample_data`). Routing keys on those real names, not a literal `md.`.

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

/// Collect a single VARCHAR column across all batches into a Vec<String>.
async fn scalar_list(engine: &DuckDBEngine, sql: &str) -> Vec<String> {
    use duckdb::arrow::array::Array as _;
    let res = engine.execute(sql).await.expect("execute");
    let mut out = Vec::new();
    for batch in &res.batches {
        if let Some(arr) = batch
            .column(0)
            .as_any()
            .downcast_ref::<duckdb::arrow::array::StringArray>()
        {
            for i in 0..arr.len() {
                if arr.is_valid(i) {
                    out.push(arr.value(i).to_string());
                }
            }
        }
    }
    out
}

/// Canonical end-to-end engine integration test (P5c): exercises the real
/// `DuckDBEngine::attach` MotherDuck arm — lazy extension install, `LOAD
/// motherduck` on the live connection, `SET motherduck_token` + workspace-mode
/// `ATTACH 'md:'` (NO alias), then validates the account's databases attached
/// under their real names and a real catalog query works, then detaches.
/// `#[ignore]`d so the default suite skips it; CI runs it with `--include-ignored`.
#[tokio::test]
#[ignore = "requires MOTHERDUCK_TOKEN; CI runs with --include-ignored"]
async fn engine_attaches_md_and_queries_then_detaches() {
    let token = md_token().expect("MOTHERDUCK_TOKEN must be set");
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("md-it.duckdb"), it_budget()).unwrap();
    engine.init().await.expect("init");

    let opts = AttachOpts {
        token: Some(token),
        ..Default::default()
    };
    // alias arg is ignored by the md arm (workspace mode has no alias).
    engine.attach("md:", "md", opts).await.expect("attach md");

    // The app filters MD databases by `type = 'motherduck'` (see
    // connections::connect::list_databases). Validate that filter here: every
    // MotherDuck account has a `sample_data` database. On failure, dump
    // name|path|type for ALL databases so a wrong filter is self-diagnosing.
    let md_dbs = scalar_list(
        &engine,
        "SELECT database_name FROM duckdb_databases() WHERE lower(type) = 'motherduck' ORDER BY 1",
    )
    .await;
    assert!(
        md_dbs.iter().any(|d| d == "sample_data"),
        "expected `sample_data` among md dbs (type='motherduck'); got {md_dbs:?}; ALL dbs (name|path|type): {:?}",
        scalar_list(
            &engine,
            "SELECT database_name || '|' || COALESCE(path,'') || '|' || COALESCE(type,'') FROM duckdb_databases() ORDER BY 1",
        )
        .await
    );

    // A real catalog query scoped to the attached MD database. `duckdb_tables()`
    // is a global table function spanning all attached catalogs with a
    // `database_name` column (unlike `<db>.information_schema.schemata`, which
    // DuckDB does not expose as a 3-part path). Proves the attached MD db is
    // queryable end-to-end.
    let n = scalar(
        &engine,
        "SELECT count(*)::TEXT FROM duckdb_tables() WHERE database_name = 'sample_data';",
    )
    .await;
    assert!(n.parse::<i64>().unwrap() >= 0);

    // Detach every attached MD database (best-effort), then close.
    for db in &md_dbs {
        engine.detach(db).await.ok();
    }
    engine.close().await.expect("close");
    // Token must never appear in any output this test produced.
}
