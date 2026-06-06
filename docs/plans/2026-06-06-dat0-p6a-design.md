# P6a — Catalog + Inspector + Profiling (design)

> Phase P6 (Catalog + Inspector + Lineage) is split **2 ways**:
> **P6a** = Catalog tree + Inspector (overview, column search, per-column profile, inline charts) + Dependents list.
> **P6b** = Lineage (per-table chain + workspace DAG).
> This document specs **P6a**. P6b gets its own brainstorm + design.
>
> Brainstormed 2026-06-06 (visual-companion session). Spec source: `docs/specs/2026-04-26-dat0-design.md` §P6.

---

## §0. Locked decisions (this brainstorm)

| # | Decision | Over | Why |
|---|----------|------|-----|
| D1 | **2-way split** (P6a inspector half / P6b lineage half) | 3-way / single push | User pick. Isolates lineage/DAG into its own slice. |
| D2 | **Profiling built on DuckDB `SUMMARIZE`** (+ per-string `LENGTH()` agg + per-col top-N `GROUP BY`) | hand-rolled exact / sampling | User pick. Full-scan exact min/max/mean/median/std; only distinct% approximate (HLL); single-pass fits the `<2s` budget. |
| D3 | **Catalog = its own left dock**, separate from the P5c Connections panel (untouched). **Inspector = new right dock.** | catalog absorbs connections / stacked sections | User pick. Connections stays remote/attach mgmt; Catalog is independent nav. |
| D4 | **Hybrid profile lifecycle** — eager `SUMMARIZE` on select + cache(table,epoch) + lazy top-N (read path); per-column slice recompute on writes (write path) | pure-eager / pure-lazy | User pick. Crisp benchable gate + single-scan efficiency; neutralizes lazy's only wins (wide tables, edit-refresh) on the invalidation path. |
| D5 | **Profile target = base table by default + header toggle "Whole table ⇄ View"** (per-tab state) | base-only / view-only | User pick. `<2s` gate measures base path; "what I see" one click away. |
| D6 | **Catalog enumerates attached-DB tables AND closes D-012** — `attach()` records per-table `TableOrigin::Attached{alias,source}` in the origin registry | local-only / enumerate-without-registry | User pick. Most complete catalog; clears long-standing D-012 remainder; per-table origins drive tree icons + (P6b) cross-source lineage. |
| D7 | **Inline charts hand-rolled with GPUI quads/divs — no charting crate** | pull a chart lib | P9a is the real charts phase ("verify integration"). Keep P6a dependency-free. |
| D8 | **PD-021 banner-host folded in as T0** | leave deferred | Everything in P6a surfaces errors/toasts through `error_ux::PENDING`, which nothing drains at runtime today. Fix first. |

---

## §1. Grounded API facts (verified against live code, not recalled)

- `QueryEngine` trait (`crates/dat0-engine/src/trait_def.rs`) has `get_tables() -> Vec<TableInfo>`, `describe_table(name, schema) -> Vec<ColumnInfo>`, `execute`, `execute_paged`, `attach`, `detach`. **No profiling method exists.**
- `TableInfo { name, schema, columns, row_count_estimate, origin }`; `TableOrigin = File(PathBuf) | Derived(DerivedOrigin) | Attached{alias,source}`; `DerivedOrigin = Sql(String) | Transform{parent, ops}` (`crates/dat0-engine/src/types.rs:101-123`). **Lineage parent + transform chain already encoded here** (P6b backbone; P6a uses `parent` for Dependents).
- Per-engine origin registry: `DuckDBEngine.table_origins: Arc<RwLock<HashMap<String, TableOrigin>>>`, populated on `register_file`/`create_table` (D-012 P3a). `attach()` does **not** enumerate attached tables today (D-012 open remainder — **D6 closes this**).
- Shell render (`crates/dat0-app/src/window.rs` ~2933): body row is `flex_row` with optional left Connections dock (`w_64 border_r`, gated by `connections_panel_visible`) + `flex_1` body. New docks slot into this row.
- `error_ux::PENDING` + `drain_pending`: only `#[cfg(test)]` drains it — **no runtime host** (PD-021).
- `SESSION_SCHEMA_VERSION = 7` (`session/mod.rs:48`); migration chain in `session/migrate.rs`. P6a bumps **v7 → v8**.
- View SQL: `compile_view_sql` exists (P4a/P4c) — feeds the "View" profile-target toggle.

**Unverified → T0 spike (gate):** exact `SUMMARIZE` output column names in duckdb-rs 1.4.4, and that `SUMMARIZE (<subquery/SELECT>)` is accepted (for the View toggle).

---

## §2. Architecture

Three new app modules + one engine surface, wired into the existing shell:

