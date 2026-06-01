# dat0 P4c — Design (Projection transforms + PipelineBar + Export · P4 close-out)

> Brainstorm output, 2026-05-31. P4c is the **closing third** of spec §21.2 P4
> (P4a = view-state/filter/sort hot path, merged `79a86a9`; P4b = edit/selection/
> clipboard, merged `72bced4`). P4c lands the remaining projection transforms,
> the PipelineBar, and export-to-file, then closes the P4 exit gate.
>
> Grounded against live code at `main` @ `72bced4`. Engine + render facts below
> were verified by reading `crates/dat0-engine/src/{transform.rs,render.rs,export.rs,
> trait_def.rs}` and the GPUI 0.2.2 platform surface — not recalled.

## 0. Locked decisions (this brainstorm)

| # | Decision | Over | Because |
|---|---|---|---|
| 1 | **Max scope:** 3 transforms + PipelineBar + Export + ALL P4b polish backlog + owed UAT as formal P4-exit gate | Tight (core only) / mid (core + cheap polish) | One phase finishes P4 cleanly; polish backlog is small and adjacent to the surfaces P4c already touches |
| 2 | **Source-identity projection, display-layer-last** — filters/sorts bind to the underlying column identity (stable across rename); reorder+rename+delete-col fold into ONE outermost projection SELECT applied last | Display-name / strict-stack CTE chain | Keeps P4b's ≤2-level nesting + byte-identical fast-path; no CTE chain; engine stays schema-blind (payload-carried, like Sort carries keys); rename can't orphan a later filter |
| 3 | **PipelineBar = scrubber + per-transform remove** | Read-only viz / linear-scrubber-only | User wants mid-stack removal; decision 2's source-identity + surrogate-key binding makes mid-stack removal safe by construction |
| 4 | **Export = streaming COPY-to-path** | Reuse in-memory `export_table -> Vec<u8>` | Matches the 2GB–TB files-at-scale thesis; existing export is already COPY-to-tempfile + read-back, so this just drops the read-back |
| 5 | **SQL transform stays P5** (enum-absent; ships with the SQL Console) | Land a SQL transform node now | No SQL UI until P5; the enum comment already reserves only Reorder/Rename/Delete for P4c |

## 1. Scope

