# Phase 0 — Dioxus renderer spikes

**Date:** 2026-08-09
**Machine:** Apple M1 Max, macOS (darwin 27.0.0), release builds

| Spike | Code | Verdict |
|---|---|---|
| 0.1 grid throughput | `crates/dat0-ui/examples/grid_spike.rs` | **PASS** — p95 scroll-to-repaint 18 ms, gate ≤ 33 ms |
| 0.2 CodeMirror embed | `crates/dat0-ui/examples/editor_spike.rs` | **PASS** — schema-driven completion + full bidirectional protocol |

---

# 0.1 — Grid throughput

## Why this gate exists

`dioxus-desktop` does not mutate the DOM in-process. Edits are serialised, sent
to the webview over a localhost WebSocket, and `WebviewInstance::edits_in_progress`
(`dioxus-desktop-0.7.10/src/edits.rs`) **blocks the VirtualDom until the flush is
acked**. dat0's grid repaints on every scroll tick over a 1 M-row source, so the
whole renderer decision hangs on whether that round trip fits in a frame.

## What was measured

1,000,000 rows × 40 columns of synthetic mixed-type text (ints, floats, words,
bools, dates), styled the way the shipped grid will be: Geist Mono 12.5 px served
over the `dat0` custom asset protocol, 26 px rows, `0 8px` cell padding, a 1 px
`#dde2e8` row rule, a `:hover` tint, and a 40-cell `.sel` block so the selection
path is inside the measurement. Visible range is computed in Rust from
`ScrollData::{scroll_top, scroll_left, client_width, client_height}` with 4 rows /
2 columns of overscan; one absolutely-positioned `div` per visible row, one per
visible cell, both keyed by absolute index.

Driven programmatically: `viewport.scrollTop += 40` on a 120 Hz `setInterval` for
10 s (the browser coalesces scroll events to the 60 Hz display refresh).

## How it is measured, and two framings that lie

Timestamps are taken entirely in JS so there is no Rust/JS clock skew. Rust
stamps the canvas with `data-top` = the exact `scrollTop` the current DOM was
rendered for; JS keeps a `scrollTop → event timestamp` table and correlates.

Two obvious framings were tried first and both produced garbage:

| Framing | Reported | Why it is wrong |
|---|---|---|
| `MutationObserver` fires → sample against the newest scroll stamp | p95 = **0 ms** | The mutation for scroll *N* lands just *after* scroll *N+1*'s timestamp, so it times an unrelated pair. |
| Wait until `data-top` equals the newest `scrollTop` | p95 = **1089 ms**, 2 samples in 10 s | Under continuous scroll the target keeps moving; the DOM is always one frame behind, so it never "catches up" until scrolling stops. |

The reported numbers use two correlated metrics instead:

- **scroll-to-repaint latency** — for each distinct DOM state, the delay from the
  scroll event that caused it to the first animation frame that shows it.
- **on-screen content age** — sampled every animation frame: how old the pixels
  currently on screen are. This is what a user perceives while scrolling.

## Results (3 consecutive runs)

| Run | fps | scroll events | renders shown | repaint p50 | p95 | p99 | max | content-age p95 |
|---|---|---|---|---|---|---|---|---|
| 1 | 59.8 | 597 | 595 | 17 ms | **18 ms** | 19 ms | 70 ms | 18 ms |
| 2 | 59.9 | 599 | 597 | 17 ms | **18 ms** | 19 ms | 45 ms | 18 ms |
| 3 | 59.8 | 597 | 594 | 17 ms | **18 ms** | 21 ms | 51 ms | 18 ms |

Rust-side cross-check (scroll event → post-render effect, i.e. the VirtualDom's
own share of the cost): p50 ≈ 4 ms, p95 ≈ 7 ms.

Live DOM at the end of every run: **40 row nodes, 600 cell nodes** out of
1,000,000 × 40 virtual. Virtualization holds.

## Reading the number