```
dat0-engine
  profile.rs (new)      profile_table / profile_query / column_topn / column_length_stats
  duckdb_engine.rs      attach() → enumerate + register Attached origins (D-012 close)

dat0-app
  catalog/   (new)      left-dock tree from get_tables(); search; context menu; persistence
  inspector/ (new)      right-dock profile view; base/view toggle; column cards; dependents
  charts/    (new)      GPUI-native mini histogram / top-N bars / sparkline (no lib)
  error_ux/  (PD-021)   banner host mounted in WorkspaceShell render (drains PENDING)
  window.rs            two new docks in the body row; toggle actions; epoch bump hooks
  session/             v7→v8: catalog tree state + dock visibility
```

Each unit is independently testable: engine profiling is pure SQL→struct mapping; catalog is a pure function of `get_tables()` + UI state; inspector is a pure function of a `TableProfile` + target; charts are pure render fns over numeric/categorical slices.

---

## §3. Engine layer (`dat0-engine`)

New trait methods + `profile.rs`:

```rust
struct ColumnProfile {
    name: String, ty: String,
    null_pct: f64, approx_distinct: u64,   // approx_distinct = HLL, labeled "approx" in UI
    count: u64,
    numeric: Option<NumericStats>,         // min,max,avg,std,q25,median,q75 (from SUMMARIZE)
    length: Option<LengthStats>,           // strings only, lazy: min/max/avg(length(col))
    topn: Option<Vec<(String,u64)>>,       // lazy on expand
    histogram: Option<Vec<(f64,f64,u64)>>, // lazy numeric buckets
}
struct TableProfile { table: String, rows: u64, cols: usize, columns: Vec<ColumnProfile> }

async fn profile_table(&self, name, schema) -> Result<TableProfile>;   // SUMMARIZE <qualified>
async fn profile_query(&self, sql: &str) -> Result<TableProfile>;      // SUMMARIZE (<view sql>)
async fn column_topn(&self, name, col, n) -> Result<Vec<(String,u64)>>;
async fn column_length_stats(&self, name, col) -> Result<LengthStats>;
```

- `profile_table` runs one `SUMMARIZE` and maps each result row → `ColumnProfile` (numeric stats populated; null_pct, approx_distinct, count for all).
- Top-N / length / histogram are **separate lazy queries** (not in the headline `<2s` pass).
- **D-012 close:** `attach(dsn, alias, opts)` after attaching runs `information_schema.tables WHERE table_catalog = alias` (or `duckdb_tables()`), and for each inserts `TableOrigin::Attached{alias, source: dsn}` into `table_origins`. `get_tables()` already joins the registry → attached tables now surface with real origins. `detach()` removes the alias's entries.

---

## §4. Catalog tree (`catalog/`, new left dock)

- Built from `engine.get_tables()`, grouped: **Sources** (File nodes + Attached-DB parent nodes whose children are their tables) / **Tables** (local base) / **Derived** (`DerivedOrigin`).
- New `CatalogToggle` action + `catalog_panel_visible` gate; renders as a left dock in the body `flex_row`, independent of `connections_panel_visible`.
- **Interaction:** single-click node → set Inspector target + (for a table/view) focus-or-open its grid tab; expand/collapse triangles; right-click context menu → open · rename · drop · export (reuse existing actions where present).
- **Search:** token-AND box filters the tree by table name.
- **Persistence:** expand state + selection + `catalog_panel_visible` → `session.json` (v8). Scratch-scoped today; P7 migrates to per-workspace.

---

## §5. Inspector (`inspector/`, new right dock)

- New `InspectorToggle` action + `inspector_panel_visible` gate; right dock in the body `flex_row`. Driven by the current target (catalog selection or active tab).
- **Header:** name · row/col count · origin · **"Whole table ⇄ View" toggle** (per-tab state, persisted v8).
- **Column search:** token-AND over column names.
- **Per-column card:** type · null% · distinct% (approx tag) · numeric stats OR string length stats · inline chart (§6).
- **Dependents** section: tables whose `DerivedOrigin == Transform{parent==current}` (or `Sql` referencing it — name-match best-effort). Live via the existing catalog-changed `cx.notify()`. (Reverse-lineage; the forward lineage view is P6b.)

---

## §6. Inline charts (`charts/`, no lib — D7)

Pure GPUI render fns over a numeric/categorical slice:
- **Mini histogram** (numeric): buckets from a cheap `histogram()` agg or quantile-derived bins → row of `div` quads scaled to max count.
- **Top-N bars** (string/low-card): horizontal bars from `column_topn`.
- **Sparkline header** (numeric): thin trend strip. *(Trim-valve candidate — §10.)*
No charting crate. P9a is the real charts phase and will "verify integration" with these.

