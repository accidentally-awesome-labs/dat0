# P3a — Bench baselines

Recorded by T14 (`crates/dat0-app/benches/grid_scroll.rs`). The bench measures
the `render_cell` dispatch cost over a synthetic 1M-row Arrow batch (Int64 +
Float64 + Utf8 + UInt64, 1% nulls). It is GPU-kernel-independent: it samples the
per-cell decision + Arrow column-access path that dominates real-world scroll
frames once the widget reaches steady state. GPU frame-pacing on a real grid
widget is verified at P10 (spec §21.2 P10) on the provisioned self-hosted GPU
runner; P3a's metric is an *engine + cell-render* upper bound.

Spec target: 60 fps → per-frame mean ≤ 16.67 ms for a 50-row × 4-column
viewport. Soft floor recorded; no merge gate at P3a (spec line 819).

## Format

Each row records one bench run. Columns:

| Date | Host / OS | rustc | Criterion estimate | fps |
|------|-----------|-------|--------------------|-----|

`fps = 1 s / per-iter mean`. Use `cargo bench -p dat0-app --bench grid_scroll`
to reproduce. Criterion output: `viewport_50rows_4cols`.

## Baselines

| Date | Host / OS | rustc | Criterion estimate | fps |
|------|-----------|-------|--------------------|-----|
| 2026-05-16 | macOS 25.4.0 / arm64 (dev box, Apple Silicon) | 1.95.0 | mean = 14.759 µs/iter [14.654, 14.892] | ~67 700 fps |

## Reading the result

The first baseline puts cell-render dispatch at ~15 µs for a 50-row × 4-column
viewport — roughly **1130×** the 60-fps budget. The bench measures only
`render_cell` over an in-memory Arrow batch; in the live grid, GPU upload +
text shaping + paint dominate per-frame time. The dispatch cost recorded here
is an *upper bound* on the engine + cell-render contribution to a real frame.

Spec target (60 fps, ≤16.67 ms/frame) is satisfied with significant headroom on
this hardware. P10 will re-bench under the real grid widget + GPU pipeline.

