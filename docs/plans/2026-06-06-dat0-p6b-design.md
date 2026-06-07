# dat0 P6b — Lineage chain (design)

> **P6 split (set in P6a):** P6a = Catalog + Inspector + Profiling (shipped, PR #13 `afac88e`).
> **P6b** (this doc) = Lineage. Brainstormed 2026-06-06 via `superpowers:brainstorming` + visual companion.
> **As-scoped P6b = the per-table lineage chain only.** The workspace DAG promised in the P6a
> split was dropped by user pick (option A) and is deferred (candidate **D-018**).

## Goal

Turn the existing `DerivedOrigin` lineage backbone into a **clickable, full-closure lineage
chain** rendered in the Inspector. This **replaces** the flat "Dependents" section P6a shipped,
**formalizes Sql-reference edges** (P6a matched only `Transform{parent}`; Sql refs were explicitly
deferred), and **closes PD-022** (Inspector staleness on undo/redo + SQL-console grid-bind).

## Decisions (locked in brainstorm)

| # | Decision | Over | Why |
|---|----------|------|-----|
| D1 | **Indented chain in the Inspector** (no workspace DAG) | node-edge DAG / hybrid | User pick (option A). Low-effort, always-on, local context. DAG deferred → D-018. |
| D2 | **`json_serialize_sql` AST walk** for Sql-table edges | regex name-match / `sqlparser` crate | Robust + DuckDB-native + no new dep; handles joins/CTEs. **Spike-gated** (T0); regex fallback if unavailable. |
| D3 | **Full transitive closure** (roots → leaves) | ±1 hop / ±2+expand | Chains in dat0 are shallow (file→import→transform→sql); most informative; collapse/scroll if tall. |
| D4 | **Click node = open + select** (re-roots Inspector) | open-only / re-root-only | One action; re-roots profile+chain for free (Inspector follows selection); matches P6a catalog click-to-open. |
| D5 | **Fold in PD-022 fix** | keep separate | The chain lives in the Inspector and needs the same freshness; cheap; closes the deferral. |
| D6 | **No session-schema bump** | v8→v9 | Chain is derived live from the catalog; nothing new to persist. v8 stays, no migration. |

## Grounded facts (verified live, 2026-06-06)

- `DerivedOrigin = Sql(String) | Transform{parent, ops}` and
  `TableOrigin = File(PathBuf) | Derived(DerivedOrigin) | Attached{alias, source}`
  (`crates/dat0-engine/src/types.rs:100-114`). **The lineage backbone already exists here.**
- Reverse-lineage today: `crates/dat0-app/src/inspector/dependents.rs::dependents_of(table, tables)`
  filters `TableOrigin::Derived(Transform{parent==table})` — **exact-parent only; Sql refs NOT matched**
  (the file's own doc-comment flags this as "P6b formalizes lineage").
- Derived `Sql` tables are **base tables**: `catalog.rs:162` emits `CREATE TABLE <name> AS <sql>` (CTAS).
  → DuckDB's own `duckdb_dependencies()` does **not** track them as depending on their sources.
  Edges must come from dat0's `DerivedOrigin` registry + parsing the stored SQL string. (This is why
  D2 is required and why we cannot lean on a DuckDB dependency catalog.)
- File-imported tables are **VIEWs** with origin `File(path)` (`register/{csv,json,parquet}.rs`
  emit `CREATE OR REPLACE VIEW … read_*`). They are lineage **roots** (parent = the file).
- App seam for live recompute: `WorkspaceShell::recompute_dependents` (`window.rs:1458`) runs off the
  catalog-changed `cx.notify()` path; `set_dependents` writes the Inspector model
  (`inspector/model.rs:80`). P6b extends this seam.
- `get_tables` already filters internal `__dat0_meta%` / system tables, so the surrogate
  `__dat0_rowid` and meta tables never enter the graph.

## Engine — lineage graph builder

New module `crates/dat0-engine/src/lineage.rs` (engine-side so it can run `json_serialize_sql`
against the live connection and be unit-tested without the app):

```
enum LineageNode {
    Table { name: String },               // base/derived table or view
    File  { path: String },               // import root
    External { alias: String, source: String },  // attached / MotherDuck leaf
}

struct Edge { from: NodeId, to: NodeId, label: EdgeKind }   // from feeds to
enum EdgeKind { FileImport, Transform(/*ops summary*/ String), SqlRef }

struct LineageGraph { /* nodes + adjacency both directions */ }

struct Closure { ancestors: Vec<ChainStep>, descendants: Vec<ChainStep> }
// ChainStep = { node, edge_label, depth }, ordered by depth then name.
```

- **Build** `LineageGraph::build(conn, &[TableInfo]) -> Result<LineageGraph>`. Edge derivation per origin:
  - `Transform { parent, ops }` → one edge `parent → child`, label `Transform(<ops summary>)`.
  - `Sql(sql)` → run `json_serialize_sql(<sql>)`, walk the AST, collect every `BASE_TABLE.table_name`
    **minus** names defined in the statement's `cte_map` → one `SqlRef` edge per referenced table that
    exists in the catalog. (Unknown/external names are dropped or shown as `External` if attached.)
  - `File(path)` → a `File` node → table edge, label `FileImport` (root).
  - `Attached { alias, source }` → an `External` leaf node.
- **Closure** `graph.closure(target) -> Closure` — BFS upward (ancestors) and downward (descendants)
  with a **visited-set cycle guard** (defensive; the graph should be a DAG but rename/re-create edge
  cases must not loop). Deterministic ordering.
- **Cost:** one `json_serialize_sql` call per `Sql` table per graph rebuild. Sql tables are few
  (only SQL-console Save-as-Table outputs); negligible. No new crate, no async.

### T0 spike (GATE — mirrors P6a's SUMMARIZE spike)

Confirm on **duckdb-rs 1.4.4** that:
1. `SELECT json_serialize_sql('<multi-table SELECT with a JOIN and a CTE>')` is accepted and returns JSON.
2. Base-table references appear as nodes with `"type":"BASE_TABLE"` carrying `"table_name"`
   (and optionally `"schema_name"`/`"catalog_name"`/`"alias"`).
3. CTE names are distinguishable (under `cte_map`) so they can be excluded.

Expected (from DuckDB docs, **to be verified by the spike**): `{"statements":[{"node":{"type":"SELECT_NODE",
"from_table":{…BASE_TABLE / JOIN / SUBQUERY…}, "cte_map":{…}}}]}`.

**If the spike fails** (function absent or shape unworkable): fall back to **regex name-match** against
known catalog table names (best-effort, false-positive-prone) and flag the degradation in the retro.
This keeps P6b shippable without `json_serialize_sql`.

## App — Inspector lineage chain

- `crates/dat0-app/src/inspector/dependents.rs` → **`inspector/lineage.rs`**: `dependents_of` is replaced
  by chain construction from `LineageGraph::closure`. Descendants now include `SqlRef` children, not just
  `Transform` children.
- `inspector/model.rs`: replace `dependents: Vec<String>` with `lineage: LineageChain` (typed nodes +
  edge labels + depth for ancestors and descendants).
- `inspector/panel.rs`: hand-rolled **indented vertical** render — `ANCESTORS ↑`, the inspected table
  (highlighted), `DESCENDANTS ↓`. GPUI quads/rows (no chart/graph lib), per-node-type glyph
  (table / 📄 file / ☁ external), per-edge label (`feeds (File import)` · `Transform: <ops>` · `used by (Sql ref)`).
  Empty/leaf states degrade gracefully (a base table with no parent shows only itself; no descendants → "— none —").
- **Click a node** → reuse P6a's catalog open-table action: opens/focuses that table's grid tab **and**
  sets it as the active selection. The Inspector follows selection, so profile + chain **re-root** on the
  clicked node automatically (no separate re-root code path).
- `window.rs`: rename `recompute_dependents` → **`recompute_lineage`** (builds the graph + closure for the
  current target), wired to the same catalog-changed seam.

## PD-022 fix (folded in — closes the deferral)

P6a's Inspector refresh fires on forward mutations (edit/paste/cut/delete/rename/reorder/transform-apply
via `route_change`) but **not** on `undo`/`redo` or SQL-console grid-bind, which rebind via
`apply_view_change` with no Inspector hook. P6b hooks `apply_view_change` (or an `on_rebind_complete`
seam) to invalidate the profile cache **and** `recompute_lineage` for the inspected table. Closes PD-022.

