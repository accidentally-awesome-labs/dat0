# dat0 P9a — Charts (design)

**Status:** brainstormed + approved (2026-06-14). Branch `p9a-design` off `main`
(`6e787d9`, P8 merged).
**Master-spec anchor:** §21.2 P9a — Charts (`docs/specs/2026-04-26-dat0-design.md:1107`).
**This cycle ships P9a-1** (render / explore / export). **P9a-2** (save → persist +
lineage) is fully designed here for context but executed as a separate
spec→plan→subagent cycle, mirroring the P6a/P6b split.

---

## 1. Context & framing

P9a is the "real charts phase". P6a deliberately hand-rolled the inspector's inline
histogram / top-N with GPUI quads and **deferred the charting-library decision to
P9a** (P6a design D7, `docs/plans/2026-06-06-dat0-p6a-design.md:22`). Those inline
inspector charts are tiny, theme-matched, and working — **they stay untouched**.
P9a adds a **per-query chart panel**: pick a chart type + axes against any active
grid, render via a real plotting library, and export.

**Purpose (user pick):** *fast in-flow exploration* — glance at the shape of a
result while working. Speed + native feel first; export is secondary but in scope.

**Decision: use a real charting crate** (user pick) — good-quality charts with much
less hand-rolling than extending GPUI quads to 7 chart types + authoring an SVG
emitter by hand.

---

## 2. Locked decisions

| # | Decision | Over | Why |
|---|----------|------|-----|
| D1 | **`plotters` → RGBA→BGRA buffer → GPUI `img(RenderImage)`** | ECharts/Plotly-in-webview; hand-rolled GPUI quads | Pure-Rust, no JS runtime/webview. One backend-generic draw routine serves screen + PNG + SVG. `plotters 0.3.7` **already in the tree** (dev-dep via `criterion`). Integration proven by gpui's own code. |
| D2 | **Unified "Visualize" on any active grid** | SQL-Console-results-only; dedicated chart tab | Most fluid for exploration. Source is always an engine table/view name (`GridDataSource`), so no per-surface special-casing. |
| D3 | **All 7 chart types in P9a-1** | core-5-then-defer | bar / line / area / scatter / histogram / box plot / heatmap. plotters renders all; box + heatmap add only data-prep + axis-config branches. |
| D4 | **Push-down plot query per type** | plot raw grid rows | Axis picks build a plot SQL via the engine (bar/box → `GROUP BY` + agg, histogram → binning, scatter → sample-to-N, line/area → `ORDER BY`). Fast + correct on million-row tables. |
| D5 | **Right-side chart panel in the content area** | bottom panel; modal sheet | Grid + chart visible together. Occupies the right-dock region (coexists with / supplants the Inspector dock while charting). |
| D6 | **Type-aware axis pickers** | free-for-all column lists | numeric-only for y/value; low-cardinality for group/color; per-type field visibility (histogram = x only; box = category + value; heatmap = x + y + value). |
| D7 | **Static raster charts in v1 (interactivity deferred)** | hover/crosshair/tooltip now | Raster needs a coord-mapping overlay; "all 7 types" already grew the slice. Deferred to a P9a polish follow-up (new deferral). |
| D8 | **Split P9a-1 (render) / P9a-2 (persist + lineage)** | one big slice | Mirrors P6a/P6b. P9a-1 delivers exploration value with zero persistence surface; P9a-2 adds Save → session + `.dat0` + lineage. |
| D9 | **PNG @ 2× device scale + SVG vector export, native save panel** | screenshot capture; fixed-DPI only | Reuse the P4c export save-panel pattern (`view/export_dialog.rs`); plotters writes the file (not engine COPY). |

---

## 3. Architecture — P9a-1

New crate-local module `crates/dat0-app/src/charts/` (extends the existing P6a
`charts/mod.rs`, which keeps its inline `histogram_bins` / `render_histogram` /
`render_topn`):

```
charts/
  mod.rs       (existing) inline inspector histogram/top-N — UNCHANGED
  spec.rs      ChartSpec (pure): ChartType, source name, x/y/group/color, title, opts.
               Serde-ready now so P9a-2 reuses it verbatim (no rewrite).
  query.rs     pure: ChartSpec + Vec<ColumnInfo> schema → plot SQL string. Unit-tested.
  data.rs      pure: Vec<RecordBatch> → PlotData (typed columns: Vec<f64> / Vec<String>),
               via downcast (reuse P6a profile.rs cell_f64 / decimal/null handling).
  render.rs    pure-ish: ChartSpec + PlotData + size → plotters draw routine,
               generic over plotters::DrawingBackend (screen, PNG, SVG share it).
  raster.rs    BitMapBackend → RGBA Vec<u8> → swap(0,2)+premult → RenderImage
               (scale_factor = device pixel ratio). The one GPUI seam.
  export.rs    PNG (BitMapBackend hi-res) + SVG (SVGBackend) → file bytes.
  panel.rs     GPUI ChartPanel view: type selector + axis pickers + Save/PNG/SVG,
               renders the RenderImage via img(); re-renders on spec/data change.
```

