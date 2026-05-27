# dat0 P4a — View state + Filter + Sort hot path (brainstorm-output design)

**Authored:** 2026-05-27
**Phase:** P4a (first of a three-way split of spec §21.2 P4)
**Status:** Design approved; ready for `/gsd:plan-phase` (or hand-authored plan).
**Inputs:**
- Spec `docs/specs/2026-04-26-dat0-design.md` §6, §8.2, §9, §21.2 P4 row
- Deferral register `docs/deferrals.md` (D-008 / D-014 / PD-002 / PD-011 / PD-012 referenced)
- P3a retro `docs/plans/2026-05-16-dat0-p3a-retro.md`
- P3b retro `docs/plans/2026-05-25-dat0-p3b-retro.md` (§"Recommendations for P4")
- Current engine surface: `crates/dat0-engine/src/{trait_def.rs,types.rs,duckdb_engine.rs,render.rs(new)}`
- Current grid surface: `crates/dat0-app/src/grid/{mod.rs,data_source.rs}`

---

## 0. Phase decomposition (locked)

P4 in the spec is one ~4-week phase covering 8 transformation types + selection + clipboard + bulk ops + PipelineBar + export dialog. P3 followed an identical "too big for one phase" shape and was split mid-flight (P3a hot path + P3b polish). P4 starts pre-split into three sub-phases:

| Sub-phase | Scope (one-liner) | Hard exit gate |
|---|---|---|
| **P4a** | Typed Transformation model + ViewModel + Filter + Sort + compact-inline filter popover + multi-column sort headers + session.json v2 + undo/redo | 1M-row Filter < 500 ms p95 (spec §21.2 P4) |
| **P4b** | Inline cell editing + row/range/multi selection + TSV clipboard + fill-down + bulk ops + context menu + selection-aware a11y | TSV round-trips Excel + Sheets; selection passes a11y baseline |
| **P4c** | Column reorder / rename / delete + RowDelete + PipelineBar (collapsed pill strip + expanded vertical timeline) + export-to-file dialog | PipelineBar both modes functional; export valid for current-view + full-table scopes |

Each sub-phase ships its own PR and retro, mirroring the P3a/P3b cadence.

This document is P4a only.

---

## 1. Locked decisions (from brainstorm clarifying loop)

| # | Decision | Rationale |
|---|---|---|
| 1 | Three-way split P4a → P4b → P4c | P4 spec scope > P3 (which was already split); subagent dispatch budget + CI saga risk |
| 2 | Active transforms execute via **DuckDB temp VIEW per chain change** | Single SQL gen per chain edit (not per scroll); leverages query planner once; clean "Save as Table" path (CTAS from view at P5); preserves engine-as-source-of-truth for SQL |
| 3 | Undo / redo = **per-tab `Vec<Transformation>` + cursor**, history cap 200 | Matches editor norm (Cmd+Z scoped to focused tab); cheap (transforms are tiny); aligns with PipelineBar's eventual need for typed op chain (P4c) |
| 4 | `dat0-engine` owns typed `Transformation` enum + SQL render | Single source of SQL truth; lineage round-trip lossless (forced for P7 `transforms.jsonl` + P8 `.dat0` bundle anyway) |
| 5 | Full type-aware filter set + **IN-list + regex** | Matches Airtable/Excel power-user expectations; IN distinct-values panel ≤ 50 + manual-entry fallback |
| 6 | Persistence via `session.json` schema v2 with explicit `schema_version: u32` + forward-migration fn | Reuses P3a crash-recovery vehicle; Transformation serde is forced by P7/P8 regardless |
| 7 | T0 perf spike on day 1, blocks T1+ | 1M-filter < 500 ms is the only hard P4 perf number; mis-estimate would force mid-phase rewrite |
| 8 | Filter popover = **compact inline** (operator dropdown + inline value(s)) | Smaller surface, power-user-friendly; option B (tabbed) rejected as wider + more chrome |

---

## 2. Architecture overview

