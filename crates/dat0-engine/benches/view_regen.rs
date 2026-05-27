//! P4a T0 — view_regen perf spike.
//!
//! Measures the round-trip latency of:
//!   t1  = CREATE OR REPLACE TEMP VIEW v AS SELECT * FROM t WHERE price > 5000
//!   t2  = execute_paged("SELECT * FROM v", 0, 100)            // first page
//!   t3  = execute_paged("SELECT * FROM v", 999_900, 100)      // last page (forces full materialisation)
//!
//! Hot-path metric per design §9: t1 + t2 (Apply-click → first row paint).
//! Target: < 500 ms p95 on macOS-arm + linux-x86. If exceeded, document
//! Plan B (CTAS-on-apply) in docs/internal/dat0-p4a-t0-spike.md and amend
//! design §4 + plan T2/T4/T13 in place.

use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use tempfile::TempDir;
use tokio::runtime::Runtime;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use dat0_fixtures::filter::gen_filter_fixture;

fn bench_view_regen(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");

    // Setup: build the 1M-row fixture once (cached across runs) and register.
    let tmp = TempDir::new().expect("tempdir");
    let csv = gen_filter_fixture(tmp.path(), 1_000_000, 0xCAFEBABE).expect("generate fixture");

    let engine = rt.block_on(async {
        let e = DuckDBEngine::new(
            tmp.path().join("scratch.duckdb"),
            MemoryBudget {
                bytes: 4 * 1024 * 1024 * 1024, // 4 GB headroom for the spike
            },
        )
        .unwrap();
        e.init().await.unwrap();
        e.register_file(&csv, RegisterOpts::default())
            .await
            .unwrap();
        Arc::new(e)
    });

    let table_name = rt.block_on(async { engine.get_tables().await.unwrap()[0].name.clone() });

    // t1 + t2 — hot-path metric
    let mut group = c.benchmark_group("view_regen");
    group.sample_size(20); // 20 samples; ~10 minutes total
    group.measurement_time(std::time::Duration::from_secs(30));

    group.bench_function("t1_create_view_plus_t2_first_page", |b| {
        let e = Arc::clone(&engine);
        let tn = table_name.clone();
        b.to_async(&rt).iter(|| {
            let e = Arc::clone(&e);
            let tn = tn.clone();
            async move {
                let sql = format!(
                    "SELECT * FROM \"{}\" WHERE \"price\" > 5000.0",
                    tn.replace('"', "\"\"")
                );
                let view_sql = format!("CREATE OR REPLACE TEMP VIEW v_spike AS {}", sql);
                e.execute(&view_sql).await.unwrap();
                e.execute_paged("SELECT * FROM v_spike", 0, 100)
                    .await
                    .unwrap();
            }
        });
    });

    // t1 + t2 + t3 — full-materialisation sanity check
    group.bench_function("t1_plus_t2_plus_t3_last_page", |b| {
        let e = Arc::clone(&engine);
        let tn = table_name.clone();
        b.to_async(&rt).iter(|| {
            let e = Arc::clone(&e);
            let tn = tn.clone();
            async move {
                let sql = format!(
                    "SELECT * FROM \"{}\" WHERE \"price\" > 5000.0",
                    tn.replace('"', "\"\"")
                );
                let view_sql = format!("CREATE OR REPLACE TEMP VIEW v_spike AS {}", sql);
                e.execute(&view_sql).await.unwrap();
                e.execute_paged("SELECT * FROM v_spike", 0, 100)
                    .await
                    .unwrap();
                e.execute_paged("SELECT * FROM v_spike", 999_900, 100)
                    .await
                    .unwrap();
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_view_regen);
criterion_main!(benches);