**Data flow**

```
active grid (bound to engine table/view name)
   │  user clicks "Visualize"
   ▼
ChartPanel opens, source = that name, default ChartType inferred from schema
   │  user sets type + axes (type-aware pickers)
   ▼
query::build_plot_sql(spec, schema)  ──►  engine.execute(sql) → Vec<RecordBatch>
   │                                          (small: aggregated / sampled)
   ▼
data::to_plot_data(batches)  ──►  render::draw(spec, data, size, backend)
   │
   ├─ screen:  raster::to_render_image(...) → img(ImageSource::Render(...))
   ├─ PNG:     export::png(spec, data, hi_res) → save panel → file
   └─ SVG:     export::svg(spec, data) → save panel → file
```

**Boundaries / testability** — `spec`, `query`, `data`, `render`, `export` are pure
(no GPUI), unit-testable headless. Only `raster` (GPUI `RenderImage`) and `panel`
(view) touch the framework. This matches dat0's "pure kernel + thin GPUI shell"
convention (cf. `export_dialog::build_export` pure kernel + `run_export` shell).

---

## 4. Rendering pipeline & the one gotcha

`plotters::backend::BitMapBackend` draws into an RGB(A) buffer. GPUI's `RenderImage`
stores **BGRA, premultiplied alpha**, and uses a `scale_factor` for crisp output —
verified in gpui's own SVG path (`elements/img.rs:660-707`, registry `gpui 0.2.2`):

```rust
// gpui img.rs:671-674 — its own RGBA→BGRA swizzle
for pixel in data.chunks_exact_mut(4) { pixel.swap(0, 2); }
// gpui img.rs:698-706 — pixels → RenderImage, with scale_factor
let buffer = ImageBuffer::from_raw(w, h, pixmap.take()).unwrap();
let mut image = RenderImage::new(SmallVec::from_elem(Frame::new(buffer), 1));
image.scale_factor = SMOOTH_SVG_SCALE_FACTOR;
```

So `raster.rs` is: plotters RGBA → `pixel.swap(0,2)` per pixel (charts are opaque,
α=255, so premultiply is a no-op) → `ImageBuffer::from_raw` → `RenderImage::new` →
set `scale_factor` = window device pixel ratio → `img(ImageSource::Render(Arc::new(img)))`.
**Without the swizzle, red/blue are swapped; without `scale_factor`, retina is blurry.**
This is the **T0 spike** (§9).

**All 7 types via plotters:** line/area/bar/scatter/histogram are first-class series;
**box plot** uses `plotters::element::Boxplot` fed per-category quartiles (computed in
the plot query or `data.rs`); **heatmap** is a matrix of filled `Rectangle`s with a
color-scale map over a 2-D pivot.

---

## 5. Plot-query pushdown (`query.rs`, D4)

`build_plot_sql(spec, schema) -> String`, wrapping the source name:

| Type | Plot SQL shape |
|------|----------------|
| bar | `SELECT x, <agg>(y) FROM src GROUP BY x ORDER BY x` (agg default `sum`; `count(*)` if no y) |
| line / area | `SELECT x, y[, group] FROM src ORDER BY x` (optional per-`group` series) |
| scatter | `SELECT x, y[, color] FROM src USING SAMPLE <N> ROWS` (DuckDB `USING SAMPLE`); panel notes "sampled N" |
| histogram | bin x over `[min,max]` into K buckets (reuse P6a `histogram_bins` math or a SQL `histogram`/`width_bucket`) |
| box plot | per category: `quantile_cont(y, [0,.25,.5,.75,1]) GROUP BY category` → whiskers/quartiles |
| heatmap | `SELECT x, y, <agg>(value) FROM src GROUP BY x, y` → pivot to grid in `data.rs` |

Defaults: numeric → bar/histogram; two numerics → scatter; temporal/ordered x →
line. Row/point caps keep render bounded (scatter sample-to-N; categorical group/
color capped with an "+M more" note). Plot queries run through the existing
`engine.execute` (`Vec<RecordBatch>`), same path as profiling.

---

## 6. Panel UX (D5, D6)

Opens in the right-dock region of the content area (grid stays visible to its left).

```
┌ grid ───────────────┬ Chart ──────────────────────────────┐
│ id  region   sales  │ [▾ Bar] x:[region▾] y:[sales▾]       │
│ ..  West     1203   │ group:[—▾] color:[—▾]  title:[_____] │
│ ..  East      980   │ [ Save ]  [ PNG ]  [ SVG ]           │
│ ..                  │ ┌────────────────────────────────┐   │
│                     │ │      ▆     ▆                    │   │
│                     │ │   ▆  █  ▆  █   (plotters img)   │   │
│                     │ │   █  █  █  █                    │   │
│                     │ └────────────────────────────────┘   │
└─────────────────────┴──────────────────────────────────────┘
```

- **Type selector** — 7 entries (icon/dropdown). Switching type re-derives which axis
  fields show (D6) and re-runs the plot query.
- **Axis pickers** — populated from the source schema (`engine` column info), filtered
  by role: y/value numeric-only; group/color prefer low-cardinality.
