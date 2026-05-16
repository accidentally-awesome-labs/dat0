//! 1M-row virtualized-scroll bench.
//!
//! Generates a synthetic Arrow batch (Int64, Float64, Utf8, UInt64; 1%
//! nulls), drives the same `render_cell` dispatch the live grid uses
//! over the full row range, and samples frame time. The bench is
//! GPU-kernel-independent: it measures the cost of the per-cell render
//! decision plus Arrow column access — the path that dominates real-world
//! scroll frames once the widget reaches steady state. Real GPU
//! frame-pacing is verified at P10 on the provisioned self-hosted GPU
//! runner; P3a's metric is an *engine + cell-render* upper bound.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use dat0_app::grid::renderers::render_cell;
use duckdb::arrow::array::{Float64Builder, Int64Builder, StringBuilder, UInt64Builder};
use duckdb::arrow::datatypes::{DataType, Field, Schema};
use duckdb::arrow::record_batch::RecordBatch;
use std::sync::Arc;

const ROWS: usize = 1_000_000;
const VIEWPORT_ROWS: usize = 50; // typical visible rows per frame

fn build_batch() -> RecordBatch {
    let mut i_b = Int64Builder::with_capacity(ROWS);
    let mut f_b = Float64Builder::with_capacity(ROWS);
    let mut s_b = StringBuilder::with_capacity(ROWS, ROWS * 8);
    let mut u_b = UInt64Builder::with_capacity(ROWS);
    for r in 0..ROWS {
        if r % 100 == 0 {
            i_b.append_null();
            f_b.append_null();
            s_b.append_null();
            u_b.append_null();
        } else {
            i_b.append_value(r as i64);
            f_b.append_value(r as f64 * 0.5);
            s_b.append_value(format!("r{}", r));
            u_b.append_value((r as u64).wrapping_mul(11));
        }
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("i", DataType::Int64, true),
        Field::new("f", DataType::Float64, true),
        Field::new("s", DataType::Utf8, true),
        Field::new("u", DataType::UInt64, true),
    ]));
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(i_b.finish()),
            Arc::new(f_b.finish()),
            Arc::new(s_b.finish()),
            Arc::new(u_b.finish()),
        ],
    )
    .unwrap()
}

fn bench_frame_render(c: &mut Criterion) {
    let batch = build_batch();
    let mut group = c.benchmark_group("grid_scroll");
    group.throughput(Throughput::Elements((VIEWPORT_ROWS as u64) * 4));
    group.bench_function("viewport_50rows_4cols", |b| {
        let mut row_cursor = 0usize;
        b.iter(|| {
            for r in row_cursor..(row_cursor + VIEWPORT_ROWS).min(ROWS) {
                for c in 0..4 {
                    black_box(render_cell(&batch, c, r));
                }
            }
            row_cursor = (row_cursor + VIEWPORT_ROWS) % (ROWS - VIEWPORT_ROWS);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_frame_render);
criterion_main!(benches);