## Testing

- **Engine** (`crates/dat0-engine`): `Transform` edge; `Sql` single + **multi-parent (JOIN)** via
  `json_serialize_sql`; CTE-name exclusion; `File` root; `External`/`Attached` leaf; full-closure ordering;
  cycle-guard. The T0 spike ships as a checked-in test asserting the AST shape.
- **App** (`crates/dat0-app`): chain render (normal + empty/leaf); click-to-open re-roots; recompute on
  catalog change; **PD-022** — recompute on undo/redo and SQL-bind.
- **i18n gate:** new keys for the section heading, edge labels, and node-type labels.
- Existing local gate (clippy `-D warnings`, fmt, i18n, full test bins) + CI both platforms.

## Non-goals (explicitly out)

- **Workspace DAG / node-edge graph / pan-zoom / graph layout** → deferred (candidate **D-018**).
- Real charting libs (P9a territory).
- Any change to `compile_view_sql` / the projection or edit pipelines.
- Lineage across MotherDuck server-side views (we show attached objects as `External` leaves; we do
  not parse remote view definitions).

## Trim valve (if too big for one PR)

In order: (1) drop the `External` cross-source node typing → show attached objects as plain leaves;
(2) if the T0 spike slips, ship the **regex name-match** fallback for Sql edges and defer
`json_serialize_sql` to a follow-up. **Never trim:** the chain itself, click-to-open, the PD-022 fix.

## Deferrals touched

- **Closes PD-022** (Inspector staleness on undo/redo + SQL-bind).
- **Opens D-018** (candidate): workspace lineage DAG — node-edge graph with layout, the P6a-split
  remainder dropped from P6b by D1.

## Decisions register (for the retro)

- **CHOSE** Inspector chain **OVER** workspace DAG — user pick; low-effort, always-on, local. *Revisit if* users ask for a whole-workspace picture → D-018.
- **CHOSE** `json_serialize_sql` AST **OVER** regex / `sqlparser` — robust, native, no dep. *Revisit if* the T0 spike fails → regex fallback.
- **CHOSE** full transitive closure **OVER** bounded depth — chains are shallow. *Revisit if* real-world chains prove deep enough to bloat the panel.
- **CHOSE** open+select click **OVER** open-only / re-root-only — re-roots for free. *Revisit if* users want lineage navigation without opening tabs.
- **CHOSE** fold PD-022 **OVER** defer — chain needs freshness regardless.