**In:**
- Engine: `Reorder`, `Rename`, `DeleteColumn` transform variants + their render.
- Engine: `compile_view_sql` projection fold (source-identity model, `base_columns` param).
- Engine: streaming `export_query_to_path` (COPY direct to destination).
- App: column-header projection UI — drag-reorder (grip zone), inline rename (header body), delete (context menu). These finally wire the P4a four-zone header's grip + body zones, which P4a T9 shipped as "stubs for P4b/c".
- App: per-tab column model extended from source-names to `(source, display)` pairs.
- App: PipelineBar — collapsed pill strip + expanded vertical timeline; scrubber (jump-to-state) + per-transform remove.
- App: unified pipeline history (snapshot timeline) replacing P4a's `Vec + cursor`.
- App: Export dialog (format + scope + native save panel).
- session.json v3→v4 (additive variants; identity migration; reuse forward-incompat guard).
- **Polish backlog (all, per decision 1):** marching-ants render (range already stored P4b); header-click-to-select (wire the built-but-unreachable `Key::SelectRow`/`SelectColumn` + `SelectionModel::select_row`/`select_column`); `window.rs` (~1739 lines) → extract grid edit/clipboard handlers into `grid/edit_ops.rs`; inline-cell-editor key-focus rework.
- **P4 close-out:** the owed manual Excel/Sheets TSV round-trip + keyboard-only UAT sweep, run as the formal P4 exit gate (user-run; needs GUI perms — agents can't drive cross-app + Screen-Recording).

**Out (explicit):**
- SQL transform (P5, with the console).
- D-007 MotherDuck ATTACH end-to-end, D-008 cancellation-token (P5).
- D-015 AccessKit / screen-reader selection-tree (P10 — operability-only a11y stands).
- Non-linear *reordering* of existing transforms in the pipeline (only removal + scrubber-jump; drag-to-reorder pipeline steps is not in scope).
- Persisting in-session undo history across restart (see §5 trade).

## 2. Engine surface

### 2.1 Transform variants (additive)

`crates/dat0-engine/src/transform.rs` — the enum's `// P4c will add` comment is realized:

```rust
Reorder {
    /// Full visible source-column order after this step (excludes __dat0_rowid).
    columns: Vec<String>,
},
Rename {
    /// Source column identity (base-table column name; stable across renames).
    column: String,
    /// New display name.
    to: String,
},
DeleteColumn {
    /// Source column identities dropped from the visible projection.
    columns: Vec<String>,
},
```

Serde: same tagged wire format as P4b (`kind`-tagged externally per the established
convention). New `kind` strings added to `KNOWN_TRANSFORM_KINDS` (forward-incompat guard).

**Why these payload shapes** — source identity is the **base-table column name**, which
never changes (a rename only adds a display alias at the outermost projection; the
underlying column keeps its name). So every transform that references a column —
`Filter`, `Sort`, `Rename`, `DeleteColumn`, and the `columns` list in `Reorder` — carries
source names. The app resolves the clicked *display* column back to its source name when
building a transform. This is decision 2 made concrete.

### 2.2 Render — `compile_view_sql` projection fold

Signature gains the base column list (the renderer needs it to fold a partial set of
projection ops into a complete explicit projection):

```rust
pub fn compile_view_sql(
    base: &str,
    base_columns: &[String],   // NEW: base-table user columns (source names, no surrogate)
    ops: &[Transformation],
) -> Result<String, RenderError>
```

Fold algorithm (projection ops only; filter/sort/edit/delete unchanged from P4b):
1. Seed an ordered model `Vec<ProjCol { source, display, visible }>` from `base_columns`
   (`display = source`, `visible = true`).
2. Walk `ops` in stack order; for each projection op mutate the model:
   - `Rename { column, to }` → set `display = to` on the matching `source`.
   - `DeleteColumn { columns }` → set `visible = false` for those sources.
   - `Reorder { columns }` → set the model order to this full visible source list (it always
     carries the complete post-step visible order; deleted sources are absent from it).
3. If the model is unchanged from seed (no projection op touched it) AND there are no
   edits/deletes → emit P4a's **byte-identical flat** `SELECT * …` (fast path; `base_columns`
   ignored). This preserves the P4b structural perf guard.
4. Otherwise the projection becomes the **outermost SELECT's projection list** (replacing
   `SELECT *`) — *not* a new wrapping level. SQL evaluates `WHERE`/`ORDER BY` against the
   `FROM` (source) columns before the SELECT list is applied, so filter/sort/rename/reorder/
   delete all coexist in one SELECT. Nesting stays ≤2 (the optional P4b edit/delete overlay
   is the only inner level), matching decision 2.

```sql
-- flat case (no edit/delete overlay): projection replaces the `*` in P4a's flat SELECT
SELECT __dat0_rowid, "src1" AS "DispA", "src3" AS "DispC"
FROM <base> WHERE <filters on source> ORDER BY <sort on source>

-- overlay case: projection replaces the `*` in P4b's outer SELECT over the inner overlay
SELECT __dat0_rowid, "src1" AS "DispA", "src3" AS "DispC"
FROM ( SELECT * REPLACE (…) FROM <base> WHERE __dat0_rowid NOT IN (…) )
WHERE <filters on source> ORDER BY <sort on source>
```

`WHERE`/`ORDER BY` on a since-deleted column still compile (the source is present in the
`FROM`/subquery even when not projected) — the UI won't offer it, but a stale stack entry
won't break. Rules:
- `__dat0_rowid` is **always** carried explicitly as the first projected column → stays
  app-hidden via the existing `schema_index_for_visible` mapping. Never reorderable,
  never deletable (not in `base_columns`).
