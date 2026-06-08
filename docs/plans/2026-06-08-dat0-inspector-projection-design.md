# Projection-Aware Inspector — Design

**Date:** 2026-06-08
**Status:** design (approved in brainstorm; pending spec review → plan)
**Follow-on to:** P6b (Inspector lineage chain, PR #14 `b94698a`; PD-022 follow-up PR #15 `1de7163`)

## Goal

Make the Inspector reflect the grid's **display-only column projection** (Reorder / Rename / DeleteColumn) so a user can line up Inspector cards with what they see in the data grid. Today the Inspector profiles the *underlying* view/table and renders one card per underlying column — in physical order, with physical names, including columns the user has hidden and the internal `__dat0_rowid` surrogate. That does not match the grid, which shows the projected columns (reordered, renamed, hidden-omitted). This feature closes that gap.

This is the "projection-aware Inspector" deferred at the P6b PD-022 follow-up. It is **not** a profiling change — it is a display-layer re-arrangement of already-computed cards.

## Background (verified against live code, 2026-06-08)

- **The projection is display-only.** Reorder/Rename/DeleteColumn are explicit no-ops in `compile_view_sql` (`crates/dat0-engine/src/render.rs:73-80`, design Option B). So **both** Inspector profiling modes see the *same column set*:
  - Whole-table mode → `SUMMARIZE <base table>` → all base columns + the `__dat0_rowid` surrogate.
  - Current-view mode → `SUMMARIZE (<compile_view_sql>)`; since projection is a no-op there, the compiled view still selects every base column + surrogate.
  The two modes differ only in **row scope** (filters/edits), never in the **column set**. So the projection logic is mode-independent.
- **The projection is already computed and stored live.** `crate::view::fold_columns(base, ops)` (`crates/dat0-app/src/view/column_view.rs`) returns `Vec<ProjectionColumn { source, display }>` — visible columns in display order, renames applied to `display`, deletes omitted. `WorkspaceShell.column_view` (`window.rs:643`) holds the active tab's folded projection, refreshed by `refresh_column_view()` (`window.rs:996`) from `data_source.visible_column_names()` (the base, surrogate already excluded) + `view_model.active()` (the transform stack).
- **The surrogate name** is `dat0_engine::transform::ROWID_COL` (`= "__dat0_rowid"`). It is excluded from `visible_column_names()` but appears in the profile (it is a physical column).
- **The Inspector panel** is a free function `render_inspector(&InspectorModel, cx)` (`inspector/panel.rs:25`), called once from `WorkspaceShell::render` (`window.rs:3439`). It renders one `column_card` per `profile.columns` entry.
- **The Inspector target** (`inspector.target_table: Option<String>`, a bare name) is set **only** via `set_inspector_target`, whose sole caller is `open_table_tab` — which also makes that table the active grid tab. So `inspector.target` normally equals the active tab's base table. The active tab's base table identity is available via `view_model.base_table()` (quoted `"schema"."table"`).

## Locked decisions (from brainstorm)

- **D1 — Both modes project.** The card *set/order/labels* always mirror the grid, in Whole-table and Current-view alike. The mode toggle changes only which rows are profiled, not which columns are shown.
- **D2 — Hidden columns → a collapsed "Hidden" section.** Grid-visible columns render as normal cards in grid order. Columns the user hid (DeleteColumn) collect under a collapsed-by-default, dimmed **"Hidden (N)"** header (click to expand) so their stats stay reachable without un-hiding. If none are hidden, no header.
- **D3 — Renamed cards show new name + subtle original.** Card header shows the `display` label prominently with a subtle `· was <source>` when `display != source`. Stats are always matched to the underlying `source` column.
- **D4 — Surrogate `__dat0_rowid` is always omitted** (both visible and hidden lists, projection or not). This is internal, never a user column. Resolves the standing P6a cosmetic note as a free side effect.
- **D5 — Display-layer only (Approach A).** No profiling change, no projected re-SUMMARIZE, no snapshot of the projection into `InspectorModel`. The live `column_view` stays the single source of truth.
- **D6 — No session-schema bump.** The only new state is one ephemeral `hidden_expanded: bool` on `InspectorModel` (reset on target change). Not persisted.

## Architecture

Engine: **untouched.** App: a pure mapping unit + a shell guard + a panel render change.

```
WorkspaceShell::render (window.rs:3439)
  └─ self.inspector_projection() -> Option<ProjectionContext>
       │   Some  ⇔ inspector.target_table matches the active tab's base table
       │            (identity compared on the bare, unquoted table name)
       └─ render_inspector(&self.inspector, projection, cx)
            └─ inspector::projection::project_cards(profile.columns, projection.as_ref())
                 → ProjectedCards { visible: Vec<RenderCard>, hidden: Vec<RenderCard> }   // PURE
```

### Units

1. **`inspector/projection.rs` (new, pure, unit-tested)** — the heart.
   - Types:
     - `ProjectionContext { visible: Vec<ProjectionColumn>, base_sources: Vec<String> }` — `visible` = the grid `column_view`; `base_sources` = all non-surrogate base column names (to derive the hidden set).
     - `RenderCard { source: String, label: String, original: Option<String> }` — `source` keys into the profile; `label` is the header text; `original` = `Some(source)` only when renamed.
     - `ProjectedCards { visible: Vec<RenderCard>, hidden: Vec<RenderCard> }`.
   - `fn project_cards(profile_cols: &[ColumnProfile], ctx: Option<&ProjectionContext>) -> ProjectedCards` (pure). Rules in the next section.
2. **`WorkspaceShell::inspector_projection(&self) -> Option<ProjectionContext>` (window.rs)** — the guard. Returns `Some` only when `inspector.target_table` equals the active tab's base table (extract the bare, unquoted table name from `view_model.base_table()`'s `"schema"."table"` form and compare). This bare-name identity is intentionally consistent with the app's existing bare-name catalog/lineage keying (and shares its known limitation — two same-named tables in different attached DBs would compare equal; pre-existing, out of scope here). Builds `visible` from `self.column_view.clone()` and `base_sources` from `data_source.visible_column_names()`. Returns `None` when there is no view model / data source, or when the Inspector targets a different table than the active grid (graceful, no mis-projection). The guard is independent of the Inspector's Whole-table/Current-view mode — the per-tab `ViewModel`/`data_source` exist in both, so projection applies in both (D1).
3. **`inspector/panel.rs` (modified render)** — calls `project_cards`, renders `visible` cards (existing `column_card`, now taking a label + optional original) in order, then a "Hidden (N)" collapsible section for `hidden` (dimmed cards). Takes the new `Option<ProjectionContext>` parameter.
4. **`inspector/model.rs`** — add `hidden_expanded: bool` (default false; cleared in `set_target` when the table changes) + a toggle method `toggle_hidden(&mut self)`.

