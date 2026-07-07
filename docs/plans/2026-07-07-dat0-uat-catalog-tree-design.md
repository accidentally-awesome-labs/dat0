# UAT Slice — Catalog tree: hierarchy + keyboard nav (design)

> **Date:** 2026-07-07 · **Branch:** `uat-catalog-tree` off `main` (`8473801`)
> Second of the **deferred internal keyboard-nav slices** (the Slice-6 carve-out),
> after recents-nav (PR #46). Unlike recents, this slice is **feature + nav in
> one**: the catalog panel today renders a *flat* list (`CatalogNode.children` is
> built empty and ignored by the render), so "tree keyboard nav" first requires
> building the tree.

## Problem

Two gaps, deliberately combined (user scope decision):

1. **Product gap — no hierarchy.** `catalog/panel.rs` renders four flat sections
   (Sources / Cloud / Tables / Derived). Tables from an *attached database*
   (SQLite file or MotherDuck `md:`) appear as loose flat rows indistinguishable
   from file-backed tables — the attach itself has no node, nothing groups its
   tables, and `CatalogNode.children` ("For attached-DB source nodes: the tables
   inside") has never been populated.
2. **A11y gap — no keyboard nav.** Catalog rows are plain
   `div().id("cat-{sec}-{name}").on_click(..)` — no `tab_index`, no
   `FocusHandle`, no keyboard activation, no ring (P10b `docs/a11y.md` A1). This
   is the biggest remaining kbd-nav surface after Slice 6 + recents-nav.

## Scope decisions (locked in brainstorming)

- **Hierarchy first, then tree-nav** — build the real tree in this slice (not
  nav-only over the flat list).
- **Attach-parents only (depth 2).** One parent node per attached DB (grouped
  by `alias`); its tables are children. File / local / Derived rows stay flat
  leaves. No schema-level grouping; section headers stay non-interactive.
- **Collapse state persists in the session — schema v9→v10.**
- **ARIA-tree core keys**: ↑ ↓ ← → + Enter/Space, one tab stop for the panel.
  No Home/End/type-ahead (deferred).
- **One combined slice/PR** (not two stacked slices).

## Design

### 1. Model (pure, `catalog/tree.rs`)

`CatalogTree::build` groups `TableOrigin::Attached` tables by `alias`: one
parent `CatalogNode { name: alias, children: Vec<String> = table names }` per
attach. Parents whose `source` starts with `md:` go to `cloud`; other attaches
to `sources` (routing rule unchanged, now applied to the parent). `File`,
local (`tables`), and `derived` nodes stay flat leaves with empty `children`.
Deterministic order: parents by alias, children by table name, leaves by name.

`filter` semantics defined now (dormant API — zero prod call sites — but the
first search-box slice must not inherit undefined behavior): a parent survives
if its **alias** matches OR **any child** matches; a surviving parent keeps only
its matching children, except an alias-match keeps all children.

### 2. Session v10 (`session/mod.rs`, `session/migrate.rs`)

- New UI-state field **`catalog_collapsed: Vec<String>`** (attach aliases),
  `#[serde(default)]` — empty = all expanded (today's visual behavior
  preserved on fresh/migrated sessions).
- `SESSION_n` 9→10 with an **explicit literal migration arm** in `migrate.rs`
  (the file's own rule: literal version arms, never `n if n == SESSION_n`).
  v9→v10 is a pure default-fill.
- `session_migration.rs` tests + insta wire snapshots updated (Slice-3
  precedent: `assert_snapshot!` on `serde_json::to_string_pretty`; accept by
  renaming `.snap.new` → `.snap`, strip `assertion_line`).
- `WorkspaceShell` mirrors the field as `catalog_collapsed: HashSet<String>`,
  restored on load and written back on session snapshot (same shape as
  `catalog_panel_visible` at window.rs:2282/3937).

### 3. Flatten seam (pure — the load-bearing piece)

```rust
/// Every VISIBLE catalog row, in render order. Children of collapsed parents
/// are absent. Section headers are NOT rows (non-interactive; nav skips them).
fn visible_rows(tree: &CatalogTree, collapsed: &HashSet<String>) -> Vec<CatalogRow>

struct CatalogRow { section: &'static str, kind: RowKind }
enum RowKind {
    Parent { alias: String, expanded: bool, n_children: usize },
    Leaf   { name: String, depth: u8 },   // 0 = top-level, 1 = child of a parent
}
```

**Single source of truth for BOTH the render iteration AND the nav index** —
the ring, arrow moves, Enter target, and painted rows are all derived from one
`Vec`, so they cannot drift (the recents M1 "single clamp site" lesson,
generalized). `catalog_active` is clamped to `visible_rows.len()-1` at ONE site
before every use.

### 4. Nav transitions (pure)

```rust
enum NavAction { Move(usize), Toggle(String /* alias */), Open(String /* table */), None }
fn tree_nav(rows: &[CatalogRow], active: usize, key: &str) -> NavAction
```

Slice-4 `resolve_relaunch_action` idiom: pure fn → enum → thin match in the
GPUI closure. Transition table:

| key | on collapsed parent | on expanded parent | on child leaf | on top-level leaf |
|---|---|---|---|---|
| ↓ / ↑ | `Move(±1)` clamped at both ends, linear over `rows` | ” | ” | ” |
| → | `Toggle` (expand) | `Move(first child)` | `None` | `None` |
| ← | `None` | `Toggle` (collapse) | `Move(parent index)` | `None` |
| Enter/Space | `Toggle` | `Toggle` | `Open(name)` | `Open(name)` |

### 5. Render (`catalog/panel.rs`)

The panel iterates `visible_rows` output (grouped under the existing four
section headers). Header label stays `section_label(name, n)` where **`n` = the
section's top-level node count** — an attach = 1 parent, so Slice-5's
`Cloud (1)` teeth survive grouping unchanged.

- **Parent row (new):** `▸`/`▾` chevron + alias + child count;
  `on_click → ws.toggle_catalog_parent(alias)` — a NEW single-source shell
  method (flip alias in `catalog_collapsed`, clamp `catalog_active`,
  `cx.notify()`); the keyboard `Toggle` arm calls the SAME method (Slice-6
  single-source rule: mouse and keyboard cannot drift). Parent rows chain
  `.a11y_label(Label, alias-row-text)` onto the existing row div.
- **Child rows:** indented `pl_4`; keep today's `on_click → open_table_tab` and
  the existing `.a11y_label(name)` chain (Slice-5 seam preservation — chain
  onto existing elements, NO new wrapper divs).
- **Active ring:** on row `i == catalog_active` only (grid `is_active` idiom,
  decoupled from gpui focus).
- Row `ElementId` stays section-qualified (`cat-{sec}-{name}`); parents get
  `cat-{sec}-attach-{alias}` (avoids collision with a same-named table).

### 6. Focus / kbd wiring (`window.rs`)

ONE container `focus_stop("catalog-tree", &fh, 0, activate)` on the catalog
dock container:

- Fixed id `"catalog-tree"` in the existing `hero_focus` map
  (`hero_focus_handle` get-or-insert — the map is generic shell state, already
  reused by recents).
- Tab stop **gated on `catalog_panel_visible`** (grid `grid_visible` idiom) —
  hero/settings Tab-sequence tests unaffected when the panel is closed.
- `.a11y("catalog-tree", Button, t("catalog.title"))` twin — SAME static id,
  the focus-oracle label source (`focused_label()` compares label TEXT).
- Chained **second** `.on_key_down` for ↑ ↓ ← → (recents R1 PROVEN: gpui
  pushes `key_down_listeners`, both fire; fallback = single unified
  `on_key_down` + no-op activate).
- **`catalog_active: usize`** — NEW field on the persistent shell (never the
  render-pass locals). Both closures do: build `visible_rows` → clamp →
  `tree_nav` → `match NavAction`.

## Test harness & coverage

### Units (no GPUI)

- **Build-grouping** (`tree.rs`): alias grouping; md: parent → `cloud`, sqlite
  parent → `sources`; single-table attach still gets a parent; deterministic
  sort; File/Derived untouched.
- **`filter` with parents**: alias-match keeps all children; child-match prunes
  siblings; no-match drops the parent.
- **`visible_rows`**: collapse hides exactly that parent's children; expand
  restores; order = render order; depth/section correct.
- **`tree_nav` transition table** (~10 cases incl. clamps at both ends,
  ←-from-child → parent index, →-into-first-child).
- **Session v10**: migration arm (v9 file loads, `catalog_collapsed` defaults
  empty) + wire snapshot with a non-empty collapsed set.

### GPUI (`tests/catalog_nav.rs`, a11y-capture, full shell mount)

Reuses `seed_catalog_tree_for_test` (Slice 5) — the shim calls
`CatalogTree::build` directly, so grouping flows through with zero shim change.
Seed: 1 file table + 1 sqlite attach (2 tables) + 1 md attach (1 table).

1. **Tab reaches the tree** — `press_tab` loop until
   `focused_label() == Some(t("catalog.title"))`; panel-hidden negative
   (stop absent when `catalog_panel_visible == false`).
2. **↓/↑ move the active index** — `catalog_active_for_test()` accessor
   (`#[cfg(a11y-capture)]`), clamp at both ends.
3. **← on a child jumps to its parent** (index assertion).
4. **← on the expanded parent collapses** — child label VANISHES from the a11y
   tree (absence teeth) + `visible_rows` count shrinks + active stays clamped.
5. **→ re-expands** — child label reappears.
6. **Enter on a parent toggles** (same observables as 4/5, via the activate
   path).
7. **Collapse persists** — toggle, drive a session save, assert the alias
   appears in the saved session JSON (primary observable). A full
   reload-restores round-trip is optional strengthening; the plan decides
   after T0 shows what a headless save/load costs.
8. **Enter on a leaf → `open_table_tab`** — **T0 decides** drivable vs
   stays-human (recents precedent: the real open stayed human; the `Open` arm
   of `tree_nav` is unit-covered either way).

**Cross-binary gate:** controller `cargo test --workspace --no-fail-fast`
(Slice-6 frame-count-drift lesson). Sharpest edge: **`motherduck_window` (5
tests) must stay green** — grouping changes what Slice-5's seeded md table
renders under. Expected-survive: header count semantics (top-level nodes) +
default-expanded (the table row label still renders). T0 verifies empirically.

## Seam / release cost

The hierarchy render, chevrons, `toggle_catalog_parent`, `catalog_collapsed`,
`catalog_active`, `focus_stop`, and the arrow handler all ship
**unconditionally** — genuine product feature + a11y fix (3rd
production-shipping slice, after Slice 6 and recents-nav). Only
`catalog_active_for_test` / `catalog_collapsed_for_test` accessors and the
oracle's `record_focus_id` are `#[cfg(a11y-capture)]`. New a11y nodes: parent
rows' `.a11y_label` chains (existing divs, no wrappers) + ONE `"catalog-tree"`
container twin. Zero new deps expected (D-015 stays open).

## Risks / T0 spike (HARD GATE — spike EVERY asserted surface)

- **R1 — arrows + focus_stop coexistence on THIS container.** Proven on recents
  (gpui 0.2.2 listener-push), but Slice-3's lesson says prove it per-surface:
  T0 must show ArrowDown mutates `catalog_active` on the catalog container.
  Fallback: unified single `on_key_down`.
- **R2 — collapsed-child absence is visible to `A11ySnapshot`.** The absence
  teeth (test 4) require the child's `.a11y_label` node to genuinely disappear
  when the render skips the row. Should follow from render-conditioned seams
  (Slice-3 rule), but T0 asserts it.
- **R3 — Slice-5 `motherduck_window` breakage.** T0 runs the focused suite
  post-grouping BEFORE any T1 code. If the header-count or row-label teeth
  break, fix the semantics (count = top-level nodes) or update Slice-5
  assertions consciously — never silently.
- **R4 — session v10 churn.** Migration arm + wire snapshots + any test fixture
  writing `"n": 9`. `rg '"n": *9'` across tests; update deliberately.
- **R5 — Enter-on-leaf drivability.** `open_table_tab` does off-thread engine
  work via the dispatcher (warns "catalog stale" without one). T0 probes
  whether the test-engine path is graceful (Slice-3 reopen precedent: tokio
  runtime sufficed) or the open stays human.
- **R6 — cross-binary frame-count drift.** `a11y_spike.rs` (the only exact
  node-count assertion) mounts with the catalog panel hidden → likely zero
  drift; workspace gate is the backstop.

## Stays human (owed glance grows)

- Chevron / indent / active-row ring pixels + container/row DOUBLE-ring +
  WCAG ≥3:1 both themes — joins the standing About / Charts / Settings /
  Slice-6 / recents glances.
- Live attached-DB grouping against a REAL engine (`refresh_catalog`'s
  off-thread `get_tables` → build → panel) — real SQLite/MotherDuck attach
  round-trip.

## Build cadence (SDD)

T0 spike **hard gate** (R1–R6 empirically resolved) → T1 pure model
(build-grouping + `visible_rows` + `tree_nav` + units) → T2 session v10
(migration + snapshots) → T3 render (hierarchy + chevron + ring + toggle
method) → T4 focus/kbd wiring + `tests/catalog_nav.rs` → per-task spec+quality
reviews → final opus whole-branch review → green **both** platforms → squash →
**watch the post-merge main run** (macOS grid-scroll bench is push-to-main-only
→ can redden main silently). Implementers run only the fast focused test;
controller runs the workspace + clippy gate (anti-loop rule). `cargo fmt --all`
before every commit (Slice-5 lesson).