- `__dat0_rowid__src` (the P4b collision-rename of a real user column) **is** a normal
  user column → it lives in `base_columns` and the projection model like any other.
- Only `visible == true` columns are projected, in model order, `source AS "display"`
  (display always quoted via `quote_ident`; alias omitted when `display == source`).
- Filters/sorts reference **source** names, available from the `FROM` clause regardless of
  display renames → no rebinding, no CTE chain.

This sidesteps DuckDB `* RENAME` (uncertain on the 1.4.4 pin) entirely — the explicit
list expresses reorder+rename+delete in one SELECT with no star-magic.

### 2.3 Export — streaming COPY-to-path

`crates/dat0-engine/src/export.rs` today is COPY-to-tempfile + read-back into `Vec<u8>`.
P4c extracts the COPY to write **directly to the user's destination**:

```rust
pub(crate) fn export_query_to_path(
    conn: &duckdb::Connection,
    select_sql: &str,       // a full SELECT (already surrogate-stripped by the caller)
    format: ExportFormat,
    dest: &Path,
) -> Result<()>   // COPY (<select_sql>) TO '<dest>' (FORMAT …); no read-back
```

`export_table_bytes` is refactored to delegate (tempfile dest + read-back) so the in-memory
path and tests keep working. New trait method on `QueryEngine` (async):
`export_query_to_path(select_sql, format, dest) -> Result<()>`.

Format clauses reuse the proven strings: `FORMAT CSV, HEADER` / `FORMAT JSON, ARRAY` /
`FORMAT PARQUET`. Path literal escaped via `.replace('\'', "''")`; table/idents via
`quote_ident` (both already in `export.rs`).

**Surrogate handling (correctness):** the export SELECT must drop `__dat0_rowid` but keep
`__dat0_rowid__src`:
- *Current view* → `COPY (SELECT <visible display projection, no __dat0_rowid> FROM (<compiled view>)) TO …`
- *Full table* → `COPY (SELECT * EXCLUDE (__dat0_rowid) FROM <base>) TO …`

The module docstring's "streaming export … deferred (spec §4 out-of-scope)" is updated —
P4c brings it in scope.

## 3. App surface

### 3.1 Column-header projection UI (P4a four-zone header, grip + body finally wired)

- **Reorder** — drag the header **grip** zone; on drop, app computes the new visible order
  and emits `Reorder { columns }` (source names, new order).
- **Rename** — double-click the header **body** (or a header context-menu item) → inline
  text edit → `Rename { column, to }`.
- **DeleteColumn** — header context menu → `DeleteColumn { columns }` (current column, or
  the selected column set if a column selection is active).

### 3.2 Column model: source → (source, display)

The per-tab column model (today `visible_column_names` / `column_name` / `column_type`,
returning source names) gains a display layer folded from the projection ops:
`Vec<ProjCol { source, display, visible }>`. The grid renders `display` + model order;
all transform-builders (filter funnel, sort, rename, delete, export) resolve the clicked
display column → `source`. This is the "track a stable column id through renames" cost of
decision 2; it is app-side only (the engine receives finished source-name payloads).

### 3.3 Export dialog

Compact modal (gpui-component modal/overlay; P3b mounted a Sheet for recovery):
- Format radio: CSV / JSON / Parquet.
- Scope radio: **Current view** (transforms applied) / **Full table** (base).
- Export button → `cx.prompt_for_new_path(dir, suggested_name)` (GPUI native save panel,
  mac + linux) → engine `export_query_to_path` on a tokio task → Banner on success/error.
- Suggested filename = `<base>.<ext>`; scope + format drive the SELECT the app hands the engine.

## 4. PipelineBar

Per-tab visualization of the transform stack, two modes.

