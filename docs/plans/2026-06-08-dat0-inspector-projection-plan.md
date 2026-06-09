# Projection-Aware Inspector — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Inspector mirror the grid's display-only column projection — cards in grid order, renamed labels, hidden columns under a collapsed "Hidden" section, surrogate omitted — without re-profiling.

**Architecture:** Display-layer only (Approach A). A pure `inspector::projection::project_cards` re-arranges the already-computed profile cards from the live `WorkspaceShell.column_view`; a shell guard supplies the projection only when the Inspector targets the active grid tab; the panel renders the result. Engine untouched. No session-schema change.

**Tech Stack:** Rust, GPUI (hand-rolled divs), `dat0-engine` (`ProjectionColumn`, `ColumnProfile`, `ROWID_COL`), `dat0-i18n`.

**Design doc:** `docs/plans/2026-06-08-dat0-inspector-projection-design.md`

**Conventions (verified live):**
- Inspector panel is a free function `render_inspector(&InspectorModel, cx)` at `crates/dat0-app/src/inspector/panel.rs:25`, called once from `WorkspaceShell::render` at `crates/dat0-app/src/window.rs:3439`.
- `WorkspaceShell.column_view: Vec<dat0_engine::transform::ProjectionColumn>` (`window.rs:643`), refreshed by `refresh_column_view()` (`window.rs:996`) from `data_source.visible_column_names()` + `view_model.active()`.
- `ProjectionColumn { source: String, display: String }`; surrogate const `dat0_engine::transform::ROWID_COL` (`"__dat0_rowid"`).
- `ColumnProfile { name, ty, null_pct, approx_distinct, count, numeric, length }`; `TableProfile { rows, columns: Vec<ColumnProfile> }`.
- Click wiring idiom: `cx.listener(|ws, _ev, window, cx| …)` (see the mode-toggle at `panel.rs:62`).
- i18n: single locale `crates/dat0-i18n/src/strings/en.json`; lookup `dat0_i18n::t("key") -> String`.
- Local gate before each commit: `cargo test -p dat0-engine -p dat0-app`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

---

## File Structure

| File | Responsibility | Action |
|------|----------------|--------|
| `crates/dat0-app/src/inspector/projection.rs` | Pure `project_cards` + `ProjectionContext`/`RenderCard`/`ProjectedCards` + unit tests | Create |
| `crates/dat0-app/src/inspector/mod.rs` | `pub mod projection;` | Modify |
| `crates/dat0-app/src/inspector/model.rs` | `hidden_expanded: bool` + `toggle_hidden`; clear on target change | Modify |
| `crates/dat0-app/src/inspector/panel.rs` | Render projected visible cards + collapsible Hidden section; new `Option<ProjectionContext>` param | Modify |
| `crates/dat0-app/src/window.rs` | `inspector_projection()` guard; pass into `render_inspector`; notify on display-only rebind | Modify |
| `crates/dat0-app/src/actions/view_actions.rs` | `cx.notify()` on display-only undo/redo; update seam comment | Modify |
| `crates/dat0-app/src/view/mod.rs` | Update the guard-test comment (re-project, not re-profile) | Modify |
| `crates/dat0-i18n/src/strings/en.json` | `inspector.hidden`, `inspector.col.was` | Modify |
| `docs/catalog-inspector.md` | Document projection-aware Inspector + Hidden section | Modify |

---

## Task 1: Pure `project_cards` + types

