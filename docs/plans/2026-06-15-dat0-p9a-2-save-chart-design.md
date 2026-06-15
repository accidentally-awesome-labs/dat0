# dat0 P9a-2 — Save chart → persist + lineage (design)

**Date:** 2026-06-15
**Branch off:** `main` (`9d093e0`, P9a-1 merged via PR #22)
**Predecessor:** P9a-1 (charts render / explore / export — `docs/plans/2026-06-14-dat0-p9a-design.md`, §7 of which is the seed for this slice)
**Cycle:** brainstorm → spec → plan → execute (subagent-driven-dev), its own slice — mirrors the P6a/P6b split.

---

## 1. Goal

A rendered chart is, today, ephemeral: P9a-1 ships render / explore / export but the
chart panel has **no Save**. P9a-2 makes a chart a **persisted, lineage-attached
artifact**:

- **Save** a chart from the panel → it survives workspace save / reopen.
- The saved chart appears in the Inspector **lineage chain** as a node descending from
  its source table; clicking it reopens the chart.
- A saved chart round-trips through the `.dat0` **package format** (writer → reader),
  participates in `dat0 diff` (a new charts dimension) and is listed by `dat0 inspect`.

This satisfies the master-spec exit "chart save adds a node to lineage" and rounds out
charts as first-class workspace + package citizens.

Non-goals (explicitly out): chart interactivity (deferred, D-026); editing a chart's
underlying query; a Charts list in the Catalog or command palette (lineage node is the
only reopen surface for this slice); CLI authoring of charts (CLI only **reads** them).

---

## 2. Verified premises (against `main` @ `9d093e0`)

| Premise | Status |
|---|---|
| `ChartSpec` is serde-ready (`charts/spec.rs`): `chart_type`, `source: String` (quoted/qualified), `x/y/group/color: Option<String>`, `#[serde(default)] title`. Reused **verbatim**. | ✓ |
| Session at `SESSION_SCHEMA_VERSION = 8`; `saved_queries: Vec<SavedQuery>` lives on `SessionState`; migrate uses **literal version arms** (`session/migrate.rs`). | ✓ |
| `SavedQuery { id: Uuid, name: String, sql: String, saved_at: i64 }` (`session/queries.rs`). | ✓ |
| `dat0-format/model.rs` uses the **parallel-struct** convention: `PackageQuery { id, name, sql, saved_at }` + `Queries(Vec<…>)` newtype + `queries: Queries` on **both** `PackageContents` (writer input) and `ParsedPackage` (reader output). `PackageView`/`Views` likewise. | ✓ |
| `dat0-format/diff.rs` reports orthogonal dimensions today: `schema`, `lineage`, `queries`, `row_count_deltas` (the "four"); `QueryChange { Added, Removed, SqlChanged { from, to } }`, `QueryDelta { name, change }` keyed by **name**; `PackageDiff::is_empty()` + `render_text()` aggregate them. | ✓ |
| `inspector/lineage.rs`: `enum NodeKind { Table, File, External }`; `LineageGraph::build` assigns kinds; edges match on **bare** names (P6b rule). | ✓ |
| `charts/panel.rs` has **no** Save affordance. | ✓ |

**Correction to the brainstorm transcript:** during brainstorming I stated `SavedChart`
would carry "no uuid, no timestamp." Reading the code shows `SavedQuery` (and its package
mirror `PackageQuery`) carry both `id: Uuid` and `saved_at: i64`. To keep the
parallel-struct convention faithful and the writer/diff symmetric, `SavedChart` /
`PackageChart` carry `id` + `saved_at` too. Identity for **dedupe** remains the **name**.

---

## 3. Data structures

### 3.1 App / session — `SavedChart`

New type (in `session/`, alongside `SavedQuery`; `ChartSpec` imported from `charts::spec`):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedChart {
    pub id: Uuid,
    pub name: String,
    pub spec: ChartSpec,   // reused verbatim
    pub saved_at: i64,
}
```

`SessionState` gains:

```rust
#[serde(default)]
pub charts: Vec<SavedChart>,
```

`#[serde(default)]` makes an old (v8) session deserialize with `charts == []`.

**Upsert by name:** saving a chart whose `name` already exists **replaces** that entry
in place (keeping its `id`, refreshing `spec` + `saved_at`); otherwise push a new
`SavedChart` with a fresh `Uuid`. This mirrors how a saved query with a reused name is
treated and keeps `name` the stable user-facing key.

### 3.2 Session migration — v8 → v9

- Bump `SESSION_SCHEMA_VERSION` to **9**.
- Add `migrate_v8_to_v9(raw)`: purely additive — set `charts = Vec::new()`, stamp
  `schema_version = 9`. Follow the existing **literal-arm** dispatch (`8 => migrate_v8_to_v9`)
  and the established doc-comment style in `migrate.rs`.
- All older arms (`migrate_vN_to_v(N+1)`) chain forward to 9 as today; each already sets
  `schema_version = SESSION_SCHEMA_VERSION` at the end of the chain.

### 3.3 Format — `PackageChart` / `Charts`

In `dat0-format/model.rs`, mirroring `PackageQuery` / `Queries` exactly:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageChart {
    pub id: uuid::Uuid,
    pub name: String,
    pub spec: ChartSpec,   // serialized structurally; NOT a rendered image
    pub saved_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Charts {
    pub charts: Vec<PackageChart>,
}
```

`ChartSpec` currently lives in `dat0-app` (`charts/spec.rs`), and `dat0-format` must not
depend on `dat0-app`. **Precedent (verified):** `PackageView.transform_stack` is
`Vec<Transformation>` where `Transformation` is defined in **`dat0-engine`**
(`transform.rs`) and `dat0-format` reuses it via `use dat0_engine::transform::Transformation`
— i.e. portable model types are **engine-owned** and shared by both app and format (both
already depend on `dat0-engine`).

**Decision: relocate the portable core of `charts/spec.rs` (`ChartType`, `ChartSpec`,
`AxisRole`, `is_numeric`, and their inherent impls `axes()` / `label_key()`) into
`dat0-engine`** (e.g. `dat0-engine/src/chart_spec.rs`), and re-export from
`dat0-app::charts::spec` so existing app call sites are untouched. `dat0-format` then
references the engine type — a single serde definition shared by app + package, exactly
mirroring `Transformation`. (`label_key()` returns a const i18n-key string and `is_numeric`
is a DuckDB-type helper — both are pure data, fine to live in the engine.)

Add `charts: Charts` to **both** `PackageContents` and `ParsedPackage`,
with `#[serde(default)]` on the on-disk manifest field for forward-compat (an older
package without `charts` reads as empty).

---

## 4. Save flow (UX)

- Add a **Save** button to the right end of the existing button-cycle toolbar in
  `charts/panel.rs`. Disabled / no-op when no chart is currently rendered (no spec bound).
- Click → a **name prompt**, pre-filled with a generated default:
  - `"<Type>: <y> by <x>"` when both axes are set (e.g. `"Bar: amount by region"`),
  - else `"<Type> of <bare source>"` (e.g. `"Histogram of orders"`).
  - Reuse the same text-input prompt mechanism as P5b **Save-as-Table** (do not invent a
    new widget; planning pins the exact gpui-component used there).
- Confirm → `upsert_by_name` into `session.charts`, persist the session via the existing
  save path (charts ride along because they live on `SessionState`), and push a
  confirmation **banner/toast** through the existing host (reuse P5b's Save-as-Table toast).
- Reopening a saved chart (from lineage), tweaking axes/type, and Saving with the **same
  name** overwrites it; saving under a new name creates a second chart.

No session-dirty/auto-save semantics change: a saved chart is persisted on the same
trigger as any other session mutation.

---

## 5. Lineage

`inspector/lineage.rs`:

- Add `NodeKind::Chart` to the enum, with a glyph (e.g. `📊`) in the render path that
  maps `NodeKind` → glyph (alongside `Table`/`File`/`External`).
- In `LineageGraph::build`, after table/view nodes are placed, inject each
  `session.charts` entry as a **descendant** of its source table. Match chart → table by
  **bare name**: reduce `spec.source` via the existing `bare_table_name()` helper and
  compare to the bare table node key — consistent with P6b's bare-vs-bare edge rule.
  (Known limit, inherited from P6b: same bare name across attached DBs collides; documented,
  not addressed here.)
- A chart node carries enough to reopen: its `SavedChart` (by `id`/`name`). Clicking a
  Chart node reopens that chart in the panel (re-bind the panel to `spec`, re-render
  against the already-materialized source — no recompute). Wire through the same
  open/re-root path the lineage chain already uses for table nodes.
- Multiple charts on one source all attach as sibling descendants.

`LineageGraph::build`'s signature gains the saved charts (e.g. `&[SavedChart]`), threaded
from the same place the catalog/inspector refresh already has session access.

---

## 6. Package round-trip, diff, inspect

### 6.1 Writer / reader

- **writer**: map `session.charts` → `Vec<PackageChart>` (field-for-field) into
  `PackageContents.charts`.
- **reader**: surface `ParsedPackage.charts`; on in-app open, hydrate `session.charts`
  from the package.
- **Replay** (`dat0 replay`) is unchanged: a chart carries `spec` + `source` name only;
  it re-renders **in-app** against the Parquet that replay already materializes. No image
  is stored in the package; the CLI does not render.

### 6.2 `dat0 diff` — charts dimension (5th)

In `diff.rs`, mirroring the query dimension:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChartChange {
    Added,
    Removed,
    SpecChanged { from: String, to: String }, // one-line spec summaries
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChartDelta {
    pub name: String,
    pub change: ChartChange,
}
```

- Add `pub charts: Vec<ChartDelta>` to `PackageDiff`; key by **name** (added / removed /
  changed). `SpecChanged` carries a compact one-line summary of each side
  (e.g. `"bar x=region y=amount"`) rather than the full serialized spec — keeps
  `render_text()` readable, mirrors `SqlChanged`'s from/to shape.
- Update `PackageDiff::is_empty()` to include `charts.is_empty()`.
- Update `render_text()` with a `Charts:` section (`+`/`-`/`~` lines), matching the
  existing Lineage/Schema/Query section style.
- Update the "four orthogonal dimensions" doc comment to **five**.

### 6.3 `dat0 inspect`

Extend the inspect listing to print saved charts (`name · type · source`) alongside the
existing views/queries sections.

---

## 7. Testing & exit criteria

**Engine / pure (cargo test --workspace):**

- `session/migrate`: a v8 session JSON migrates to v9 with `charts == []`; round-trips a
  v9 session with charts intact. Literal-arm dispatch covered.
- `SavedChart` upsert-by-name: new name appends; reused name replaces in place (id stable,
  spec + saved_at refreshed); count invariant.
- `dat0-format` writer → reader round-trip preserves a `PackageChart` field-for-field.
- `dat0 diff` charts dimension: Added, Removed, SpecChanged cases; `is_empty()` false when
  only charts differ; `render_text()` emits the Charts section.
- `inspector/lineage`: a saved chart appears as a descendant of its source table; bare-name
  match holds when `spec.source` is quoted/qualified; multiple charts → multiple sibling
  nodes; a chart whose source table is absent does not crash `build`.

**Manual UAT (headless cannot verify — joins the standing backlog):**

- Save button → name prompt (pre-filled default) → confirm → toast.
- Reopen workspace → saved chart still present and re-renders.
- Inspector lineage shows the Chart node under its source; click reopens it.
- Export `.dat0` in-app → `dat0 inspect` lists the chart → reopen elsewhere round-trips.

**Exit:** all automated tests green; `clippy --workspace --all-targets -D warnings`; `fmt`;
i18n keys present for the Save button + any new labels; manual UAT script written and added
to the owed-UAT backlog doc.

---

## 8. Risk / notes

- **D-025 does not bite.** Charts enter a `.dat0` **only via in-app export**, where
  lineage is preserved; the CLI cannot author charts and only reads them. The cold-CLI
  derived-origin flattening that D-025 describes does not apply to the charts dimension.
- **plotters-text-on-Linux gotcha** (from P9a-1): not re-triggered here — P9a-2 adds no
  new chart **rendering**; it persists specs. The Linux `font-kit` dlopen dep already in
  the tree stands.
- **`ChartSpec` relocation** (§3.3) is the one structural touch with blast radius: moving
  the portable core into `dat0-engine` changes the definition site, though re-exporting
  from `charts::spec` keeps app call sites unchanged. Precedent confirmed against code
  (`Transformation` is engine-owned, reused by `dat0-format`), so this is a firm decision,
  not a gate. Run the full workspace build after the move (T2) — it is the cross-crate
  compile unit for this slice.
- **Lineage signature change** threads `&[SavedChart]` into `LineageGraph::build` — touches
  every `build` caller; small but cross-file, dispatch the build + callers as one compile
  unit (P6b lesson: a shared-signature change is a single compile unit, gate once).

---

## 9. Task sketch (~7, subagent-driven-dev)

| T | Scope |
|---|---|
| T0 | `SavedChart` type + `SessionState.charts` + `SESSION_SCHEMA_VERSION = 9` + `migrate_v8_to_v9` + upsert-by-name + tests |
| T1 | `charts/panel.rs` Save button + name-prompt (reuse P5b) + persist + toast |
| T2 | Relocate portable `ChartSpec`/`ChartType` core into `dat0-engine`, re-export from `charts::spec` (mirrors `Transformation`); add `PackageChart` + `Charts` model to `dat0-format` |
| T3 | writer (`session.charts` → package) + reader (`ParsedPackage.charts` → session) round-trip |
| T4 | `dat0 diff` charts dimension (`ChartChange`/`ChartDelta` + `is_empty` + `render_text`) |
| T5 | `dat0 inspect` charts listing |
| T6 | lineage `NodeKind::Chart` + glyph + inject-as-descendant + click-reopen (build + callers, one compile unit) |

Order: T0 → (T1 ‖ T2) → T3 → (T4 ‖ T5) → T6. Each task: TDD where pure, two-stage review
(spec + code-quality), final integration review. Branch off `main` @ `9d093e0`.