- **Save / PNG / SVG** — `Save` is **disabled/absent in P9a-1** (lands in P9a-2); PNG +
  SVG open the native save panel and write via `export.rs`. *(If we prefer no dead
  button, omit `Save` until P9a-2 — flagged for spec review.)*

---

## 7. P9a-2 (designed now, executed later)

Save a chart → it becomes a **persisted, lineage-attached artifact**. Follows the P8
**parallel-struct** convention (app session struct + format package struct + map at
export), exactly like `SavedQuery`↔`PackageQuery` and `PackageView`.

- **Session** (`session/mod.rs`, currently `SESSION_SCHEMA_VERSION = 8`): add
  `#[serde(default)] charts: Vec<SavedChart>` → **v8 → v9, additive** (same shape as
  the v5→v6 `saved_queries` add). Charts survive workspace save/reopen.
- **Format** (`dat0-format/model.rs`): add `PackageChart` + `Charts` and a
  `charts` field on `PackageContents` (`#[serde(default)]`, forward-compat). `writer`
  maps app→package; `reader` loads; **`dat0 diff` gains a charts dimension** (added /
  removed / changed by name). Replay is trivial — a chart carries its spec + source
  table name; on open it re-renders against the already-materialized Parquet (no
  recompute).
- **Lineage** (`inspector/lineage.rs`): add `NodeKind::Chart` (+ glyph); inject saved
  charts into `LineageGraph::build` as **descendants of their source table**; click =
  reopen the saved chart. Satisfies the master-spec exit "chart save adds a node to
  lineage".
- **D-025 does not bite** — charts enter a `.dat0` **only via in-app export** (the CLI
  can't author charts), where lineage is preserved; the CLI only reads charts.

---

## 8. Testing & exit criteria

**P9a-1 (this cycle)** — maps to the master-spec P9a exit it can satisfy without
persistence:

| Test | Asserts |
|------|---------|
| `query.rs` unit tests — one per type | correct plot SQL for bar/line/area/scatter/histogram/box/heatmap; agg + sample + bin + quantile shapes |
| `data.rs` unit tests | RecordBatch → typed PlotData incl. the P6a `Decimal128`/null gotchas |
| `render.rs` smoke (headless) | each of the 7 types draws to a BitMapBackend buffer without panic on sample data |
| `raster.rs` T0 round-trip | RGBA→BGRA swizzle correct (a known-color pixel lands as the right channel); non-empty `RenderImage` |
| Export PNG | produces a non-empty PNG at the expected pixel size (2× scale) |
| Export SVG | produces a parseable SVG (root `<svg>`, expected viewBox) |
| "Visualize" wiring | opens the panel bound to the active grid's source name; type/axis change re-renders |
| Big-table guard | scatter on a large table samples (point count ≤ N); plot query returns promptly |

**Deferred to P9a-2:** chart save adds a lineage node; chart persists across reopen;
chart survives `.dat0` export/inspect; `dat0 diff` charts dimension.

**Advisory bench** (no hard gate until P10): render+raster a representative chart on a
1 M-row source — exploration should feel instant; record the number.

---

## 9. T0 spike gates (decide/prove in the plan)

1. **`raster.rs` GPUI seam (the real risk):** plotters `BitMapBackend` → RGBA →
   `swap(0,2)` + premult → `RenderImage::new` + `scale_factor` → `img()` renders a
   correct, crisp test chart in a throwaway window. Proves byte order + scale.
2. **plotters API at 0.3.7:** `Boxplot` element + a heatmap (rectangle matrix + color
   map) compile and draw — confirm the two non-series types before committing to "all 7".
3. **Panel mount:** opening the chart panel in the right-dock region composes with the
   existing `WorkspaceShell` docks (Catalog left / Inspector right) without layout
   breakage; "Visualize" reaches the active grid's bound source name.
4. **Engine plot-query path:** `engine.execute(plot_sql)` → `Vec<RecordBatch>` →
   `data.rs` extraction works for `GROUP BY` / `USING SAMPLE` / `quantile_cont` results.

If a spike fails: D1 fallback is plotters **SVGBackend → resvg/tiny-skia rasterize →
RenderImage** (those crates are already gpui deps), reusing gpui's exact SVG-raster
code path instead of BitMapBackend.

---

## 10. Deferrals opened by P9a

- **D-026 (new): chart interactivity** — hover-tooltip / crosshair / click-select on
  raster charts (needs a GPUI coord-mapping overlay). Deferred from P9a-1 (D7); revisit
  as a P9a polish or v1.x.
- **P9a-2** itself is the tracked follow-on for save + persist + lineage (§7).

---

## 11. Open questions for spec review

1. **`Save` button in P9a-1** — show it disabled (signals what's coming) or omit it
   until P9a-2 (no dead control)? *Default: omit.*
2. **PNG default size** — 2× device scale of the panel, or a fixed 2560×1600? *Default:
   2× device scale, min 1280×800 logical.*
3. **Default chart type** on first open — infer from schema (§5) or always start on
   bar? *Default: infer.*
