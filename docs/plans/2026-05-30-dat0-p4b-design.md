# dat0 P4b — Edit + Selection + Clipboard (brainstorm-output design)

**Authored:** 2026-05-30
**Phase:** P4b (second of the three-way split of spec §21.2 P4)
**Status:** Design approved; ready for `/gsd:plan-phase` (or hand-authored plan).
**Inputs:**
- Spec `docs/specs/2026-04-26-dat0-design.md` §8.2 (DataGrid), §9 (lineage), §21.2 P4 row
- P4a design `docs/plans/2026-05-27-dat0-p4a-design.md` (§0 split table, §3 Transformation, §5 ViewModel)
- P4a retro `docs/plans/2026-05-27-dat0-p4a-retro.md` (§"PD-016 decision" hand-off contract; §"Recommendations for P4b")
- Deferral register `docs/deferrals.md` (PD-016 / PD-002 / PD-011 referenced)
- Current engine surface as-merged at `79a86a9`: `dat0-engine/src/{transform.rs,render.rs,catalog.rs,duckdb_engine.rs,trait_def.rs}`
- Current app surface as-merged at `79a86a9`: `dat0-app/src/{view/{mod,model,filter_popover_entity,sort_header}.rs,grid/{mod,data_source}.rs,session/{mod,migrate}.rs,actions/}`

---

## 0. Position in the P4 split (recap)

