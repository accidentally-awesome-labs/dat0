# P3 — Bench baselines

Recorded by P3a T14 and refreshed by P3b T13 (`crates/dat0-app/benches/grid_scroll.rs`).
The bench measures the `render_cell` dispatch cost over a synthetic 1M-row
Arrow batch (Int64 + Float64 + Utf8 + UInt64, 1% nulls). It is
GPU-kernel-independent: it samples the per-cell decision + Arrow
column-access path that dominates real-world scroll frames once the widget
reaches steady state. GPU frame-pacing on a real grid widget is verified at
P10 (spec §21.2 P10) on the provisioned self-hosted GPU runner; the metric
recorded here is an *engine + cell-render* upper bound.

Spec target: 60 fps → per-frame mean ≤ 16.67 ms for a 50-row × 4-column
viewport. Soft floor recorded; no merge gate at P3a/P3b (spec line 819).

**Headless-vs-real-Table:** P3b T4 mounted the real `gpui_component::Table`
widget against `WorkspaceShell.data_source`. The Table's
`TableDelegate::render_td_cell` runs inside the GPUI foreground executor on
the Cocoa main thread and takes `&mut Window` — there is no non-window
test harness for it. The headless bench here continues to exercise the
`render_cell` path the real `GridTableDelegate` calls through. Real-Table
frame-pacing measurement is deferred to the P10 perf-budget runner per
spec line 819.

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

## P3b T13 — real Table mount

Date: 2026-05-25
Branch: `p3b-ux-polish`
Commit at write-time: pre-`docs(p3b): retro + bench baseline refresh` (T13)
Hardware: Apple Silicon dev box (macOS 25.5.0 / arm64)

**Status:** local bench run skipped at T13 — the criterion executable was
still compiling when the 2-minute time-cap fired (the dev box's
`target/criterion/grid_scroll/` is empty after the run). The macOS-arm64
hosted CI runs `cargo bench -p dat0-app --bench grid_scroll` unconditionally
on every push to the PR branch and uploads the criterion output as an
artifact; that run is the authoritative P3b T13 number once CI lands.

Until the CI artifact arrives, the most-recent recorded baseline is still
the **P3a T14** entry above (mean 14.759 µs/iter, ≈ 67 700 fps for the
50 × 4 viewport on the same dev box). The bench code itself is unchanged
between P3a T14 and P3b T13 — the only delta is a doc-comment update to
flag that the real `gpui_component::Table` widget cannot be exercised
headlessly (see the headless-vs-real-Table note above and the module
doc-comment in `crates/dat0-app/benches/grid_scroll.rs`).

**Expected vs floor:** P3a retro Rec #5 estimated a 3-5× slowdown going
from `render_cell`-only to the full Table chain (text shaping + GPU
upload + paint). Even at 5× of the P3a baseline (≈ 74 µs/iter), the
50 × 4 viewport draws roughly 225× under the 60 fps frame budget
(16 670 µs). The bench has no merge gate at P3b (spec line 819);
P10 perf-budget runner remains the final word, against the real Table
widget exercised through a live `Window`.

| Date | Host / OS | rustc | Criterion estimate | fps |
|------|-----------|-------|--------------------|-----|
| 2026-05-25 | macOS 25.5.0 / arm64 (dev box, Apple Silicon) | 1.95.0 | deferred to CI (local run timed out at compile) | — |