```
dat0-engine (Rust, no UI deps)
├── transform.rs                  ← Transformation, FilterOp, FilterValue, Scalar, SortKey, SortDirection
├── render.rs                     ← compile_view_sql(base, &[Transformation]) -> Result<String, RenderError>
├── types::DerivedOrigin::Transform { parent, ops: Vec<Transformation> }   ← upgraded from Vec<String>
└── QueryEngine trait
    ├── create_or_replace_view(name, sql) -> Result<()>     ← new
    ├── drop_view(name) -> Result<()>                       ← new
    └── (existing) execute, execute_paged, execute_streaming

dat0-app (GPUI)
├── view/mod.rs
│   ├── model.rs       ← ViewModel { tab_id, base_table, stack, cursor, active_view, nonce_seq }
│   │                    apply / undo / redo / clear / replace_at_cursor / set_sort
│   ├── filter_popover.rs    ← Entity<FilterPopover>, compact-inline layout
│   └── sort_header.rs       ← header sort-zone click cycle + shift-click multi-sort
├── grid/                ← unchanged structurally; data_source.rebind(view_name) on stack change
├── session/migrate.rs   ← new: load_and_migrate(path) -> SessionState; v1→v2 inline-write
└── actions             ← view.undo / view.redo registered in ActionRegistry (P3b T3)
```

### Data flow on `ViewModel::apply(t)`

1. (main thread) Truncate `stack[cursor..]`, push `t`, `cursor++`, bump `nonce_seq`
2. (main) Compute new SQL: `engine::render::compile_view_sql(base_table, &stack[..cursor])`
3. (tokio worker) `engine.create_or_replace_view(active_view_name, sql).await`
4. (main, via `MainThreadDispatcher`) Rebind `GridDataSource` to `active_view_name`, invalidate page LRU, request render; drop previous view (if any)
5. (debounced ~250 ms via P3a write path) Serialize ViewModel state into `session.json` v2

### Threading discipline

- Stack mutations + SQL render — main thread (cheap, deterministic)
- View create/replace/drop — tokio worker (DuckDB I/O); result posted back via `MainThreadDispatcher` (P3b T1)
- Cancellation — existing `engine.interrupt(handle)` on supersede (matches P3b T10 import-cancel pattern); D-008 structured CancellationToken stays P5

### Reuse summary

Every P4a UI moving part rides primitives already shipped: `GridDataSource` + LRU (P3a), `MainThreadDispatcher` (P3b T1), `Banner` shape (P3b T2), `ActionRegistry` (P3b T3), `Theme` (P3b T12), `SessionStore` atomic-write (P3a). No new infrastructure crates.

---

## 3. Typed `Transformation` enum + serde