P4 was pre-split three ways in the P4a design (§0). P4a shipped the engine/view-state hot path (PR #7, `79a86a9`). This document is **P4b**.

| Sub-phase | Scope (one-liner) | Hard exit gate |
|---|---|---|
| P4a ✅ | Typed `Transformation` + ViewModel + Filter + Sort + filter popover + multi-sort + session v2 + undo/redo | 1M Filter < 500 ms p95 — **met** (229 ms arm / 295 ms x86) |
| **P4b** (this) | Inline cell edit + row/range/multi selection + TSV clipboard + fill-down + bulk ops + context menu + selection-aware a11y | TSV round-trips Excel + Sheets; selection passes a11y baseline |
| P4c | Column reorder/rename/delete + PipelineBar (collapsed + expanded) + export-to-file dialog | PipelineBar both modes; export valid for current-view + full-table |

P4b also **discharges PD-016** — the UI-click → ViewChange wirings P4a's plan T13 left unowned (P4a retro §"PD-016 decision", Path B, explicit hand-off to **P4b T0**).

---

## 1. Locked decisions (from brainstorm clarifying loop, 2026-05-30)

| # | Decision | Rationale |
|---|---|---|
| 1 | **Row identity = synthetic `__dat0_rowid` surrogate**, injected at import, modeled as an extensible `RowKey` enum | Source-type-agnostic (works for future SQLite-scan / MotherDuck, unlike DuckDB `rowid`); fully controlled (no vacuum/portability quirks); integer key → fast `IN`/join at scale; `RowKey` enum keeps semantic-PK replay open for P7 with **no wire break** (tagged serde). DuckDB-`rowid` (A) rejected: dead-ends on non-DuckDB sources. User-PK (C) rejected for P4b: heavy (PK detect/select UI), and edits are ephemeral until "Save as Table" (spec §9.4) so semantic replay only bites at P7. |
| 2 | **`Edit` carries `Vec<CellEdit>`; `RowDelete` carries `Vec<RowKey>`** — one user action = one transform = one undo step | Exit gate requires batch ops (paste, fill-down, set-value) to undo as a single step. Multi-cell transform is the natural shape; matches the existing single-push `ViewModel::apply`. |
| 3 | **Edit/RowDelete render as an inline `* REPLACE` + `WHERE NOT IN` overlay** inside `compile_view_sql`; edits side-table **deferred** | P4b has no perf gate of its own; correctness-first. Fits P4a's one-view-per-chain model with zero new mutable DB state (undo stays free via cursor replay). Render strategy is internal to `compile_view_sql` — a side-table can replace it later with **no serialized-format change**. Only weak case = a single enumerated bulk edit over a huge discontiguous selection; documented ceiling. |
| 4 | **Overlay semantics: edits/deletes apply at base level; filters + sorts see *edited* values** (one nesting level, CTE chain rejected) | Spreadsheet-intuitive (change a cell → filter/sort reflect it). One subquery nesting; strict stack-order interleave (full CTE chain) is more faithful but rarely observable and churns the filter/sort-only output. When the stack has no Edit/RowDelete, `compile_view_sql` emits P4a's **exact** flat SQL → no golden/perf regression. |
| 5 | **Selection model = ephemeral keyboard-operable `SelectionModel` in grid; not persisted.** a11y baseline = keyboard operability only; SR semantics probed at T0, deferred | Exit gate says "keyboard nav covers all selection variants" — that is operability, achievable as pure input handling with no AccessKit-tree dependency. Screen-reader exposure (AccessKit) is real but reads as P10 hardening; a T0 probe scopes it. Selection is transient viewport state, not lineage → never written to session.json. |
| 6 | **Clipboard: immediate-clear cut; clamp + coerce-or-skip paste; de-facto Excel/Sheets TSV dialect** | Cut isn't gated (gate is *format* round-trip); immediate-clear (copy + clear as one Edit) is simple and net-equivalent to move. Paste clamps at table edges (P4b has no Row/Col-insert), coerces valid cells to column type, skips + banners invalid ones. Dialect = tab cols / `\r\n` rows / CSV-style quoting — verified against live Excel + Sheets, the real arbiter, not literal RFC 4180. |

---

## 2. Architecture overview

```
dat0-engine (Rust, no UI deps)
├── transform.rs   ← + Edit { cells: Vec<CellEdit> }, RowDelete { rows: Vec<RowKey> },
│                     + CellEdit, + RowKey enum (Surrogate(i64); P7 adds Pk)
├── render.rs      ← compile_view_sql gains inner overlay (SELECT * REPLACE(..) + WHERE NOT IN)
│                     wrapped by existing filter/sort; flat path unchanged when no edits/deletes
├── import path    ← inject deterministic __dat0_rowid surrogate column at table create;
│   (duckdb_engine)  lazy migration adds it to pre-P4b tables on open
└── (existing)     create_or_replace_view / drop_view / execute_paged — unchanged

dat0-app (GPUI)
├── view/model.rs       ← + edit_cells(), delete_rows(), is_dirty()  (apply/undo/redo reused)
├── grid/
│   ├── mod.rs          ← T0: funnel/sort click wiring (PD-016); hide __dat0_rowid; render ants + editor + dirty
│   ├── data_source.rs  ← surface __dat0_rowid per row (hidden key) for screen-row → RowKey
│   ├── selection.rs    ← NEW: SelectionModel (ranges/anchor/active + keyboard ops), pure-logic
│   ├── clipboard.rs    ← NEW: TSV codec + cut/copy/paste → Edit transforms
│   ├── cell_editor.rs  ← NEW: type-aware inline editor (gpui-component Input/Select)
│   └── context_menu.rs ← NEW: selection-aware menu → ActionRegistry dispatch
├── session/migrate.rs  ← schema_version 2 → 3 (additive transform variants; identity migration)
└── actions/            ← + edit/delete/clipboard/fill-down/bulk actions registered
```

**Data flow — cell edit:** grid commit → `vm.edit_cells([CellEdit{row,col,val}])` → `ViewChange` → (tokio) `engine.create_or_replace_view(name, sql)` → (main, via `MainThreadDispatcher`) `GridDataSource.rebind` + LRU invalidate + drop previous view. Identical to P4a's apply path; only the SQL differs.

**Reuse:** every P4b moving part rides shipped primitives — `ViewChange`/`spawn_view_change`/`apply_view_change` (P4a T13), `GridDataSource` + LRU (P3a), `MainThreadDispatcher` (P3b T1), `Banner` (P3b T2), `ActionRegistry` (P3b T3), gpui-component `Input`/`Select` real-window mount (P4a T10b), `SessionStore` atomic-write + fsync (P4a T8). No new infra crates.

---

## 3. Engine — `transform.rs` additions

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transformation {
    Filter { column: String, op: FilterOp, value: FilterValue },  // P4a
    Sort   { keys: Vec<SortKey> },                                // P4a
    Edit      { cells: Vec<CellEdit> },                           // P4b
    RowDelete { rows:  Vec<RowKey> },                             // P4b
    // P4c will add: Reorder, Rename, Delete (column)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellEdit {
    pub row:    RowKey,
    pub column: String,
    pub value:  Scalar,   // reuse P4a typed Scalar — literal renders via existing render_scalar
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowKey {
    Surrogate(i64),       // P4b ships this — maps to __dat0_rowid
    // P7 adds: Pk { col: String, val: Scalar }  — semantic replay; no wire break (tagged)
}
```

Wire shape stays self-describing JSONL (consumed verbatim by session.json v3, P7 `transforms.jsonl`, P8 `.dat0`). `RowKey` uses internally-tagged serde so the P7 `Pk` variant is purely additive. `CellEdit.value` reuses `Scalar` (adjacent-tagged) so no new literal-rendering code.

Serde round-trip tests mirror P4a `transformation_serde.rs`: Edit (empty + multi-cell), RowDelete, RowKey, nested Scalar variants.

---

## 4. Engine — `render.rs` overlay model

`compile_view_sql(base, ops)` keeps its signature. New behaviour:

1. Partition `ops` into edits/deletes vs filters/sorts (single pass).
2. **If no Edit/RowDelete present → emit the existing flat SQL unchanged** (`SELECT * FROM base WHERE … ORDER BY …`). Preserves P4a golden tests + 229 ms perf.
3. **If present → emit one nesting level:**

```sql
SELECT * FROM (
  SELECT * REPLACE ( <CASE per edited column> )
  FROM <base>
  WHERE __dat0_rowid NOT IN (<deleted keys>)        -- omitted if no RowDelete
) AS _edited
WHERE  <filters AND-joined>                          -- omitted if no Filter
ORDER BY <sort>                                       -- omitted if no Sort
```

**Fold semantics.** Walk active Edit transforms in stack order into `BTreeMap<column, Vec<(i64 key, Scalar)>>`, **last write wins per (key, column)**. Per edited column emit:

```sql
CASE WHEN __dat0_rowid = k1 THEN <lit1>
     WHEN __dat0_rowid = k2 THEN <lit2>
     ELSE <quoted_col> END AS <quoted_col>
```

`* REPLACE` means only edited columns are named — no full-schema enumeration, no schema parameter. Identifiers via `catalog::quote_ident`; literals via existing `render_scalar` (single-quote escaped, float `{:?}`). Deleted keys render as an integer `IN`-list (always `Surrogate(i64)` in P4b).

**New `RenderError` cases:** `EmptyEdit` (Edit with zero cells), `EmptyRowDelete`. Existing filter/sort error paths unchanged. New golden + error tests extend `render.rs` / `render_errors.rs`.

**Scale note (documented ceiling):** a single Edit/RowDelete enumerates its keys inline; a bulk op over a very large discontiguous selection produces a large `CASE`/`IN`-list. Acceptable for P4b (interactive, no perf gate). Predicate-based bulk + edits side-table are the escalation path and require **no** serialized-format change (decision 3).

---

## 5. Engine — `__dat0_rowid` surrogate + migration

- **At import / table create:** add a deterministic monotonic `BIGINT` column over scan order. Use an explicit sequence (or `row_number() OVER (ORDER BY <scan order>)`) — **not** bare `row_number() OVER ()` (order-undefined, breaks P7 replay reproducibility). T0 spike pins the exact DuckDB construct and confirms stability under the overlay (`* REPLACE` / `NOT IN`).
- **Collision guard:** the injected key name is **invariant** (`__dat0_rowid`) so the render path (§4) can reference it without a schema parameter. If a source already has a column of that name, the rare colliding **source** column is renamed at import (`__dat0_rowid__src`), not ours. The rename is recorded in the table's catalog entry for display.
- **Migration (pre-P4b tables):** tables imported before P4b lack the column. On first open, if absent, `ALTER TABLE … ADD COLUMN __dat0_rowid BIGINT` + a sequenced `UPDATE`. Lazy (only when a view/edit needs it). No session-format dependency.
- **Visibility:** the column rides through `SELECT *` views; the **grid hides it** (§8) and the filter-popover distinct-values query excludes it.

---

## 6. App — `view/model.rs` entry points

No structural change to `ViewModel` (its `apply(t) -> ViewChange` already drives any variant; undo/redo/clear/`HISTORY_CAP = 200` inherited). Add:

```rust
pub fn edit_cells(&mut self, cells: Vec<CellEdit>) -> ViewChange { self.apply(Transformation::Edit { cells }) }
pub fn delete_rows(&mut self, rows: Vec<RowKey>)   -> ViewChange { self.apply(Transformation::RowDelete { rows }) }
/// True when any Edit | RowDelete is in the active slice (stack[..cursor]). Feeds the dirty-tab indicator.
pub fn is_dirty(&self) -> bool {
    self.active().iter().any(|t| matches!(t, Transformation::Edit { .. } | Transformation::RowDelete { .. }))
}
```

Batch ops (paste, fill-down, set-value, multi-row delete) build one `Edit`/`RowDelete` and call once → one undo step (exit gate).

---

## 7. App — `grid/selection.rs` (new, pure-logic)

```rust
pub struct SelectionModel { ranges: Vec<CellRange>, anchor: CellCoord, active: CellCoord }
pub struct CellRange { r0: usize, c0: usize, r1: usize, c1: usize }  // inclusive rect (screen coords)
pub struct CellCoord { row: usize, col: usize }
```

- Rectangular ranges; full-row = all columns, full-column = all rows.
- Discontiguous via Cmd/Ctrl+click (push range); range-extend via Shift from `anchor`.
- Keyboard ops (§13) mutate `active`/`anchor`/`ranges` purely.
- Coordinates are **screen-space** over the filtered/sorted view; resolved to `RowKey` via the hidden key column at copy/edit time (§8).
- Ephemeral — never serialized. Unit-tested in isolation (no GPUI).

---

## 8. App — grid key-column plumbing + render

- `GridDataSource` exposes `__dat0_rowid` per page row (read alongside visible columns) → screen-row → `RowKey::Surrogate`.
- Grid **hides** the key column from render and from any column-enumeration UI.
- **Marching-ants:** overlay stroke on the copy-source range (decision 6); cleared on next mutating action / Esc.
- **Dirty indicator:** tab shows a dot when `vm.is_dirty()` (spec §8.2).

---

## 9. App — `grid/cell_editor.rs` (new, type-aware inline editor)

- Mounts a gpui-component `Input` (text/numeric) or `Select`/toggle (bool) on the active cell, on a real window (reuse P4a T10b mount infra; headless mount infeasible — P4a T0 spike §3).
- Type map: `STR`→text · `INT`/`NUM`→numeric-validated text · `DATE`/`TS`→validated text (`Scalar::validate_date`/`validate_timestamp`; calendar picker deferred) · `BOOL`→toggle/dropdown.
- Commit (Enter / focus-out) → `vm.edit_cells([CellEdit])`; Esc cancels. Invalid input blocks commit with inline affordance (reuse popover regex-validity pattern, P4a T10).

---

## 10. Clipboard — `grid/clipboard.rs` (new)

**TSV codec (pure-logic, unit-tested):**
- Serialize: tab between columns, `\r\n` between rows. A cell containing tab / CR / LF / `"` is wrapped in `"…"` with internal `"`→`""`. Discontiguous selection → bounding grid; gap cells emit empty.
- Parse: accepts `\r\n` and `\n` row breaks; handles quoted multiline cells; un-doubles `""`.

**Operations:**
- **copy** → TSV to clipboard + marching-ants on source.
- **cut** → copy + clear source cells, as **one** `Edit` (set selected cells to their type's empty/NULL).
- **paste** → parse clipboard TSV → anchor at `active` → **clamp** to table edges (no grow) → per cell coerce to target column `Scalar` type; **invalid cells skipped + counted**, surfaced via `Banner` ("N cells couldn't be pasted"); valid cells → **one** `Edit`.

**T0 probe:** verify GPUI clipboard read/write text API (`ClipboardItem` surface).

**Gate:** automated dialect asserts + **manual Excel + Google Sheets round-trip** in the retro UAT runbook (the exit criterion's true arbiter).

---

## 11. Context menu — `grid/context_menu.rs` (new)

gpui-component menu widget; items dispatch through P3b `ActionRegistry`. Selection-aware:
- cell → Copy · Paste · Clear · Fill down
- row(s) → + Delete row(s) · Set NULL · Set value…
- column(s) → + Set NULL / Set value over column

Bulk ops = the spec four (delete rows, set NULL, set value, fill-down); each maps the resolved selection → one `Edit`/`RowDelete`. Fill-down (Ctrl+D) copies the top selected cell down its column within the selection.

---

## 12. T0 — PD-016 wiring + probes (gate-blocking, first task)

Per P4a retro §"PD-016 decision" hand-off contract:
1. `grid/mod.rs` funnel-zone click → mount + present `filter_popover_entity` via `WorkspaceShell`.
2. popover `Outcome::Apply` → `vm.apply` + `spawn_view_change` (focused-workspace lookup); `Outcome::Clear` → `vm.clear`.
3. sort-zone click (plain + shift) → read `vm.current_sort_as_active()` → mutate → `vm.set_sort` + `spawn_view_change`.
4. click-path integration test: UI-click → ViewChange → engine round-trip → `apply_view_change` rebind loop.
5. **Probes (cheap, scope-setting):** GPUI clipboard API (§10); GPUI AccessKit surface (decision 5) → file SR-exposure feasibility for P10.

Per P4a retro lesson 4: add the `run-heavy` label at T0 completion (not retro time); keep the perf bench runner-independent.

---

## 13. a11y keyboard map (baseline = operability)

| Key | Action |
|---|---|
| Arrows | move `active` cell |
| Shift+Arrows | extend range from `anchor` |
| Cmd/Ctrl+Arrows | jump `active` to data edge |
| Cmd+A | select all |
| Shift+Space | select row |
| Ctrl+Space | select column |
| Esc | clear selection / cancel edit / cancel ants |
| Enter / F2 | begin edit on `active`; Enter commits |
| Cmd/Ctrl+D | fill down |
| Cmd/Ctrl+C/X/V | copy / cut / paste |
| Delete/Backspace | clear selected cells (one Edit) |

Visible focus ring on `active`. Every selection variant keyboard-reachable (exit gate). Screen-reader semantics deferred per T0 probe.

---

## 14. Persistence — session.json v2 → v3

- Edit/RowDelete/RowKey serialize into the existing per-tab stack automatically (tagged serde, additive). Selection is **not** persisted.
- **Bump `schema_version` 2 → 3.** Reason: a v3 session may contain `Edit`/`RowDelete` `kind`s an older P4a binary can't deserialize; bumping makes `migrate.rs` show its **forward-incompat banner** instead of a hard serde error. v2→v3 forward-migration is the identity (no field changes); `migrate.rs` gains the v3 arm + an unknown-`kind` guard.
- Crash + restore extends P4a `view_restore_e2e`: the active stack (now including edits/deletes) + cursor + view rebind restore as before — edits replay through `compile_view_sql` on reload.

---

## 15. Threading + reuse discipline

Unchanged from P4a: stack mutation + SQL render on main thread; view create/replace/drop on tokio worker; result posted via `MainThreadDispatcher`; supersede-cancel via `engine.interrupt` (D-008 structured token stays P5). Clipboard codec + SelectionModel + TSV parse are pure-logic (main thread, cheap). Cell-editor widget mount requires a real window (P4a T10b).

---

## 16. Testing strategy

- **Pure-logic (no GPUI/DB):** `SelectionModel` ops (move/extend/jump/multi/row/col/clear); TSV codec round-trip incl. quoting + multiline + discontiguous; render overlay golden (`* REPLACE` + `NOT IN`, no-edit flat path unchanged); edit-fold last-write-wins; RowKey/Edit/RowDelete/CellEdit serde.
- **Integration (real DuckDB):** edit → view round-trip; row delete; **edit-then-filter sees edited value** (decision 4); paste coerce/skip; `__dat0_rowid` determinism + migration on a pre-P4b table.
- **App integration:** PD-016 click → ViewChange loop (T0); cell-editor commit → `edit_cells`; context-menu bulk op → one transform → one undo.
- **E2E:** extend `view_restore_e2e` → apply edit + delete + filter + undo + redo + crash + reload.
- **Gate (manual UAT, retro runbook):** TSV copy → paste into Excel and into Google Sheets, and reverse, with tab/newline/quote/unicode cells; full keyboard-only selection sweep.

---

## 17. Task sketch (full breakdown at `/gsd:plan-phase`)

| T | Title |
|---|---|
| T0 | PD-016 click wirings + integration test; clipboard + AccessKit probes; `run-heavy` label |
| T1 | `transform.rs`: `RowKey` + `Edit` + `RowDelete` + `CellEdit` + serde tests |
| T2 | `render.rs`: overlay model (`* REPLACE` + `NOT IN`), fold last-write-wins, error cases, golden tests |
| T3 | Import `__dat0_rowid` surrogate (deterministic) + collision guard + lazy migration |
| T4 | `SelectionModel` (pure-logic) + keyboard op set |
| T5 | Grid hidden-key plumbing (screen-row → `RowKey`) + hide from render/UI |
| T6 | `cell_editor.rs` type-aware inline editor (gpui-component mount) → `edit_cells` |
| T7 | `clipboard.rs` TSV codec + cut/copy/paste + coerce-or-skip + reject banner + marching-ants |
| T8 | Fill-down (Ctrl+D) + bulk ops (delete rows / set NULL / set value) → one transform each |
| T9 | `context_menu.rs` selection-aware menu via `ActionRegistry` |
| T10 | Dirty-tab indicator (`vm.is_dirty`) |
| T11 | a11y keyboard map wiring + focus ring + keyboard-only selection sweep test |
| T12 | PD-002 settings.toml fsync close (`settings/store.rs` `sync_all` + parent-dir fsync) |
| T13 | session v2→v3 migration + unknown-`kind` guard; E2E restore extension |
| T14 | Retro + deferral closures + manual Excel/Sheets UAT |

~15 tasks — matches P4a cadence. Each its own commit; two-stage review per dev workflow.

---

## 18. Exit criteria + deferrals

**Exit criteria (design → verifiable):**
1. `Edit` + `RowDelete` apply correctly and reverse via undo (integration + E2E).
2. Batch ops (paste, fill-down, set-value, multi-delete) undo as one step.
3. TSV round-trips Excel **and** Google Sheets (manual UAT) + automated dialect asserts.
4. Selection passes a11y baseline — keyboard nav covers all variants (keyboard-only sweep test).
5. PD-016 closed — funnel/sort click → ViewChange loop live + tested.
6. No-edit chains emit P4a-identical SQL (render golden parity; 500 ms filter gate unregressed).

**Deferrals:**
- **Closes PD-016** (P4a retro hand-off) and **PD-002 settings.toml side** (open since P1; session side closed P4a T8).
- **PD-011** stays open (P4b does not touch the import wizard).
- No new D-NNN expected — clipboard + AccessKit risks pre-probed at T0. If the AccessKit probe finds SR exposure infeasible on the pinned GPUI, file a D-NNN targeted at P10 hardening.

---

## 19. Risks / T0 spike checklist

| Risk | Mitigation (T0) |
|---|---|
| `__dat0_rowid` non-deterministic or unstable under overlay | Pin the DuckDB sequence/`row_number` construct; assert stable keys across filter/sort/delete; golden the overlay SQL. |
| GPUI clipboard API shape unknown | Probe `ClipboardItem` read/write before T7. |
| AccessKit absent on pinned GPUI | Probe; if absent, scope SR to P10, keep operability gate (decision 5). |
| `* REPLACE` interaction with later filter/sort on edited column | Integration test edit-then-filter (decision 4) before T7/T8. |
| Large enumerated bulk edit SQL size | Documented ceiling (decision 3); side-table is the no-format-change escalation. |

---

*End of P4b design.*