17 ms p50 is one 60 Hz frame. The pipeline is scroll event at frame *N* → Rust
render (~4 ms) → serialise → WebSocket → apply → paint at frame *N+1*. That is the
floor for *any* architecture that reacts to a scroll event, so the WebSocket
round trip is not the bottleneck; the display refresh is. Renders-shown tracks
scroll-events one-for-one, so no frame is dropped under sustained 120 Hz scroll
pressure.

Neither contingency from the plan ("Assumptions & contingencies") is needed: no
single-`div`-per-row collapse, no `<canvas>` body.

## Reproduce

`crates/dat0-ui` is a **detached workspace** for the duration of the migration —
`gpui 0.2.2` pins `cocoa "=0.26.0"` while every stable `dioxus-desktop 0.7.x`
requires `^0.26.1`, and Cargo cannot hold both in one lockfile. So the command is
run from inside the crate, not with `-p` from the repo root:

```sh
cd crates/dat0-ui && cargo run --release --example grid_spike
```

Exits 0 on PASS, 1 on FAIL.

---

# 0.2 — CodeMirror 6 embed

**Verdict: PASS.**

## What had to be proven

Blitz was rejected partly because a CodeMirror-class SQL editor is impossible
without a JS engine. That argument only holds if the editor is genuinely
tractable *with* one — specifically, whether Rust can drive a CodeMirror
instance that lives inside the webview: push a schema in, get document and
cursor changes back, and keep the editor's own keymap from leaking into the
shell's key cascade.

## Shape

`crates/dat0-ui/vendor/codemirror/` holds a pinned `package.json` and an
`esbuild` config; `node build.mjs` emits a 413 KiB minified IIFE to
`crates/dat0-ui/assets/codemirror.js`, which is **committed** (CI has no Node,
and `xtask` bundling must stay a pure `cargo build`). The bundle exposes one
global, `window.dat0cm`, and speaks the Phase-4.2 protocol.

There is **no `codemirror.css`**: CodeMirror 6 injects its styles at runtime via
`style-mod`, and dat0's editor theme is generated in JS from the resolved
`--d0-*` tokens, so the editor cannot drift from the rest of the app.

### One finding that shapes Phase 4.2

`document::eval` hands the script a **scoped** `dioxus` object; it is *not*
published on `window`. A bundle loaded through `<script src>` therefore has no
route back to Rust. The fix is an explicit handoff — Rust opens one long-lived
eval per window whose first act is `dat0cm.bind(dioxus)` — and every push
message (`ready`, `change`, `cursor`, `run`) then travels on that one channel.
Phase 4.2 must keep that channel alive for the window's lifetime, not per
message.

## Result

```
completions after `SELECT * FROM `: 837 offered -> [nyc_taxi_trips, ABORT, ABS, …]
completions after `date_tr`:        3 offered -> [date_trunc, DATETIME_INTERVAL_CODE, DATETIME_INTERVAL_PRECISION]
document observed from Rust:        "-- dat0 SQL console\nSELECT * FROM \nSELECT date_tr"
pushed from the editor:             ready=true change x2 cursor x2 run x1 (last cursor Some((3, 15)))
ACCEPTANCE: PASS
```

All five assertions hold:

| Assertion | Result |
|---|---|
| Rust-supplied table `nyc_taxi_trips` heads the popup after `SELECT * FROM ` | yes |
| Rust-supplied function `date_trunc` completes from the DuckDB catalogue | yes |
| Document text round-trips to Rust | yes |
| `ready` / `change` / `cursor` reach Rust from inside the editor | yes |
| A real `Mod-Enter` keydown on `contentDOM` surfaces as `run`, not as a shell key | yes |

The last two are checked against push messages the *bundle* emitted, not values
the driver script fabricated, so they exercise the path Phase 4.2 will ship.

## Reproduce

```sh
cd crates/dat0-ui && cargo run --release --example editor_spike
```

Exits 0 on PASS, 1 on FAIL. To rebuild the bundle first:

```sh
cd crates/dat0-ui/vendor/codemirror && npm ci && node build.mjs
```