## Data flow — `project_cards` rules

Let `S = ROWID_COL`. For input `profile_cols` (keyed by `.name`) and `ctx`:

- **Surrogate:** any `profile_col.name == S` is dropped in all branches.
- **`ctx == None`** (Inspector targets a non-active table, or no view bound): graceful fallback = today's behavior minus the surrogate.
  - `visible` = every non-surrogate `profile_col` in profile order, `label = name`, `original = None`.
  - `hidden` = empty.
- **`ctx == Some`:**
  - `visible`: for each `ProjectionColumn { source, display }` in `ctx.visible` order, find `profile_col.name == source`. If found → `RenderCard { source, label: display, original: (display != source).then(|| source.clone()) }`. (A projection source with no matching profile column is skipped — defensive; should not happen.)
  - `hidden`: `hidden_sources = ctx.base_sources − {c.source for c in ctx.visible}` (set difference, preserving `base_sources` order). For each, find its `profile_col` → `RenderCard { source, label: source, original: None }`.
  - Profile columns whose `name` is neither in `ctx.base_sources` nor the surrogate → dropped (defensive).

Card stats (numeric/length/distinct/null + inline charts) are looked up exactly as today, keyed by `source` (the real column name) — renames never affect stat lookup.

## Panel render

- **Visible section:** unchanged layout; each card's header now renders `label` (with a subtle, dimmed `· was {original}` suffix when `original` is `Some`). The card body, stat lines, and inline charts are unchanged and still keyed by `source`.
- **Hidden section:** rendered only when `hidden` is non-empty.
  - A clickable **"Hidden ({N})"** header row (uses `inspector.hidden` i18n + the count), collapsed by default. Clicking toggles `inspector.hidden_expanded` via `cx.listener(|ws,…| ws.inspector.toggle_hidden(); cx.notify())` (mirrors the existing mode-toggle button wiring at `panel.rs:62`).
  - When expanded, the hidden cards render dimmed (reduced opacity / muted) beneath the header.
- The overview line and the Whole-table⇄Current-view toggle are unchanged. The lineage chain section (P6b) is unchanged.

## Reactivity