**Location:** `dat0-engine/src/transform.rs` (new module, re-exported from `lib.rs`).

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transformation {
    Filter { column: String, op: FilterOp, value: FilterValue },
    Sort   { keys: Vec<SortKey> },
    // P4b will add: Edit, RowDelete
    // P4c will add: Reorder, Rename, Delete (column)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq, Neq, Lt, Lte, Gt, Gte,
    Between,                  // value = Range { lo, hi, inclusive }
    Contains, NotContains, StartsWith, EndsWith,
    In,                       // value = List(Vec<Scalar>)
    Regex,
    IsEmpty, IsNotEmpty,
    IsTrue, IsFalse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterValue {
    Scalar(Scalar),
    Range { lo: Scalar, hi: Scalar, inclusive: bool },
    List(Vec<Scalar>),
    None,                     // for IsEmpty / IsNotEmpty / IsTrue / IsFalse
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Date(String),             // ISO-8601 yyyy-mm-dd, validated at parse
    Timestamp(String),        // ISO-8601 RFC 3339, validated at parse
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortKey {
    pub column: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection { Asc, Desc }
```

**Design notes:**
- `#[serde(tag = "kind")]` on the outer enum gives self-describing JSONL rows — same shape that P7 `.dat0/lineage/transforms.jsonl` and P8 `.dat0` format will consume directly
- `Scalar` is a sum type (not `serde_json::Value`) — keeps DuckDB type literals explicit + render predictable
- `FilterValue::None` covers nullary ops without `Option` field on every variant
- Engine does **not** validate column existence or type compatibility at construction — that's the render-time gate (`compile_view_sql` returns `Result<String, RenderError>`)

**Replacement of placeholder:** `types::DerivedOrigin::Transform { parent: String, ops: Vec<String> }` → `{ parent: String, ops: Vec<Transformation> }`. Single grep + replace; no callers serialize `ops` today (P2/P3 only wrote `Sql(String)` for derived tables).

**Dep impact:** `serde = "1"` + `serde_json = "1"` already in workspace via settings + sample_data; no top-level dep churn.

---

## 4. SQL render + temp VIEW lifecycle

**Module:** `dat0-engine/src/render.rs`.

```rust
pub fn compile_view_sql(
    base: &str,                 // already-quoted table name (e.g. `"main"."orders"`)
    ops: &[Transformation],
) -> Result<String, RenderError>;
```

### Pipeline

Filters fold into one `WHERE` clause (`AND`-joined). Sort folds into one `ORDER BY` (last-`Sort`-wins per stack semantics — UI prevents but render is defensive).

Example output:

```sql
SELECT * FROM "main"."orders"
WHERE  ("price" >= 10.00 AND "price" <= 99.99)
  AND  ("city" IN ('SF', 'NYC', 'LA'))
  AND  regexp_matches("name", '^A.*')
ORDER BY "city" ASC, "price" DESC
```

### Identifier + value handling

- All column references → `dat0-engine::ident::quote_ident` (P2 I-1 helper). No raw concat in render path.
- `Scalar::Str` → single-quote escaped (`'` → `''`)
- `Scalar::Date` → DuckDB literal `DATE 'yyyy-mm-dd'`; `Scalar::Timestamp` → `TIMESTAMP 'yyyy-mm-dd hh:mm:ss[.fff]'`
- `Scalar::Float` → `{:?}` debug format (precise round-trip), not `{}`
- `Scalar::Null` → unwrap to `IS NULL` / `IS NOT NULL` at op level, never `= NULL`

### `RenderError` variants

`EmptyInList` · `InvalidRegex` (compile-checked via `regex` crate before emit) · `MismatchedRange` · `UnsupportedOpForType` (defensive last gate; UI shouldn't allow).

### Engine surface extension

```rust
// dat0-engine trait additions
async fn create_or_replace_view(&self, name: &str, sql: &str) -> Result<()>;
async fn drop_view(&self, name: &str) -> Result<()>;
```

Impl = `CREATE OR REPLACE TEMP VIEW {name} AS {sql}` + `DROP VIEW IF EXISTS {name}`. `TEMP` scopes the view to the connection — gone when engine handle drops.

### View naming + lifecycle

- Name format: `v_tab{tab_id}_{nonce}`. Nonce regenerated per `apply` so concurrent readers of the old view name can't see torn state (DuckDB `CREATE OR REPLACE` is atomic, but page LRU may hold stale fetches).
- Old view dropped after rebind completes (via `ViewChange::previous_active_view`).
- Empty stack (`cursor == 0`) — no view exists; `GridDataSource` binds directly to base table. `apply` from empty creates view; `undo` to empty drops it.

---

## 5. Per-tab undo / redo + `ViewModel` API

**Module:** `dat0-app/src/view/model.rs`.

```rust
pub struct ViewModel {
    pub tab_id: TabId,
    pub base_table: String,
    pub stack: Vec<Transformation>,
    pub cursor: usize,                  // active ops = stack[..cursor]
    pub active_view: Option<String>,    // None when cursor == 0
    nonce_seq: u32,
}

impl ViewModel {
    pub fn new(tab_id: TabId, base_table: String) -> Self;

    // Mutators (caller awaits engine round-trip on returned ViewChange)
    pub fn apply(&mut self, t: Transformation) -> ViewChange;
    pub fn undo(&mut self) -> Option<ViewChange>;
    pub fn redo(&mut self) -> Option<ViewChange>;
    pub fn clear(&mut self) -> ViewChange;
    pub fn replace_at_cursor(&mut self, t: Transformation) -> ViewChange;
    pub fn set_sort(&mut self, keys: Vec<SortKey>) -> ViewChange;  // upsert-or-append

    // Queries
    pub fn active(&self) -> &[Transformation];
    pub fn can_undo(&self) -> bool { self.cursor > 0 }
    pub fn can_redo(&self) -> bool { self.cursor < self.stack.len() }
    pub fn current_filters(&self) -> impl Iterator<Item = (&str, &Transformation)>;
    pub fn current_sort(&self) -> Option<&[SortKey]>;
}

pub struct ViewChange {
    pub new_active_view: Option<String>,    // None = bind to base
    pub previous_active_view: Option<String>,
    pub sql: Option<String>,
}
```

### Semantics

- `apply(t)` — truncate `stack[cursor..]` (lost-redo on new branch), push `t`, `cursor++`, bump nonce, compute new view name + SQL
- `replace_at_cursor` — replaces `stack[cursor - 1]` in place (no new history entry); used by filter-popover edit + by `set_sort` when an existing Sort op is present
- `set_sort` — if a `Sort` op exists in `stack[..cursor]`, locate + replace in place; else append via `apply`
- `clear` — truncates to empty (`cursor = 0`), drops `active_view`; one undo restores
- `undo` / `redo` past edges = `None` (silent no-op at action layer)

### Action registry wiring

- `view.undo` and `view.redo` registered in P3b `ActionRegistry`
- Keybinds: `Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z`
- Dispatch grabs the **focused tab's** ViewModel (focused-tab tracking lives in `WorkspaceShell`), invokes mutator, awaits engine round-trip, posts rebind via `MainThreadDispatcher`

### History cap

`view::HISTORY_CAP = 200`. On `apply` past the cap, drop `stack[0]` and decrement `cursor`. Guards against pathological op loops without surprising real analyst sessions.

### Scroll restoration

`ViewChange` does **not** carry scroll position. Grid resets scroll-to-top on rebind (Excel/Sheets norm). Revisit at P4c only if user feedback demands.

---

## 6. Filter popover UI + four-zone column header wiring

### Header zones (per spec §6)

```
┌──────────────────────────────────────────────┐
│ ⋮⋮  price         ⌃        🔽       ▽         │
│ ↑   ↑             ↑        ↑         ↑        │
│grip click-body  type    sort       funnel    │
│(P4c)  (P4b)    badge   (P4a)       (P4a)     │
└──────────────────────────────────────────────┘
```

- **Grip + click-body** capture pointer-down but no-op in P4a (zones reserved for P4c / P4b)
- **Sort zone** — click cycles `none → asc → desc → none`; Shift+Click adds secondary (does not clear primary). Visual: filled ▲/▼ when active; subscript number for rank in multi-sort
- **Funnel zone** — click toggles popover; filled icon = active filter present

### Filter popover entity

`dat0-app/src/view/filter_popover.rs` — GPUI `Entity<FilterPopover>` anchored to funnel icon's screen rect. Spawned on funnel click; dismissed on Apply / Cancel / outside-click / Esc.

### Compact-inline layout (option A — locked)

- **Title row** — `Filter: <column>` + type badge (right, muted)
- **Operator dropdown** — ops filtered to column type via `SUPPORTED_OPS_FOR` table:
  - `NUMERIC` → Eq, Neq, Lt, Lte, Gt, Gte, Between, In, IsEmpty, IsNotEmpty
  - `STRING` → Eq, Neq, Contains, NotContains, StartsWith, EndsWith, In, Regex, IsEmpty, IsNotEmpty
  - `DATE` / `TIMESTAMP` → Eq, Neq, Lt, Lte, Gt, Gte, Between, In, IsEmpty, IsNotEmpty
  - `BOOL` → IsTrue, IsFalse, IsEmpty
- **Value field(s)** — rendered per op shape:
  - Single-value ops → one type-appropriate input (number-input / date-picker / text / checkbox)
  - `Between` → two inputs + " and " separator
  - `In` → dropdown trigger → distinct-values panel (see below)
  - `Regex` → text input (monospace) with inline validity dot (green / red), updated on input via `regex::Regex::new` compile-check
  - Nullary ops → no value field
- **Footer** — `Clear` (left, dim) · `Cancel` + `Apply` (right). Apply disabled until value is non-empty + valid.

### IN-list distinct-values panel

- On `In` selected, fetch top-50 distinct via `SELECT col, COUNT(*) FROM base GROUP BY col ORDER BY 2 DESC LIMIT 50` (async, debounced 150 ms)
- Panel: scrollable checkboxes (value + count); selected values collected into `FilterValue::List`
- Manual-entry text field (`"+ add value..."`) at bottom for off-top-50 values
- Banner when distinct-count > 50: "Showing 50 of N distinct values; type to add others."

### Edit-existing-filter flow

- Funnel click on a column with an existing filter → popover opens **pre-populated**
- Apply → `ViewModel::replace_at_cursor` (single undo step, history clean)
- Clear → remove the op from `stack[..cursor]` at its position; shift later ops down; decrement cursor by 1 if cursor was past it

### gpui-component primitive risk

P3b D-001 closed predicate state + closures but left the visible `Input` / `Select` mount stubbed. P4a depends on those primitives rendering for the filter popover to be functional. **T0 spike must verify `Input` + `Select` headless mount.** If still infeasible:
- Pull the primitive-mount task into P4a explicitly (preferred — a phase exit that ships a stubbed filter popover is not a real exit)
- Or document parallel deferral (D-NNN) matching D-001 closure shape

---

## 7. Multi-column sort UI

### Visual states (sort zone, header right edge, left of funnel)

- `▽` hollow — no sort on this column
- `▲` / `▼` filled — primary sort
- `▲₂` / `▼₂` filled + subscript — secondary / tertiary; subscript = 1-based rank

### Click semantics

| Action | Behavior |
|---|---|
| Click (no shift) | Replace entire sort: this column becomes sole sort. Cycle `none → asc → desc → none`. |
| Shift+Click | Append/cycle within existing sort. Absent → appended at next rank (asc). Present → cycle `asc → desc → remove` within current rank. |

Removing a non-last rank shifts later ranks up.

### ViewModel mapping

Sort is **one** `Transformation::Sort { keys: Vec<SortKey> }` op in the stack, not one per column. UI calls `ViewModel::set_sort(keys)` — upserts an existing Sort or appends a new one. Single `ORDER BY` clause in render (last-Sort-wins defensive rule already in §4). One undo reverts the whole sort change.

If finer-grained per-key undo is later requested, document as v1.x trade-off.

### Keyboard

No P4a hotkey for sort (column-header driven). `Cmd+Shift+S` ("sort by selected column") reserved for P4b once selection model lands.

---

## 8. session.json schema v2 + forward migration

### Current state (P3a)

```jsonc
{ "tabs": [{ "id": "tab_7", "table_name": "orders", ... }], "active_tab": "tab_7" }
```

No `schema_version` field. No transform-related fields.

### Schema v2 shape

```jsonc
{
  "schema_version": 2,
  "tabs": [
    {
      "id": "tab_7",
      "table_name": "orders",
      // NEW in v2:
      "transform_stack": [
        { "kind": "filter", "column": "price",
          "op": "between",
          "value": { "lo": 10.00, "hi": 99.99, "inclusive": true } },
        { "kind": "sort",
          "keys": [{ "column": "city", "direction": "asc" }] }
      ],
      "undo_cursor": 2
    }
  ],
  "active_tab": "tab_7"
}
```

### Migration vehicle

`dat0-app/src/session/migrate.rs`:

```rust
pub fn load_and_migrate(path: &Path) -> Result<SessionState, SessionLoadError> {
    let raw = fs::read_to_string(path)?;
    let probe: serde_json::Value = serde_json::from_str(&raw)?;
    let version = probe.get("schema_version").and_then(|v| v.as_u64()).unwrap_or(1);
    match version {
        1 => migrate_v1_to_v2(probe),
        2 => Ok(serde_json::from_value(probe)?),
        n => Err(SessionLoadError::UnsupportedVersion(n)),
    }
}
```

### Rules

1. **Missing `schema_version` ⇒ assume v1** (P3a wrote no version field; must remain readable).
2. **Forward-incompat is hard error**, not silent best-effort. Surface Banner: "Session from a newer dat0 version (vN). Open with that version or discard." with `Discard` / `Open Empty` actions via P3b Banner shape.
3. **Migration is single-shot + write-back.** Successful v1→v2 immediately writes v2 via P3a atomic-write. Next load is pure v2 deserialize.
4. **Migration never loses data.** Unknown v1 fields preserved verbatim via `#[serde(flatten)] extra: serde_json::Map`.

### Write side

`SessionStore::save(&state)` always writes `schema_version: 2` going forward. No conditional path — eliminates "did we forget to set the version" failure mode.

### Pre-existing risk to bundle-fix

P3a session.json atomic-write writes-then-renames without `fsync` of the parent dir. PD-002 is the same shape (for settings.toml). P4a adds more state to the file → bundle a one-line fsync-parent-dir fix into P4a T-late. Doesn't change design; flagging for the plan.

---

## 9. T0 spike — 1M-filter < 500 ms validation

**Day 1, blocks T1+.** Output → `docs/internal/dat0-p4a-t0-spike.md`.

### Spike surface

1. **Fixture** — extend `dat0-fixtures` with `gen_filter_fixture(rows = 1_000_000, seed: u64) -> PathBuf`. Schema: `id INT, price DOUBLE, city VARCHAR (50 distinct), ts TIMESTAMP, active BOOL`. ~30-50 MB. Cached via P3 CI fixture cache.
2. **Bench** — new Criterion bench `dat0-engine/benches/view_regen.rs`:
   - `t1`: `CREATE OR REPLACE TEMP VIEW v AS SELECT * FROM t WHERE price > 5000`
   - `t2`: `execute_paged("SELECT * FROM v", 0, 100)`
   - `t3`: `execute_paged("SELECT * FROM v", 999_900, 100)`
   - **Hot-path metric: `t1 + t2`** (perceived latency Apply-click → first row paint)
   - Targets: `t1 + t2` < 500 ms p95 on macOS-arm + linux-x86; `t1 + t2 + t3` < 2 s as full-materialization sanity check
3. **Render path validation** — exercise `compile_view_sql` for representative ops (between, IN top-50, regex, multi-key sort); assert generated SQL parses + executes without DuckDB warnings.
4. **gpui-component `Input` + `Select` mount probe** — record headless mount feasibility (§6 risk). If infeasible, recommend bench-artifact step (matches P3a `grid_scroll` macOS pattern).

### Plan B triggers

If `t1 + t2` exceeds 500 ms p95 on either platform, spike doc must document:
- **B1 — Materialized CTAS-on-apply.** `CREATE OR REPLACE TEMP TABLE v_... AS SELECT ...`. Heavier on apply, cheaper on paging.
- **B2 — Hybrid.** View < N rows (threshold from spike data), CTAS above. Row-count heuristic in `create_or_replace_view`.
- **B3 — Prepared-statement reuse.** If overhead is parse/plan dominated. Lower confidence; defer unless B1/B2 don't close gap.

### Exit criteria

- One of {View, CTAS, Hybrid} hits 500 ms p95 on both platforms with documented bench
- §4 is amended in place if Plan B wins
- Spike doc committed before T1 starts; cited by every subsequent task that touches engine view ops

**Effort:** 1 dev-day. Matches P2 T0 / P3a T0 / P3b T0 cadence.

---

## 10. Testing strategy + exit criteria

### Test surfaces (cumulative target ≈ +60 over P3b baseline)

| Suite | Location | What | Count |
|---|---|---|---|
| Transformation serde | `dat0-engine/tests/transformation_serde.rs` | Round-trip per variant + nested cases | ~12 |
| Render golden | `dat0-engine/tests/render.rs` | `Transformation` → SQL golden strings; identifier-quote audit | ~25 |
| Render errors | `dat0-engine/tests/render_errors.rs` | EmptyInList, InvalidRegex, MismatchedRange, UnsupportedOpForType | ~6 |
| Temp-view lifecycle | `dat0-engine/tests/temp_view_lifecycle.rs` | create / replace / drop / re-create / `IF EXISTS` against real DuckDB | ~8 |
| ViewModel logic | `dat0-app/tests/view_model.rs` | Cursor math, branch truncation, replace_at_cursor, history cap, can_undo/redo | ~15 |
| View lifecycle integration | `dat0-app/tests/view_lifecycle.rs` | ViewModel ↔ engine end-to-end | ~6 |
| Filter popover state | `dat0-app/tests/filter_popover_state.rs` | Op-set per type, validity gating, pre-population, in-list debounce | ~10 |
| Sort header state | `dat0-app/tests/sort_state.rs` | Click cycle, shift-click append/cycle/remove, rank shift, set_sort upsert | ~10 |
| Session migration | `dat0-app/tests/session_migration.rs` | v1→v2, v2 round-trip, forward-incompat banner, unknown-fields preserved | ~8 |
| **Heavy / perf** | `dat0-engine/benches/view_regen.rs` | T0 spike bench, gated `heavy.yml` | (bench) |

Pure-logic suites run per-PR via `ci.yml`. Heavy bench gated to `heavy.yml` per P3-era cost remediation. Filter-popover render-verification rides the macOS bench-artifact path if headless mount stays infeasible.

### CI gates added in P4a

1. `no-raw-ident-concat` grep gate — **conditional** (only if T0 spike or T1 review catches a render-path slip). Default: skip.
2. `no-arrow-standalone` — existing, no change. Render emits SQL strings, not RecordBatch.
3. Bench job — `view_regen` artifact uploaded on `heavy.yml`; comparison-vs-baseline note in PR body (manual until P9 perf-budget runner lands).

### P4a exit criteria (spec §21.2 P4 carve-out)

1. `Transformation` typed enum lands in `dat0-engine`; `DerivedOrigin::Transform.ops` upgraded to `Vec<Transformation>`; all serde round-trips green.
2. `compile_view_sql` renders Filter + Sort variants correctly; identifier-quote audit shows no raw concat in render path.
3. `engine.create_or_replace_view` + `drop_view` work end-to-end against real DuckDB; lifecycle integration tests green.
4. ViewModel: apply / undo / redo / clear / replace_at_cursor / set_sort work per spec; history cap honored; pure-logic + integration suites green.
5. Filter popover (compact-inline) mounts on funnel click; full op surface available per type; in-list distinct-values panel works against a real engine; regex inline-validity feedback works.
6. Sort header cycles + shift-click multi-sort work; rank subscripts render correctly.
7. session.json v2 schema migration: v1 fixtures load cleanly + write back as v2; forward-incompat surfaces a Banner not a crash; unknown-fields preserved; pre-existing fsync gap closed.
8. **1M-row Filter < 500 ms p95 on macOS-arm and linux-x86** (T0 spike outcome → Plan A or documented Plan B fallback).
9. Cmd/Ctrl+Z / Cmd/Ctrl+Shift+Z bound + dispatched via `ActionRegistry`; focused-tab routing correct in multi-tab session.
10. Crash + reload restores active transform stack + undo cursor + active view rebind.

### Items intentionally NOT in P4a (carried to P4b / P4c)

- Inline cell editing, selection model, clipboard, fill-down, context menu, bulk ops, column reorder/rename/delete, RowDelete, PipelineBar (any mode), export dialog, "Save as Table".
- D-008 structured CancellationToken — stays P5 per P3b retro rec #5; P4a uses existing `engine.interrupt(handle)` on supersede.
- Spec §3.7 PD-011 cleanup — independent of P4a, retro-hygiene queue.

### Deferral / plan-defect candidates likely opened during P4a

- If T0 spike picks Plan B, possible new D-NNN for "explicit view-vs-CTAS strategy switch" if user-visible
- If gpui-component `Input`/`Select` headless mount stays infeasible, parallel D entry mirroring D-001 closure

---

## 11. Handoff

Approved design. Next step: hand-author or `/gsd:plan-phase` a TDD task plan keyed to the §10 exit criteria, starting with T0 (the spike day-1). Tasks expected to land roughly:

- **T0** — spike (this doc § 9)
- **T1** — `dat0-engine/src/transform.rs` typed enum + serde round-trips
- **T2** — `dat0-engine/src/render.rs` Filter + Sort render + golden tests
- **T3** — `DerivedOrigin::Transform` upgrade + ops-field type change
- **T4** — `engine.create_or_replace_view` + `drop_view` + temp-view lifecycle tests
- **T5** — `view::ViewModel` API + pure-logic tests
- **T6** — `view::ViewModel` ↔ engine integration tests
- **T7** — `view.undo` / `view.redo` action registration + keybinds
- **T8** — `session.json` v2 schema + `migrate.rs` + tests + fsync-parent-dir fix
- **T9** — Four-zone header wiring (funnel + sort zones live, grip + body stubs)
- **T10** — Filter popover (compact inline) — operator dropdown + value fields + Apply/Cancel/Clear + edit-existing flow
- **T11** — IN-list distinct-values panel (async fetch + debounce + manual entry + banner)
- **T12** — Multi-column sort header (click cycle + shift-click append + rank subscripts + set_sort upsert)
- **T13** — `GridDataSource` rebind on `ViewChange` + LRU invalidation + supersede-cancel via `engine.interrupt`
- **T14** — End-to-end test: apply → undo → redo → clear → crash → reload restore
- **T15** — Retro + heavy bench update + deferral register update

Task count + ordering will firm up in the plan; this list is the brainstorm-output skeleton.