---

## §7. Profile lifecycle (Hybrid — D4)

1. Target select → `describe_table` paints the **column skeleton instantly** → fire `profile_table` (**superseding** any in-flight, reusing the P4a view-change supersede pattern) → cache `TableProfile` under `(table, epoch)`.
2. **Lazy:** top-N + length + histogram fire on column expand; cached same epoch.
3. **Epoch:** per-table counter bumped by transform-apply / edit-commit / rebuild / drop.
4. **Hybrid write path:** a cell-edit / single-column transform recomputes only that column's slice (re-runs that column's aggregates), not the whole `SUMMARIZE`. Structural changes (schema change / rebuild) bump the whole epoch.
5. **View toggle** → `profile_query(compile_view_sql())`, cached under a separate key; invalidated on view-state change.

---

## §8. PD-021 banner host (T0 — D8)

Mount a banner host element in the `WorkspaceShell` render tree (e.g. top of the shell `flex_col`, above the tab strip) that calls `error_ux::drain_pending()` and renders the resulting `Banner`s. Fixes invisible export/paste/Save-as-Table toasts (P4b/P4c/P5b debt). Closes PD-021.

---

## §9. Data flow

```
catalog click ─► set target ─► describe(skeleton) + profile_table(supersede)
                                   └─► cache(table,epoch) ─► inspector render
expand column ─► topn/len/histogram (lazy) ─► cache(same epoch) ─► card render
mutation ─► epoch++ (or column-slice recompute) ─► inspector + dependents re-render via notify
view toggle ─► profile_query(compile_view_sql) ─► separate cache key
```

---

## §10. Testing / exit criteria

- **Engine:** `profile_table` maps `SUMMARIZE` correctly against a fixture (numeric + string cols); `column_topn`/`column_length_stats` aggregates; **D-012:** `attach` enumerates + records `Attached` origins, `detach` removes them (`tests/catalog_origin.rs`).
- **Bench (the exit gate):** 1M-row base-table `profile_table` **< 2s** — hosted-runner bench, same harness as P4a/P4c.
- **App:** catalog tree builds from origins (incl. attached); single-click drives inspector; dependents live-update on `create_table`; tree state + dock visibility round-trip session v8; banner host drains `PENDING` (PD-021 regression).
- **Exit criteria covered (spec §P6 subset):** catalog shows all open tables + persists · inspector profiles 1M-row table in < 2s · dependents update live as transforms apply.
- **Deferred to P6b (spec §P6 remainder):** per-table lineage view · workspace DAG · node-click-selects-tab.

---

## §11. Trim valve (if the slice won't fit one clean PR/CI/review)

In order: (1) drop the **base/view toggle** → base-only (defer toggle to P6b); (2) drop the **sparkline header** (keep histogram + top-N); (3) split **D-012 attach-enumeration** into a P6a.1 engine-only PR.
**Never trim:** catalog tree · base-table profiling · the `<2s` gate · PD-021 banner host.

---

## §12. Non-goals (explicit — out of P6a)

- Per-table lineage view, workspace DAG, dependents-as-graph (→ P6b).
- A real charting library / configurable charts (→ P9a).
- Per-workspace catalog persistence (→ P7; P6a persists in Scratch `session.json`).
- View-state profiling as default (base is default; view is the toggle).
- Editing/altering attached (MotherDuck) tables from the catalog (read + inspect only).

---

## §13. Spikes (T0 — gate the slice)

- **S1:** `SUMMARIZE <table>` output column names/types in duckdb-rs 1.4.4 + `SUMMARIZE (<SELECT…>)` accepted for the View toggle. (Gates §3/§5/§7.)
- **S2:** banner-host drain renders a `Banner` in the running shell (gates §8 / the whole error-surface story).
- **S3:** `attach()` → `information_schema.tables` enumeration returns attached-catalog tables (gates D-012 close §3).

---

## §14. Deferral register updates (on execution)

- **D-012** → CLOSED (P6a) — attach enumeration records per-table `Attached` origins.
- **PD-021** → CLOSED (P6a T0) — banner host mounted.
- Open a new deferral if the trim valve fires (e.g. **D-016** base/view toggle → P6b; or **D-017** attach-enumeration → P6a.1).

---

## §15. Open questions for plan phase

- Histogram binning source: dedicated `histogram()` agg vs quantile-derived bins (S1 informs).
- Dependents for `DerivedOrigin::Sql(sql)` — name-match heuristic vs only `Transform{parent}` exact (P6b will formalize; P6a best-effort).
- Context-menu action reuse: which of open/rename/drop/export already have actions vs need new ones.
- Right-dock default width + whether catalog + inspector + connections can all be open at once (3 docks) without crushing the grid — UAT.