```
COLLAPSED (pill strip):
 ┌─────────────────────────────────────────────────────────────┐
 │ ▣ base  ›  ⛛ Filter city  ›  ⇅ Sort price↓  ›  ✎ Rename A→B ✕ │  ⌄ expand
 └─────────────────────────────────────────────────────────────┘
   solid pill = applied            ✕ on hover = remove this transform

EXPANDED (vertical timeline):
 ┌──────────────────────────────┐
 │ ● base table                 │
 │ │  ⛛ Filter  city = 'NYC'   ✕│
 │ │  ⇅ Sort    price ↓        ✕│
 │ ●  ✎ Rename A → B           ✕│ ← active tip
 │ ┊  ⊘ Delete col notes       ✕│   greyed = redoable (post-jump)
 └──────────────────────────────┘
```

Each pill/row carries a type icon + a human description rendered from the
`Transformation` (e.g. `Filter city = 'NYC'`, `Sort price ↓`, `Rename A → B`,
`Delete col notes`, `Edit 3 cells`, `Delete 12 rows`). Hover (collapsed) reveals the full
description.

### 4.1 Unified history model (revises P4a `Vec + cursor`)

Working stack `S: Vec<Transformation>` (the pills) + a bounded **undo timeline** of `S`
snapshots (cap 200, the P4a `HISTORY_CAP`) + a redo stack. Every structural edit is the
same op class — *edit `S` → push snapshot*:

| Action | Effect on `S` |
|---|---|
| Apply transform | append (truncates redo) |
| Scrubber: jump to pill *k* | truncate to `S[0..=k]` |
| Per-transform remove (✕ on pill *k*) | remove element *k* |
| Cmd+Z / Cmd+Shift+Z | restore previous / next snapshot |

After any edit the view recompiles from the new `S` (`compile_view_sql` → `create_or_replace_view`).

**Why safe (decision 2/3 synergy):** because filters/sorts bind to **source identity** and
rows to the **stable surrogate key**, removing a mid-stack transform can't orphan a later
one — removing a rename leaves later filters intact (they reference source names),
removing a delete-col makes the column visible again, removing a RowDelete restores rows
and later edits still apply by key. No dangling references by construction.

**Trade (sign-off captured):** this replaces P4a's greyed-redo-tail-via-cursor with a
snapshot timeline. Redo after a jump is via Cmd+Shift+Z (the redo stack), not a persistent
greyed tail that survives further edits. Session persists the active `S` only; in-session
undo history is memory-only → **cross-restart undo narrows** vs P4a (active state restores;
the undo *history* does not). Parity alternative (persist the timeline) is heavier and
rejected for v1.

## 5. Data + session

- **session v3→v4.** Additive `Transformation` variants; identity migration v3→v4; reuse
  the P4b `KNOWN_TRANSFORM_KINDS` allowlist forward-incompat guard (a v3 reader meeting a
  Reorder/Rename/DeleteColumn `kind` → `ForwardIncompatTransform` banner, already built).
- Persist the active working stack `S` per tab (sufficient to reconstruct the view). Drop
  cross-restart undo history (the §4.1 trade).

## 6. Tests

- **Render goldens:** rename / reorder / delete-col individually + combined; surrogate
  carried first; `__dat0_rowid__src` preserved; projection + edit-overlay + filter + sort
  stacked; **no-projection / no-edit byte-identical P4a parity** (extends the P4b
  `no_edits_emits_flat_p4a_sql` guard to also assert no-projection flatness).
- **Projection-fold unit tests:** partial rename folds into full explicit list; reorder
  applies the full visible order (deleted sources absent); delete-then-rename-then-reorder
  ordering; projection lives in the outer SELECT list (no extra nesting level).
- **Export goldens:** 3 formats × 2 scopes; surrogate-strip (`__dat0_rowid` absent,
  `__dat0_rowid__src` present); path-quote escaping; COPY-to-path writes the dest file
  (integration vs real DuckDB).
- **History-model unit tests:** apply / jump / remove-middle / undo / redo / cap-eviction;
  remove-safety (removing a rename keeps a later filter valid).
- **Serde round-trip:** the 3 new variants.
- **E2E:** reorder → rename → delete-col → filter-on-renamed-source → export current-view →
  per-transform-remove → undo → crash/restore.
