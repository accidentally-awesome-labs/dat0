//! ADVISORY bench (no hard gate until P10). Reports end-to-end plot-query + raster wall time on a ~1M-row source.
//!
//! The measured loop is the full "explore" step the panel runs on click:
//! `build_plot_sql` -> `engine.execute` -> `PlotTable::from_query_result`
//! -> `render::render_bgra`. A categorical+numeric ~1M-row table is built ONCE
//! (outside the measured iterations) via the real DuckDB engine harness. Bar is
//! server-aggregated (cheap, ~100 rows back); Scatter is sample-capped
//! (`USING SAMPLE 2000 ROWS`) so both stay bounded regardless of source size.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use dat0_app::charts::data::PlotTable;
use dat0_app::charts::query::build_plot_sql;
use dat0_app::charts::render;
use dat0_app::charts::spec::{ChartSpec, ChartType};
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use tokio::runtime::Runtime;

const ROWS: u64 = 1_000_000;
const SIZE: (u32, u32) = (1040, 720);

fn spec(t: ChartType) -> ChartSpec {
    ChartSpec {
        chart_type: t,
        source: "\"big\"".into(),
        x: Some("region".into()),
        y: Some("amt".into()),
        group: None,
        color: None,
        title: "bench".into(),
    }
}

fn bench_chart_render(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = DuckDBEngine::new(
        tmp.path().join("bench.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .expect("engine");
    rt.block_on(async {
        engine.init().await.expect("init");
        engine
            .create_table(
                "big",
                &format!(
                    "SELECT (i % 50)::VARCHAR AS region, (i * 1.5) AS amt FROM range({ROWS}) t(i)"
                ),
                DerivedOrigin::Sql("seed".into()),
            )
            .await
            .expect("seed big table");
    });

    let mut group = c.benchmark_group("chart_render");
    for (label, t) in [("bar", ChartType::Bar), ("scatter", ChartType::Scatter)] {
        let s = spec(t);
        let sql = build_plot_sql(&s).expect("build_plot_sql");
        group.bench_function(label, |b| {
            b.iter(|| {
                let qr = rt.block_on(engine.execute(&sql)).expect("execute");
                let pt = PlotTable::from_query_result(&qr);
                black_box(render::render_bgra(black_box(&s), black_box(&pt), SIZE));
            });
        });
    }
    group.finish();

    rt.block_on(async { engine.close().await.expect("close") });
}

criterion_group!(benches, bench_chart_render);
criterion_main!(benches);