**Files:**
- Create: `crates/dat0-app/src/inspector/projection.rs`
- Modify: `crates/dat0-app/src/inspector/mod.rs`
- Test: same file (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing tests**

Put at the bottom of the new `projection.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::transform::ROWID_COL;
    use dat0_engine::ColumnProfile;

    fn col(name: &str) -> ColumnProfile {
        ColumnProfile {
            name: name.into(),
            ty: "T".into(),
            null_pct: 0.0,
            approx_distinct: 0,
            count: 0,
            numeric: None,
            length: None,
        }
    }

    fn pc(source: &str, display: &str) -> ProjectionColumn {
        ProjectionColumn { source: source.into(), display: display.into() }
    }

    // Profile carries 3 user columns + the surrogate (both modes do).
    fn profile() -> Vec<ColumnProfile> {
        vec![col("a"), col("b"), col("c"), col(ROWID_COL)]
    }

    #[test]
    fn no_projection_shows_all_user_columns_minus_surrogate() {
        let out = project_cards(&profile(), None);
        assert_eq!(
            out.visible.iter().map(|c| c.label.clone()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(out.visible.iter().all(|c| c.original.is_none()));
        assert!(out.hidden.is_empty());
    }

    #[test]
    fn reorder_and_rename_follow_the_projection() {
        // Grid shows c, then b renamed to "Bee"; "a" is hidden.
        let ctx = ProjectionContext {
            visible: vec![pc("c", "c"), pc("b", "Bee")],
            base_sources: vec!["a".into(), "b".into(), "c".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));

        assert_eq!(
            out.visible.iter().map(|c| (c.source.clone(), c.label.clone(), c.original.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("c".into(), "c".into(), None),
                ("b".into(), "Bee".into(), Some("b".into())),
            ]
        );
        // "a" is in base but not visible → hidden.
        assert_eq!(
            out.hidden.iter().map(|c| c.source.clone()).collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(out.hidden[0].label, "a");
        assert!(out.hidden[0].original.is_none());
    }

    #[test]
    fn surrogate_is_omitted_with_projection_too() {
        let ctx = ProjectionContext {
            visible: vec![pc("a", "a")],
            base_sources: vec!["a".into(), "b".into(), "c".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));
        assert!(out.visible.iter().chain(out.hidden.iter()).all(|c| c.source != ROWID_COL));
        // b and c are hidden; surrogate not present anywhere.
        assert_eq!(
            out.hidden.iter().map(|c| c.source.clone()).collect::<Vec<_>>(),
            vec!["b", "c"]
        );
    }

    #[test]
    fn visible_source_without_profile_column_is_skipped() {
        let ctx = ProjectionContext {
            visible: vec![pc("ghost", "ghost"), pc("a", "a")],
            base_sources: vec!["a".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));
        assert_eq!(
            out.visible.iter().map(|c| c.source.clone()).collect::<Vec<_>>(),
            vec!["a"]
        );
    }

    #[test]
    fn profile_column_absent_from_base_is_dropped() {
        // "c" exists in the profile but not in base_sources (and isn't the
        // surrogate) → it appears in neither list.
        let ctx = ProjectionContext {
            visible: vec![pc("a", "a")],
            base_sources: vec!["a".into(), "b".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));
        let all: Vec<String> =
            out.visible.iter().chain(out.hidden.iter()).map(|c| c.source.clone()).collect();
        assert!(!all.contains(&"c".to_string()));
        assert_eq!(out.hidden.iter().map(|c| c.source.clone()).collect::<Vec<_>>(), vec!["b"]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p dat0-app --lib inspector::projection`
Expected: FAIL — `project_cards` / types not found.

- [ ] **Step 3: Write the module**

Top of `crates/dat0-app/src/inspector/projection.rs`:

```rust
//! Pure projection of Inspector cards onto the grid's display-only column
//! projection (P-projection). The Inspector profiles every base column (both
//! Whole-table and Current-view modes; projection is a no-op in
//! `compile_view_sql`). This module re-arranges those already-computed cards to
//! match the grid: visible columns in `column_view` order with renamed labels,
//! the rest under a "hidden" list, the internal surrogate always dropped. No
//! engine/GPUI dependency — fully unit-testable.
use dat0_engine::transform::{ProjectionColumn, ROWID_COL};
use dat0_engine::ColumnProfile;
use std::collections::HashSet;

/// The active grid tab's projection, supplied by `WorkspaceShell` only when the
/// Inspector targets that tab's table (else `None` → no-projection fallback).
#[derive(Debug, Clone)]
pub struct ProjectionContext {
    /// Grid-visible columns in display order (the folded `column_view`).
    pub visible: Vec<ProjectionColumn>,
    /// All non-surrogate base column names (to derive the hidden set).
    pub base_sources: Vec<String>,
}

/// One rendered card's identity: which profile column it maps to, the header
/// label to show, and the original name when the column was renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCard {
    pub source: String,         // keys into `TableProfile.columns` by `.name`
    pub label: String,          // header text (renamed display label, or source)
    pub original: Option<String>, // Some(source) only when renamed (display != source)
}

/// The Inspector's cards split into grid-visible and hidden lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectedCards {
    pub visible: Vec<RenderCard>,
    pub hidden: Vec<RenderCard>,
}

/// Re-arrange `profile_cols` to match the grid projection `ctx`. Pure.
///
/// - The surrogate (`ROWID_COL`) is always dropped.
/// - `None` (Inspector not on the active grid table) → every non-surrogate
///   column visible in profile order, nothing hidden.
/// - `Some(ctx)` → `ctx.visible` order with renamed labels; columns in
///   `ctx.base_sources` but not visible become `hidden`; profile columns absent
///   from `base_sources` (and not the surrogate) are dropped.
pub fn project_cards(profile_cols: &[ColumnProfile], ctx: Option<&ProjectionContext>) -> ProjectedCards {
    let has = |name: &str| profile_cols.iter().any(|c| c.name == name);

    let Some(ctx) = ctx else {
        let visible = profile_cols
            .iter()
            .filter(|c| c.name != ROWID_COL)
            .map(|c| RenderCard { source: c.name.clone(), label: c.name.clone(), original: None })
            .collect();
        return ProjectedCards { visible, hidden: Vec::new() };
    };

    let mut visible_sources: HashSet<&str> = HashSet::new();
    let mut visible = Vec::new();
    for p in &ctx.visible {
        if p.source == ROWID_COL || !has(&p.source) {
            continue; // surrogate, or a projection col with no profile row (defensive)
        }
        visible_sources.insert(p.source.as_str());
        let original = (p.display != p.source).then(|| p.source.clone());
        visible.push(RenderCard { source: p.source.clone(), label: p.display.clone(), original });
    }

    let hidden = ctx
        .base_sources
        .iter()
        .filter(|s| s.as_str() != ROWID_COL && !visible_sources.contains(s.as_str()) && has(s))
        .map(|s| RenderCard { source: s.clone(), label: s.clone(), original: None })
        .collect();

    ProjectedCards { visible, hidden }
}
```

- [ ] **Step 4: Register the module**

In `crates/dat0-app/src/inspector/mod.rs`, add alongside the other `pub mod` lines:

```rust
pub mod projection;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p dat0-app --lib inspector::projection`
Expected: PASS (all five).

- [ ] **Step 6: Gate + commit**

```bash
cargo clippy -p dat0-app --lib -- -D warnings
cargo fmt -p dat0-app
git add crates/dat0-app/src/inspector/projection.rs crates/dat0-app/src/inspector/mod.rs
git commit --signoff -m "feat(inspector): pure project_cards — projection-aware card layout"
```

---

## Task 2: Inspector model — `hidden_expanded`

**Files:**
- Modify: `crates/dat0-app/src/inspector/model.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `model.rs`:

```rust
    #[test]
    fn hidden_section_toggles_and_resets_on_table_change() {
        let mut m = InspectorModel::new();
        m.set_target("orders".into());
        assert!(!m.hidden_expanded, "collapsed by default");
        m.toggle_hidden();
        assert!(m.hidden_expanded, "toggles open");
        m.set_target("orders".into()); // same table → state preserved (no reset branch)
        assert!(m.hidden_expanded, "re-targeting the SAME table keeps the section state");
        m.set_target("customers".into()); // different table → reset branch fires
        assert!(!m.hidden_expanded, "switching tables collapses the Hidden section");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p dat0-app --lib inspector::model::tests::hidden_section_toggles`
Expected: FAIL — no field `hidden_expanded` / no `toggle_hidden`.

- [ ] **Step 3: Add the field**

In `crates/dat0-app/src/inspector/model.rs`, add to the `InspectorModel` struct (after the `lineage` field):

```rust
    /// Whether the Inspector's collapsible "Hidden" section is expanded
    /// (projection-aware Inspector). Ephemeral UI state — not persisted; reset
    /// to collapsed whenever the inspected table changes.
    pub hidden_expanded: bool,
```

(`InspectorModel` derives `Default`; `bool` defaults to `false`, so the derive still holds.)

- [ ] **Step 4: Add the toggle + reset-on-target-change**

Add the method (next to `set_lineage`):

```rust
    /// Flip the "Hidden" section's expanded state (projection-aware Inspector).
    pub fn toggle_hidden(&mut self) {
        self.hidden_expanded = !self.hidden_expanded;
    }
```

In `set_target`, inside the `if self.target_table.as_deref() != Some(table.as_str())` block (the existing branch that clears `column_extras` on a real table change), also reset the section:

```rust
            self.column_extras.clear();
            self.hidden_expanded = false; // collapse the Hidden section for the new table
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p dat0-app --lib inspector::model`
Expected: PASS.

- [ ] **Step 6: Gate + commit**

```bash
cargo clippy -p dat0-app --lib -- -D warnings
cargo fmt -p dat0-app
git add crates/dat0-app/src/inspector/model.rs
git commit --signoff -m "feat(inspector): hidden_expanded state + toggle (reset on table change)"
```

---

## Task 3: i18n + projection-aware render (panel + window guard + wiring)

**Files:**
- Modify: `crates/dat0-i18n/src/strings/en.json`
- Modify: `crates/dat0-app/src/inspector/panel.rs`
- Modify: `crates/dat0-app/src/window.rs`

This is one compile-unit: changing `render_inspector`'s signature breaks its single call site in `window.rs`, so the panel render and the window guard/wiring land together.

- [ ] **Step 1: Add i18n keys**

In `crates/dat0-i18n/src/strings/en.json`, add (next to the other `inspector.*` keys — keep valid JSON with a trailing comma before the following key):

```json
  "inspector.hidden": "Hidden",
  "inspector.col.was": "was",
```

- [ ] **Step 2: Add the `inspector_projection` guard to `window.rs`**

In `crates/dat0-app/src/window.rs`, add this method to the `impl WorkspaceShell` block (near `refresh_column_view`, around line 996):

```rust
    /// The active grid tab's column projection, for the Inspector to mirror —
    /// but only when the Inspector is actually targeting that tab's table, so
    /// inspecting table X while the grid shows Y never mis-projects. `None`
    /// otherwise (no view/data source, or a cross-table target) → the Inspector
    /// falls back to its raw, unprojected card list. Identity is the bare
    /// (unquoted) table name, consistent with the app's catalog/lineage keying.
    pub(crate) fn inspector_projection(
        &self,
    ) -> Option<crate::inspector::projection::ProjectionContext> {
        let target = self.inspector.target_table.as_deref()?;
        let vm = self.view_model.as_ref()?;
        let ds = self.data_source.as_ref()?;
        // `base_table()` is quoted `"schema"."table"`; reduce to the bare name.
        let active = vm
            .base_table()
            .rsplit('.')
            .next()
            .unwrap_or("")
            .trim_matches('"');
        if active != target {
            return None;
        }
        Some(crate::inspector::projection::ProjectionContext {
            visible: self.column_view.clone(),
            base_sources: ds.visible_column_names(),
        })
    }
```

- [ ] **Step 3: Pass the projection into the panel at the render call site**

In `crates/dat0-app/src/window.rs` at the `render_inspector` call (around line 3439), change:

```rust
                            .child(crate::inspector::panel::render_inspector(
                                &self.inspector,
                                cx,
                            ))
```

to:

```rust
                            .child(crate::inspector::panel::render_inspector(
                                &self.inspector,
                                self.inspector_projection(),
                                cx,
                            ))
```

- [ ] **Step 4: Update the panel signature + render**

In `crates/dat0-app/src/inspector/panel.rs`, change the imports (line ~16) to add the projection types:

```rust
use crate::inspector::projection::{project_cards, ProjectionContext, RenderCard};
use crate::inspector::{InspectorModel, ProfileTargetMode, format};
```

Change the `render_inspector` signature (line 24) to take the projection:

```rust
pub fn render_inspector(
    model: &InspectorModel,
    projection: Option<ProjectionContext>,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
```

Replace the per-column cards block (the current `if let Some(profile) = model.cached() { … for col in &profile.columns { cards.child(column_card(col, model)) } … }`, around lines 84-91) with the projected render:

```rust
    // Per-column cards (only when a profile is cached). Projection-aware: cards
    // follow the grid's column projection (order + renames), hidden columns go
    // under a collapsible "Hidden" section, the surrogate is dropped. When the
    // Inspector targets a non-active table, `projection` is None → raw list.
    if let Some(profile) = model.cached() {
        let cards = project_cards(&profile.columns, projection.as_ref());

        let mut visible = div().flex().flex_col().gap_2();
        for card in &cards.visible {
            if let Some(col) = profile.columns.iter().find(|c| c.name == card.source) {
                visible = visible.child(column_card(col, card, model, false));
            }
        }
        root = root.child(visible);

        if !cards.hidden.is_empty() {
            let header = format!(
                "{} ({})",
                dat0_i18n::t("inspector.hidden"),
                cards.hidden.len()
            );
            let caret = if model.hidden_expanded { "▾" } else { "▸" };
            let mut section = div().flex().flex_col().gap_2().child(
                div()
                    .id("inspector-hidden-toggle")
                    .cursor_pointer()
                    .child(SharedString::from(format!("{caret} {header}")))
                    .on_click(cx.listener(|ws, _ev, _window, cx| {
                        ws.inspector.toggle_hidden();
                        cx.notify();
                    })),
            );
            if model.hidden_expanded {
                for card in &cards.hidden {
                    if let Some(col) = profile.columns.iter().find(|c| c.name == card.source) {
                        section = section.child(column_card(col, card, model, true));
                    }
                }
            }
            root = root.child(section);
        }
    }
```

- [ ] **Step 5: Update `column_card` to take the card label + dimmed flag**

In `crates/dat0-app/src/inspector/panel.rs`, change `column_card` (line 99) to render the projection label/original and support dimming:

```rust
/// One column card: the projected header (label, plus a subtle "· was <orig>"
/// when renamed), the three formatted stat lines, then — when its lazy data has
/// landed (T10) — an inline chart. `dimmed` styles hidden-section cards.
fn column_card(
    col: &dat0_engine::ColumnProfile,
    card: &RenderCard,
    model: &InspectorModel,
    dimmed: bool,
) -> gpui::Div {
    let header = match &card.original {
        Some(orig) => format!("{} · {}  ·  {} {}", card.label, col.ty, dat0_i18n::t("inspector.col.was"), orig),
        None => format!("{} · {}", card.label, col.ty),
    };
    let stats = format::format_stats_line(col);

    let mut card_div = div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .border_1()
        .child(div().child(SharedString::from(header)));
    if dimmed {
        card_div = card_div.opacity(0.55);
    }

    if !stats.is_empty() {
        card_div = card_div.child(div().child(SharedString::from(stats)));
    }
    card_div = card_div.child(div().child(SharedString::from(format::format_distinct(col))));
    card_div = card_div.child(div().child(SharedString::from(format::format_null(col))));

    // Inline chart (T10): top-N bars for low-card, histogram for numeric — only
    // when its lazy data has been fetched (see `load_column_extras`). Keyed by
    // the real source column name, not the renamed label.
    if let Some(extra) = model.extra(&col.name) {
        if let Some(topn) = &extra.topn {
            card_div = card_div.child(crate::charts::render_topn(topn));
        } else if let Some(bins) = &extra.histogram {
            card_div = card_div.child(crate::charts::render_histogram(bins));
        }
    }
    card_div
}
```

> Verify `.opacity(f32)` is the in-tree dim idiom; if GPUI's `Styled` lacks `.opacity`, dim via a muted text color used elsewhere in `panel.rs` (match the existing pattern — do not invent an API). The caret glyphs `▾`/`▸` are plain strings; keep or swap to the house convention.

- [ ] **Step 6: Build + suite**

Run: `cargo build -p dat0-app && cargo test -p dat0-engine -p dat0-app`
Expected: compiles; suite green (the pure `project_cards` tests carry the logic; this task is GPUI wiring).

- [ ] **Step 7: Gate + commit**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
git add crates/dat0-i18n/src/strings/en.json crates/dat0-app/src/inspector/panel.rs crates/dat0-app/src/window.rs
git commit --signoff -m "feat(inspector): render cards in grid projection + collapsible Hidden section"
```

---

## Task 4: Reactivity on display-only undo/redo + docs

**Files:**
- Modify: `crates/dat0-app/src/actions/view_actions.rs`
- Modify: `crates/dat0-app/src/view/mod.rs`
- Modify: `docs/catalog-inspector.md`

A display-only undo/redo updates `column_view` but currently does not repaint (the `workspace.update` closure ignores its `cx`). Without a repaint the Inspector (and the grid header) keep the pre-undo projection. This task adds the single missing `cx.notify()` so the Inspector re-projects — still **without** re-profiling (the merged guard `display_only_undo_keeps_inspector_profile_source_stable` stays valid).

- [ ] **Step 1: Notify on display-only undo**

In `crates/dat0-app/src/actions/view_actions.rs`, in `dispatch_undo`, the `workspace.update(app, |ws, _cx| { … ws.refresh_column_view(); change })` block currently ignores `cx`. Change the closure to use `cx` and notify after `refresh_column_view()`:

```rust
        let change = workspace.update(app, |ws, cx| {
            let change = ws.view_model.as_mut().and_then(|vm| vm.undo());
            // Refresh the ColumnView off the new active stack (P4c T5). A
            // display-only undo (undoing a Rename/Reorder/DeleteColumn) never
            // round-trips through `apply_view_change`, so this is the only hook
            // that keeps the grid header AND the projection-aware Inspector fresh.
            //
            // The Inspector is re-PROJECTED here (it reads the live `column_view`
            // on render), but never re-PROFILED — projection ops are no-ops in
            // `compile_view_sql`, so the SUMMARIZE source is unchanged (guarded by
            // `view::consumer_tests::display_only_undo_keeps_inspector_profile_source_stable`).
            // For a real data-view undo the notify is harmless (apply_view_change
            // repaints + re-profiles again on rebind).
            ws.refresh_column_view();
            cx.notify();
            change
        });
```

- [ ] **Step 2: Notify on display-only redo**

In `dispatch_redo`, apply the symmetric change — replace its `workspace.update(app, |ws, _cx| { … ws.refresh_column_view(); change })` with:

```rust
        let change = workspace.update(app, |ws, cx| {
            let change = ws.view_model.as_mut().and_then(|vm| vm.redo());
            // Symmetric to dispatch_undo — re-project the Inspector + refresh the
            // grid header on a display-only redo (re-projected, not re-profiled).
            ws.refresh_column_view();
            cx.notify();
            change
        });
```

- [ ] **Step 3: Update the guard-test comment**

In `crates/dat0-app/src/view/mod.rs`, the doc comment on `display_only_undo_keeps_inspector_profile_source_stable` says the Inspector is not refreshed on this path. Update the first sentence so it reflects the projection-aware behavior — the Inspector is now re-projected (re-rendered) on a display-only undo/redo but still not re-profiled, which is exactly what this test guards:

```rust
    /// PD-022 follow-up — a display-only undo/redo re-PROJECTS the Inspector
    /// (cards re-arrange to the new column projection) but never re-PROFILES it:
    /// the profiled SQL is unchanged. `dispatch_undo`/`dispatch_redo` `cx.notify()`
    /// to re-render (cheap), and this guard pins that no re-SUMMARIZE is needed.
```

(Keep the rest of the existing doc comment + the test body unchanged.)

- [ ] **Step 4: Build + suite**

Run: `cargo build -p dat0-app && cargo test -p dat0-engine -p dat0-app`
Expected: compiles; suite green (incl. the unchanged guard test).

- [ ] **Step 5: Document the feature**

In `docs/catalog-inspector.md`, update the Inspector section: the per-column cards now mirror the grid's column projection — same order, renamed labels (shown as `New name · was <original>`), and columns you've hidden move to a collapsed **"Hidden (N)"** section you can expand; the internal row-id surrogate is not shown. This holds in both Whole-table and Current-view modes (the toggle changes only which rows are profiled). Also correct the older `## Inspector` bullet / "Live refresh on edits" wording if it implies the cards are in physical-table order.

- [ ] **Step 6: Final gate + commit**

```bash
cargo test -p dat0-engine -p dat0-app
cargo clippy --all-targets -- -D warnings
cargo fmt --check
git add crates/dat0-app/src/actions/view_actions.rs crates/dat0-app/src/view/mod.rs docs/catalog-inspector.md
git commit --signoff -m "feat(inspector): re-project on display-only undo/redo + docs"
```

---

## Manual UAT (owed — joins the standing P4b/P4c/P5b/P5c/P6a/P6b backlog)

Add to `docs/plans/2026-06-08-dat0-inspector-projection-uat.md` (or extend the P6a/P6b UAT doc):

1. Import a CSV; reorder two columns → Inspector cards reorder to match.
2. Rename a column → its card header shows `New · was <orig>`; stats unchanged.
3. Hide a column → it leaves the visible list and appears under "Hidden (1)"; expand to see it dimmed; un-hide → it returns in order.
4. Confirm `__dat0_rowid` never shows as a card.
5. Toggle Whole-table ⇄ Current-view → column set/order/labels stay grid-matched in both.
6. Undo/redo a reorder/rename/hide → the Inspector re-projects immediately (no stale labels, no profile flicker).
7. Open a second table tab, inspect it, switch back → the projection tracks the inspected table.

---

## Self-Review

**Spec coverage:**
- Approach A display-layer (no profiling change) → Tasks 1,3. ✓
- D1 both modes project (guard mode-independent) → Task 3 (`inspector_projection` uses per-tab vm/ds). ✓
- D2 hidden → collapsible "Hidden" section → Tasks 2,3. ✓
- D3 renamed = new + subtle original → Tasks 1 (`original`), 3 (`column_card` header). ✓
- D4 surrogate always omitted → Task 1 (`project_cards`). ✓
- D5 live `column_view` single source of truth → Task 3 (`inspector_projection` clones it). ✓
- D6 no session bump; ephemeral `hidden_expanded` → Task 2. ✓
- Guard (target == active tab, bare-name) → Task 3. ✓
- Reactivity: re-project not re-profile on display-only undo/redo → Task 4. ✓
- i18n keys → Task 3. ✓
- Docs → Task 4. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. `.opacity`/caret glyphs flagged as verify-to-house-style, not blanks.

**Type consistency:** `ProjectionContext { visible: Vec<ProjectionColumn>, base_sources: Vec<String> }`, `RenderCard { source, label, original }`, `ProjectedCards { visible, hidden }`, `project_cards(&[ColumnProfile], Option<&ProjectionContext>) -> ProjectedCards`, `inspector_projection(&self) -> Option<ProjectionContext>`, `render_inspector(&InspectorModel, Option<ProjectionContext>, cx)`, `column_card(&ColumnProfile, &RenderCard, &InspectorModel, bool)`, `hidden_expanded`/`toggle_hidden` — used identically across Tasks 1-4. ✓