- **PipelineBar:** pill/row description rendering per variant; collapsed↔expanded.

## 7. Performance

No new perf gate. The no-projection / no-edit path stays **byte-identical** to P4a, so the
`view_regen` heavy bench holds **structurally** (same guarantee P4b made). The explicit
projection adds cost only to views that actually carry projection ops or edits. Re-run the
`run-heavy` `view_regen` bench at merge to confirm no regression on the flat path.

## 8. Exit criteria (completes spec §21.2 P4)

| Criterion | How P4c meets it |
|---|---|
| All 8 transform types apply + reverse via undo | Reorder/Rename/DeleteColumn added (the last 3 non-SQL); SQL→P5; all reverse via the snapshot history |
| Undo/redo restores batch ops | History model §4.1; per-transform-remove + jump are single undo steps |
| TSV round-trips Excel + Google Sheets | **User-run UAT** (P4b logic shipped; gate runs here) |
| 1M-row filter < 500 ms | Already met (P4a); flat path unchanged |
| PipelineBar collapsed + expanded both functional | §4 |
| Selection a11y baseline (keyboard nav all variants) | **User-run UAT** (P4b logic + header-click-to-select wired here; operability-only, D-015 SR→P10) |
| Export valid CSV/JSON/Parquet, current-view + full-table | §2.3 + §3.3 |

## 9. Task sketch (subagent-driven; two-stage review on engine/integration, combined-verify on mechanical)

| T | Task | Review |
|---|---|---|
| T0 | `compile_view_sql` projection fold + `base_columns` param + goldens (incl. flat parity) | full |
| T1 | 3 transform variants + serde + `KNOWN_TRANSFORM_KINDS` | full |
| T2 | Pipeline history model: snapshot timeline (replaces `Vec + cursor`), undo/redo/jump/remove | full |
| T3 | App column model `(source, display)` fold + display→source resolution | full |
| T4 | Header grip drag-reorder → `Reorder` | combined |
| T5 | Header rename (inline) + delete-col (menu) → `Rename`/`DeleteColumn` | combined |
| T6 | PipelineBar collapsed pill strip | combined |
| T7 | PipelineBar expanded timeline + scrubber-jump + per-transform-remove | full |
| T8 | Engine `export_query_to_path` (COPY-to-path) + `export_table_bytes` delegate + tests | full |
| T9 | Export dialog UI + native save panel + surrogate-strip SELECT wiring | combined |
| T10 | Marching-ants render (selection / cut range) | combined |
| T11 | Header-click-to-select: wire `SelectRow`/`SelectColumn` triggers | combined |
| T12 | `window.rs` → `grid/edit_ops.rs` extraction + inline-editor key-focus rework | combined |
| T13 | session v4 + E2E + retro + heavy-bench re-run | full |

## 10. Risks / open items

- **`base_columns` signature churn** — touches every `compile_view_sql` caller + all render
  goldens. Wide but mechanical; broadcast the new signature to all task briefs at T0 (the
  P4b lesson on propagating a signature change once).
- **History-model refactor (T2)** touches P4a's tested undo path — needs careful regression
  coverage so existing apply/undo/redo + crash-restore E2E stay green through the swap.
- **Editor key-focus rework depth (T12)** is fuzzy — scoped to: a real GPUI `FocusHandle`
  for the inline cell editor; Enter/Tab commit-and-advance; Esc cancel (already shipped P4b).
  Not a full keyboard-grid rewrite.
- **Export dialog modal primitive** — confirm gpui-component modal/overlay mounts headlessly
  (P3b's Sheet was stubbed); fall back to a plain GPUI overlay if needed.
- **PD-011** (sniff_csv spec-wording drift) stays open; unrelated to P4c.

## 11. Deferrals touched

- Opens nothing new expected. Any projection/export edge that slips becomes a PD at the
  task where it's found (P4a/P4b cadence).
- D-014 (Memory Budget Settings) untouched — still P3c/P9c.
- D-015 (a11y SR) untouched — P10.
