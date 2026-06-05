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

/// TEMPORARY DIAGNOSTIC (P5c CI debug): the engine arm maps every ATTACH
/// failure to `MotherDuckAuth` and DROPS the real DuckDB error (token-safety),
/// so CI only shows "MotherDuckAuth" with no cause. This probe runs the raw
/// attach sequence three ways and reports the REAL (token-redacted) error +
/// whether the token has surrounding whitespace. It `panic!`s with the summary
/// so the output is visible without `--nocapture`. REMOVE once the root cause
/// is fixed. Run locally: `MOTHERDUCK_TOKEN=<tok> cargo test -p dat0-engine
/// --test md_attach diag_md_attach_strategies -- --ignored`.
#[test]
#[ignore = "diagnostic; requires MOTHERDUCK_TOKEN"]
fn diag_md_attach_strategies() {
    let token = md_token().expect("MOTHERDUCK_TOKEN must be set");
    let redact = |s: String| -> String {
        s.replace(token.as_str(), "<TOK>")
            .replace(token.trim(), "<TOK>")
    };
    let mut out = String::new();
    out.push_str(&format!(
        "token.len()={} trimmed.len()={} surrounding_ws={} starts_with_eyJ={}\n",
        token.len(),
        token.trim().len(),
        token.len() != token.trim().len(),
        token.trim().starts_with("eyJ"), // JWTs start with eyJ; reveals format, not value
    ));

    let try_seq = |label: &str, stmts: &[String]| -> String {
        let p =
            std::env::temp_dir().join(format!("dat0-diag-{}-{}.duckdb", label, std::process::id()));
        let conn = match duckdb::Connection::open(&p) {
            Ok(c) => c,
            Err(e) => return format!("{label}: open ERR {e}\n"),
        };
        let mut line = String::new();
        for (i, s) in stmts.iter().enumerate() {
            match conn.execute_batch(s) {
                Ok(()) => line.push_str(&format!("{label}[{i}]: ok\n")),
                Err(e) => {
                    line.push_str(&format!("{label}[{i}]: ERR {}\n", e));
                    break;
                }
            }
        }
        line
    };

    let esc = token.replace('\'', "''");
    let esc_trim = token.trim().replace('\'', "''");
    // Strategy A: current engine approach (raw token, SET then ATTACH).
    out.push_str(&try_seq(
        "A",
        &[
            "INSTALL motherduck; LOAD motherduck;".into(),
            format!("SET motherduck_token='{esc}';"),
            "ATTACH 'md:' AS mda;".into(),
        ],
    ));
    // Strategy A-trim: trimmed token.
    out.push_str(&try_seq(
        "Atrim",
        &[
            "INSTALL motherduck; LOAD motherduck;".into(),
            format!("SET motherduck_token='{esc_trim}';"),
            "ATTACH 'md:' AS mdat;".into(),
        ],
    ));
    // Strategy B: token in the ATTACH DSN (trimmed).
    out.push_str(&try_seq(
        "B",
        &[
            "INSTALL motherduck; LOAD motherduck;".into(),
            format!("ATTACH 'md:?motherduck_token={esc_trim}' AS mdb;"),
        ],
    ));

    // eprintln (not panic) so this probe never reds the CI step; CI runs the
    // md step with `--nocapture` so this prints even though the test passes.
    eprintln!("DIAG RESULTS (token-redacted):\n{}", redact(out));
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