This **refines** the merged PD-022 follow-up: a display-only undo/redo (and forward Rename/Reorder/DeleteColumn) must now **re-render** the Inspector so its cards re-project — but still must **not re-profile** (the underlying data and its profile are unchanged; the merged guard test `display_only_undo_keeps_inspector_profile_source_stable` remains valid).

- Because `render_inspector` reads the live `column_view` (via `inspector_projection()`) on every shell render, any `cx.notify()` after `refresh_column_view()` re-projects the cards for free.
- Forward/undo/redo display-only paths already call `refresh_column_view()` and re-render the grid header in the same shell pass. The plan will **verify** that every `column_view`-changing path triggers a shell re-render that reaches the Inspector, and add a single `cx.notify()` only where one is missing.
- Doc updates: change the `dispatch_undo`/`dispatch_redo` seam comment and the guard-test comment from "Inspector deliberately not refreshed" → "Inspector is **re-projected (re-rendered) but not re-profiled**."

## Testing

- **`project_cards` (pure) — unit tests:**
  - visible order follows `ctx.visible`; rename sets `label` + `original`; reorder permutes; delete moves a column to `hidden`.
  - `hidden` = `base_sources − visible.sources`, order preserved.
  - surrogate omitted in every branch (visible, hidden, and `None`).
  - `ctx == None` → all non-surrogate cards visible in profile order, no hidden, `original = None`.
  - a profile column absent from `base_sources` (and not surrogate) is dropped.
  - a `visible` source with no matching profile column is skipped without panic.
- **`inspector_projection()` guard + panel render** are shell/GPUI-bound (no headless `App` in the harness, per P6a/P6b) → verified by `cargo build` + the full suite staying green + manual UAT. The pure mapping carries the test weight.
- Local gate before each commit: `cargo test -p dat0-engine -p dat0-app`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## File structure

| File | Responsibility | Action |
|------|----------------|--------|
| `crates/dat0-app/src/inspector/projection.rs` | Pure `project_cards` + types + unit tests | Create |
| `crates/dat0-app/src/inspector/mod.rs` | `pub mod projection;` + re-exports | Modify |
| `crates/dat0-app/src/inspector/model.rs` | `hidden_expanded: bool` + `toggle_hidden`; clear on target change | Modify |
| `crates/dat0-app/src/inspector/panel.rs` | Render visible cards (label + subtle original) + collapsible Hidden section; take `Option<ProjectionContext>` | Modify |
| `crates/dat0-app/src/window.rs` | `inspector_projection()` guard helper; pass it into `render_inspector`; verify notify on `column_view` change | Modify |
| `crates/dat0-app/src/actions/view_actions.rs` | Update the dispatch-seam comment (re-project, not re-profile) | Modify |
| `crates/dat0-app/src/view/mod.rs` | Update the guard-test comment (re-project, not re-profile) | Modify |
| `crates/dat0-i18n/src/strings/en.json` | `inspector.hidden`, `inspector.col.was` | Modify |
| `docs/catalog-inspector.md` | Document projection-aware Inspector + the Hidden section | Modify |

## Scope / non-goals (YAGNI)

- **No profiling change.** Stats are never recomputed for a projection tweak.
- **No session persistence** of `hidden_expanded` (ephemeral, no schema bump).
- **No per-column "hide from Inspector"** independent of the grid — the Inspector strictly follows the grid's projection.
- **Filters/edits** are out of scope here: they already flow into the Current-view profile via `compile_view_sql`; this feature is only about the display-only *column* projection.
- The `inspector_projection()` guard returning `None` for a non-active target is intentional graceful degradation, not a feature to expand.

## Manual UAT (owed; joins the standing P4b/P4c/P5b/P5c/P6a/P6b backlog)

1. Import a CSV; reorder two columns in the grid → Inspector cards reorder to match.
2. Rename a column → its card header shows the new name with a subtle `· was <orig>`; stats unchanged.
3. Hide a column → its card leaves the visible list and appears under a collapsed "Hidden (1)"; expand to see it dimmed; un-hide → it returns to the visible list in order.
4. Confirm `__dat0_rowid` never shows as a card.
5. Toggle Whole-table ⇄ Current-view → the column set/order/labels stay grid-matched in both (only row-scoped stats differ).
6. Undo/redo a reorder/rename/hide → the Inspector re-projects immediately (no profile flicker / no stale labels).
7. Open a second table tab, inspect it, switch back → the Inspector projection tracks the inspected table (no cross-table mis-projection).
