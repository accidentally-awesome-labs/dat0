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
//!
//! T13 (P3b) note: T4 mounted the real `gpui_component::Table` widget
//! against `WorkspaceShell.data_source`, but the Table widget cannot be
//! exercised headlessly — `TableDelegate::render_td_cell` takes
//! `&mut Window` and runs inside the GPUI foreground executor + Cocoa
//! main thread. A non-window test harness has no `Window` to pass.
//! T13 therefore keeps the existing harness (which measures `render_cell`
//! over the same in-memory Arrow batch the real `GridTableDelegate` calls
//! through) as the P3b baseline. Real-Table frame timing is deferred to
//! the P10 perf-budget runner per spec line 819. See
//! `docs/internal/p3-bench-baselines.md` for the recorded numbers + the
//! headless-vs-Table comparison plan.
//!
//! ⚠ UI-redesign B5 ruling — WHAT THIS BENCH DOES NOT MEASURE.
//!
//! This harness calls `renderers::render_cell` in a plain loop over a synthetic
//! Arrow batch. It never builds a `Window`, a `WorkspaceShell`, or the
//! `gpui_component::Table` widget, so it is blind to everything about how the
//! grid is MOUNTED: the `TableDelegate`, `render_td`, the per-cell theme reads
//! inside it, and the whole element tree above the table. Its sensitivity
//! surface is `render_cell` plus Arrow column access, and nothing else.
//!
//! Consequently the A5 and A6 readings — "the bench held with `grid/mod.rs` in
//! the diff" — were measuring something that structurally could not contain
//! those changes. They are not evidence of no regression; they are evidence of
//! nothing, and must not be cited as reassurance.
//!
//! B5 (DockArea adoption) keeps this bench as a `render_cell` watchdog, which
//! it genuinely is — that function is on the per-cell hot path — and bases its
//! own no-regression claim on structure instead: `grid/mod.rs` is byte-
//! untouched, and `DockItem::Panel` puts zero elements between the shell and the
//! `Table` (measured: panel-body bounds equal host bounds). Real per-frame
//! timing remains D-013's perf runner, which already owns it as a P10-exit gap.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use dat0_core::grid::renderers::render_cell;
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
