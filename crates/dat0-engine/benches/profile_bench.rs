//! P6a T5 — profile_table <2 s exit gate.
//!
//! Seeds a 1 M-row table (once) then benchmarks `profile_table` end-to-end.
//! Target: median < 2 000 ms on macOS-arm + linux-x86.

use criterion::{Criterion, criterion_group, criterion_main};
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use tokio::runtime::Runtime;

fn bench_profile_1m(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = DuckDBEngine::new(
        tmp.path().join("b.duckdb"),
        MemoryBudget {
            bytes: 1024 * 1024 * 1024,
        },
    )
    .unwrap();

    rt.block_on(async {
        engine.init().await.unwrap();
        engine
            .create_table(
                "big",
                "SELECT i AS id, (i % 7) AS cat, random() AS val, \
                 ('s' || (i % 100)::VARCHAR) AS label \
                 FROM range(1_000_000) t(i)",
                DerivedOrigin::Sql("seed".into()),
            )
            .await
            .unwrap();
    });

    let mut group = c.benchmark_group("profile_bench");
    // 10 samples keeps total wall-clock reasonable (~2–3 min) while still
    // producing a reliable median; criterion minimum is 10.
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(60));

    group.bench_function("profile_table_1m", |b| {
        b.iter(|| {
            rt.block_on(async { engine.profile_table("big", None).await.unwrap() });
        });
    });

    group.finish();
}

criterion_group!(benches, bench_profile_1m);
criterion_main!(benches);
