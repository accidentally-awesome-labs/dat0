# Catalog Tree (hierarchy + keyboard nav) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the real catalog hierarchy (one parent node per attached DB, children = its tables, collapse state persisted in session v10) and make the whole catalog panel keyboard-navigable with ARIA-tree-core keys (Tab-to-panel, ↑ ↓ ← →, Enter/Space).

**Architecture:** A pure flatten seam `visible_rows(tree, collapsed)` is the single source of truth for render order AND the nav index; a pure `tree_nav(rows, active, key) → NavAction` drives both keyboard closures; the panel container is ONE `focus_stop` tab stop (recents listbox pattern scaled up). Session gains `catalog_collapsed` (v9→v10, replacing the dead v8 `catalog_expanded`/`catalog_selection` fields).

**Tech Stack:** Rust, gpui 0.2.2 (pinned rev), gpui-component, serde/serde_json, insta (snapshots), existing `a11y-capture` test harness (`focus_stop` + `focused_label` oracle + `A11ySnapshot`).

**Design:** `docs/plans/2026-07-07-dat0-uat-catalog-tree-design.md` (approved). Branch `uat-catalog-tree` off `main` (`8473801`).

## Global Constraints

- **Zero new dependencies.** `Cargo.lock` / `NOTICE` must be unchanged (D-015 stays open). `tempfile`, `serial_test`, insta are already dev-deps.
- **Production code ships unconditionally** (hierarchy render, chevrons, `toggle_catalog_parent`, `catalog_collapsed`, `catalog_active`, `focus_stop`, arrow handler). ONLY test accessors are `#[cfg(feature = "a11y-capture")]`.
- **`cargo fmt --all` before EVERY commit** (plan example code is not fmt-clean; the CI fmt gate is hard).
- **DCO:** every commit `git commit -s` and end the message with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- **Implementers run ONLY the fast focused test** (`cargo test -p dat0-app --test catalog_nav` etc.) synchronously; the CONTROLLER runs `cargo test --workspace --no-fail-fast` + `cargo clippy --workspace --all-targets -- -D warnings` (anti-loop rule).
- **Seam rule:** chain `.a11y`/`.a11y_label` onto EXISTING elements; never wrap a bare `String`/`SharedString` in a new div.
- **`#[cfg(feature = "a11y-capture")]` shims live BEFORE any `#[cfg(test)] mod tests`** in the file (clippy `items-after-test-module` under `-D warnings`).
- New a11y nodes can shift OTHER binaries' exact node-count assertions (`a11y_spike.rs` is the only one; its scene keeps the catalog panel hidden → expected zero drift). The workspace gate is the backstop.
- Keystroke names under gpui `simulate_keystrokes`: `"up"`, `"down"`, `"left"`, `"right"`, `"enter"`, `"space"`, `"tab"`, `"shift-tab"`.

## Interfaces produced by this slice (single reference)

```rust
// crates/dat0-app/src/catalog/nav.rs (NEW — Task 2)
pub enum RowKind {
    Parent { alias: String, expanded: bool, n_children: usize },
    Leaf { name: String, depth: u8 },              // 0 = top-level, 1 = child of a parent
}
pub struct CatalogRow { pub section: &'static str, pub kind: RowKind }
pub fn visible_rows(tree: &CatalogTree, collapsed: &HashSet<String>) -> Vec<CatalogRow>;
pub enum NavAction { Move(usize), Toggle(String), Open(String), None }
pub fn tree_nav(rows: &[CatalogRow], active: usize, key: &str) -> NavAction;

// crates/dat0-app/src/window.rs (Tasks 1/3/4)
pub(crate) catalog_active: usize;                    // ephemeral, persistent shell
pub(crate) catalog_collapsed: HashSet<String>;       // mirrors session v10 field
pub(crate) fn catalog_nav_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>);
pub(crate) fn toggle_catalog_parent(&mut self, alias: String, cx: &mut Context<Self>);
pub fn catalog_active_for_test(&self) -> usize;                     // a11y-capture only
pub fn catalog_collapsed_for_test(&self) -> Vec<String>;            // a11y-capture only, sorted

// crates/dat0-app/src/catalog/panel.rs (Tasks 1/4)
pub fn render_catalog(
    tree: &CatalogTree,
    collapsed: &HashSet<String>,
    active: usize,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement;

// crates/dat0-app/src/session/mod.rs (Task 5)
pub struct SessionUiState {                          // v10: catalog_expanded/catalog_selection REMOVED
    pub catalog_panel_visible: bool,
    pub inspector_panel_visible: bool,
    pub catalog_collapsed: Vec<String>,              // sorted collapsed attach aliases
}
pub const SESSION_SCHEMA_VERSION: u32 = 10;
```

---

### Task 1: T0 spike — HARD GATE: Tab reaches the catalog container, arrows route

Proves R1 (chained `on_key_down` coexists with `focus_stop` on THIS surface) and R6/R3 baseline (motherduck_window green with the new container node) BEFORE the tree work. Ships the real container wiring + a skeleton ↑/↓ handler over the still-flat panel; Tasks 3–4 replace the skeleton's interior, not its shape.

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (field ~2057 area, ctor ~2283, render call site ~6604, shim block ~6712)
- Modify: `crates/dat0-app/src/catalog/panel.rs`
- Create: `crates/dat0-app/tests/catalog_nav.rs`

**Interfaces:**
- Consumes: `focus_stop` (`a11y/mod.rs:41`), `hero_focus_handle(id, cx)` (window.rs:5889), `seed_catalog_tree_for_test` (window.rs:6712), test support `A11ySnapshot`/`press_tab` (`tests/support/mod.rs`).
- Produces: `catalog_active: usize` field, `catalog_active_for_test()`, `render_catalog(tree, active, fh, cx)` (intermediate 4-arg signature; Task 4 adds `collapsed`), skeleton arrows closure, `tests/catalog_nav.rs` scaffolding (`set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`, `focus_shell_neutrally`, `tab_to_catalog`).

- [ ] **Step 1: Add the `catalog_active` field + accessor to `WorkspaceShell`**

In `window.rs`, directly after the `recents_active` field (~line 2057):

```rust
    /// Active-row index for keyboard nav of the Catalog panel (catalog-tree
    /// slice). Held on the persistent shell (the panel render is a free fn,
    /// rebuilt every frame); clamped to the visible-row count at each use.
    /// `pub(crate)`: `catalog::panel` (a sibling module) reaches it from
    /// `cx.listener` closures.
    pub(crate) catalog_active: usize,
```

In the ctor (after `catalog_panel_visible: ui.catalog_panel_visible,` ~line 2282):

```rust
            catalog_active: 0,
```

In the `#[cfg(feature = "a11y-capture")]` shim block (before `seed_catalog_tree_for_test`, window.rs ~6705 — the block already exists and sits before the test module):

```rust
    pub fn catalog_active_for_test(&self) -> usize {
        self.catalog_active
    }
```

- [ ] **Step 2: Wire the container `focus_stop` + skeleton arrows in `render_catalog`**

Replace `catalog/panel.rs`'s `render_catalog` with (keep `section_label` and `catalog_row` as-is for now):

```rust
pub fn render_catalog(
    tree: &crate::catalog::CatalogTree,
    active: usize,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let sections: [(String, &str, &Vec<crate::catalog::CatalogNode>); 4] = [
        ("Sources".to_string(), "Sources", &tree.sources),
        (dat0_i18n::t("catalog.cloud"), "Cloud", &tree.cloud),
        ("Tables".to_string(), "Tables", &tree.tables),
        ("Derived".to_string(), "Derived", &tree.derived),
    ];

    // T0 skeleton: flat row count; Task 4 swaps this for `visible_rows`.
    let row_count =
        tree.sources.len() + tree.cloud.len() + tree.tables.len() + tree.derived.len();
    let _active = active.min(row_count.saturating_sub(1));

    // ↑/↓ move the active-index: a SECOND `on_key_down` chained after
    // `focus_stop`'s own (gpui pushes key-down listeners; both fire —
    // recents R1, re-proven here for THIS surface).
    let arrows = cx.listener(move |ws, ev: &gpui::KeyDownEvent, _window, cx| {
        match ev.keystroke.key.as_str() {
            "down" => {
                ws.catalog_active = (ws.catalog_active + 1).min(row_count.saturating_sub(1))
            }
            "up" => ws.catalog_active = ws.catalog_active.saturating_sub(1),
            _ => return,
        }
        cx.notify();
    });
    // T0 skeleton activate: no-op body (Task 4 routes Enter/Space through
    // `catalog_nav_key`). `focus_stop` still needs a handler to wire.
    let activate = cx.listener(|_ws, _ev: &gpui::KeyDownEvent, _window, _cx| {});

    let mut root = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .focus_stop("catalog-tree", fh, 0, activate)
        .on_key_down(arrows)
        .a11y(
            "catalog-tree",
            crate::a11y::AccessRole::Button,
            dat0_i18n::t("catalog.title"),
        )
        .child(div().child(SharedString::from(dat0_i18n::t("catalog.title"))));

    for (label, id, nodes) in &sections {
        let header = section_label(label, nodes.len());
        let mut section = div().flex().flex_col().gap_1().child(
            div()
                .a11y_label(crate::a11y::AccessRole::Label, header.clone())
                .child(SharedString::from(header)),
        );
        for node in nodes.iter() {
            section = section.child(catalog_row(id, &node.name, cx));
        }
        root = root.child(section);
    }

    root.into_any_element()
}
```

Add the imports the new code needs at the top of `panel.rs`:

```rust
use crate::a11y::FocusStopExt as _;
```

(`A11yExt as _` is already imported; `.a11y` on a `Div` is fine — `Div: InteractiveElement`.)

- [ ] **Step 3: Update the render call site in `window.rs`**

`hero_focus_handle` needs `&mut self`, so hoist the handle BEFORE the element-tree builder borrows `self`. In `WorkspaceShell::render`, above the dock row construction (~line 6598):

```rust
        // Catalog-tree slice: the panel container's stable focus handle (one
        // tab stop for the whole panel). Hoisted here — `hero_focus_handle`
        // needs `&mut self`, unavailable inside the `.children(..)` closures.
        let catalog_fh = self.hero_focus_handle("catalog-tree", cx);
```

Then change the call at ~6608:

```rust
                    .children(self.catalog_panel_visible.then(|| {
                        div()
                            .w_64()
                            .border_r_1()
                            .child(crate::catalog::panel::render_catalog(
                                &self.catalog_tree,
                                self.catalog_active,
                                &catalog_fh,
                                cx,
                            ))
                    }))
```

- [ ] **Step 4: Write the T0 hard-gate test**

Create `crates/dat0-app/tests/catalog_nav.rs`. Scaffolding helpers are COPIED per-binary from `tests/recents_nav.rs` (this crate's per-binary-copy precedent) — copy `set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`, `focus_shell_neutrally` VERBATIM from `tests/recents_nav.rs:43-116` (same imports block, same `BUDGET` const, same `mod support;`). Then add:

```rust
/// A fake `md:`-origin table (alias "sample_data") and a sqlite-attached table
/// (alias "sq") — copied shapes from tests/motherduck_window.rs:104-126.
fn md_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::Attached {
            alias: "sample_data".into(),
            source: "md:sample_data".into(),
        },
    }
}
fn sqlite_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::Attached {
            alias: "sq".into(),
            source: "/tmp/x.db".into(),
        },
    }
}
fn file_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::File(std::path::PathBuf::from("/data/local.csv")),
    }
}

/// Tab from the neutral shell focus until the catalog container is the focused
/// stop, or panic after a bounded number of hops (recents_nav.rs:137 idiom;
/// the hero paints several stops before the dock, hence the larger bound).
fn tab_to_catalog(cx: &mut VisualTestContext) {
    let want = dat0_i18n::t("catalog.title");
    for _ in 0..20 {
        press_tab(cx);
        let snap = A11ySnapshot::capture(cx);
        if snap.focused_label() == Some(want.as_str()) {
            return;
        }
    }
    panic!("catalog container was never the focused Tab stop within 20 hops");
}

/// T0 HARD GATE. Seeds a flat 2-table catalog, Tabs to the container (R6 +
/// oracle twin), presses Down and asserts the active-index moved (R1: the
/// chained on_key_down receives arrows on THIS surface). If the Down assertion
/// fails, STOP — switch to the single-unified-on_key_down fallback (design R1).
#[gpui::test]
#[serial]
fn t0_catalog_tab_and_arrow(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(vec![md_tbl("md_events"), file_tbl("local_sales")]);
        });
    });
    vcx.run_until_parked();

    // The container's oracle twin renders (label = the panel title).
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label(&dat0_i18n::t("catalog.title")),
        "catalog container a11y twin must render when the panel is visible"
    );

    // R6: Tab reaches the container; the oracle names it by its label text.
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    // R1: Down moves the active-index via the chained on_key_down.
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        0,
        "active-index starts at 0"
    );
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        1,
        "Down must move the catalog active-index to 1 (R1 hard gate)"
    );

    drop(state);
}
```

- [ ] **Step 5: Run the gate**

```bash
cargo test -p dat0-app --test catalog_nav
```
Expected: `t0_catalog_tab_and_arrow ... ok`. **If the Down assertion fails: STOP. Do not proceed — report back; the fallback (single unified `on_key_down` handling up/down/left/right/enter/space with a no-op `focus_stop` activate) changes Tasks 4's closure layout and must be re-planned.**

```bash
cargo test -p dat0-app --test motherduck_window
```
Expected: all 5 pass (R3 baseline — the new container node must not disturb the Slice-5 teeth).

- [ ] **Step 6: fmt + commit**

```bash
cargo fmt --all
git add -A
git commit -s -m "feat(a11y): catalog panel container focus_stop + active-index skeleton (T0 hard gate)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Pure nav module — `visible_rows` + `tree_nav`

Pure module; nothing else references it yet (Task 4 wires it). Green by construction.

**Files:**
- Create: `crates/dat0-app/src/catalog/nav.rs`
- Modify: `crates/dat0-app/src/catalog/mod.rs` (register the module)

**Interfaces:**
- Consumes: `CatalogTree`/`CatalogNode` (`catalog/tree.rs`).
- Produces: `CatalogRow`, `RowKind`, `visible_rows`, `NavAction`, `tree_nav` (exact shapes in the plan-top Interfaces block).

- [ ] **Step 1: Write the failing unit tests + implementation**

Create `crates/dat0-app/src/catalog/nav.rs`:

```rust
//! Pure keyboard-nav model for the Catalog panel (catalog-tree slice).
//!
//! `visible_rows` flattens the [`CatalogTree`] + a collapsed-alias set into the
//! exact row list the panel paints, in paint order — the SINGLE source of truth
//! for both the render iteration and the keyboard active-index (ring, arrows,
//! Enter and painted rows derive from one Vec, so they cannot drift). `tree_nav`
//! maps (rows, active, key) to a [`NavAction`] — the pure-fn → enum → thin-match
//! idiom (`resolve_relaunch_action` precedent), unit-testable without GPUI.

use std::collections::HashSet;

use crate::catalog::{CatalogNode, CatalogTree};

/// One visible catalog row. `section` is the panel's stable section id
/// ("Sources" / "Cloud" / "Tables" / "Derived").
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogRow {
    pub section: &'static str,
    pub kind: RowKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    /// An attached-DB parent node (name = attach alias).
    Parent {
        alias: String,
        expanded: bool,
        n_children: usize,
    },
    /// A table row. `depth` 0 = top-level leaf, 1 = child of the preceding
    /// Parent (children always immediately follow their parent, so `tree_nav`
    /// finds the parent by scanning backward for the nearest Parent row).
    Leaf { name: String, depth: u8 },
}

/// Every VISIBLE catalog row in paint order. Children of collapsed parents are
/// absent. Section headers are NOT rows (non-interactive; nav skips them).
pub fn visible_rows(tree: &CatalogTree, collapsed: &HashSet<String>) -> Vec<CatalogRow> {
    let sections: [(&'static str, &Vec<CatalogNode>); 4] = [
        ("Sources", &tree.sources),
        ("Cloud", &tree.cloud),
        ("Tables", &tree.tables),
        ("Derived", &tree.derived),
    ];
    let mut rows = Vec::new();
    for (section, nodes) in sections {
        for node in nodes {
            if node.children.is_empty() {
                rows.push(CatalogRow {
                    section,
                    kind: RowKind::Leaf {
                        name: node.name.clone(),
                        depth: 0,
                    },
                });
            } else {
                let expanded = !collapsed.contains(&node.name);
                rows.push(CatalogRow {
                    section,
                    kind: RowKind::Parent {
                        alias: node.name.clone(),
                        expanded,
                        n_children: node.children.len(),
                    },
                });
                if expanded {
                    for c in &node.children {
                        rows.push(CatalogRow {
                            section,
                            kind: RowKind::Leaf {
                                name: c.clone(),
                                depth: 1,
                            },
                        });
                    }
                }
            }
        }
    }
    rows
}

/// What a key press does at (rows, active). ARIA-tree-core transition table —
/// see the design doc for the full matrix.
#[derive(Debug, Clone, PartialEq)]
pub enum NavAction {
    Move(usize),
    /// Flip this attach alias in the collapsed set.
    Toggle(String),
    /// Open this table into the main grid.
    Open(String),
    None,
}

pub fn tree_nav(rows: &[CatalogRow], active: usize, key: &str) -> NavAction {
    let Some(row) = rows.get(active) else {
        return NavAction::None;
    };
    match key {
        "down" => {
            if active + 1 < rows.len() {
                NavAction::Move(active + 1)
            } else {
                NavAction::None
            }
        }
        "up" => {
            if active > 0 {
                NavAction::Move(active - 1)
            } else {
                NavAction::None
            }
        }
        "right" => match &row.kind {
            RowKind::Parent {
                alias,
                expanded: false,
                ..
            } => NavAction::Toggle(alias.clone()),
            // Expanded parent: its first child is by construction the next row.
            RowKind::Parent { expanded: true, .. } => NavAction::Move(active + 1),
            RowKind::Leaf { .. } => NavAction::None,
        },
        "left" => match &row.kind {
            RowKind::Parent {
                alias,
                expanded: true,
                ..
            } => NavAction::Toggle(alias.clone()),
            RowKind::Leaf { depth: 1, .. } => {
                // Children always directly follow their parent, so the nearest
                // Parent row above is THE parent.
                match rows[..active]
                    .iter()
                    .rposition(|r| matches!(r.kind, RowKind::Parent { .. }))
                {
                    Some(p) => NavAction::Move(p),
                    None => NavAction::None,
                }
            }
            _ => NavAction::None,
        },
        "enter" | "space" => match &row.kind {
            RowKind::Parent { alias, .. } => NavAction::Toggle(alias.clone()),
            RowKind::Leaf { name, .. } => NavAction::Open(name.clone()),
        },
        _ => NavAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sources: [parent "sq" → (a, b)], cloud: [parent "md" → (c)],
    /// tables: [leaf "t"].
    fn tree() -> CatalogTree {
        CatalogTree {
            sources: vec![CatalogNode {
                name: "sq".into(),
                schema: String::new(),
                children: vec!["a".into(), "b".into()],
            }],
            cloud: vec![CatalogNode {
                name: "md".into(),
                schema: String::new(),
                children: vec!["c".into()],
            }],
            tables: vec![CatalogNode {
                name: "t".into(),
                schema: "main".into(),
                children: vec![],
            }],
            derived: vec![],
        }
    }

    fn names(rows: &[CatalogRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match &r.kind {
                RowKind::Parent { alias, .. } => format!("P:{alias}"),
                RowKind::Leaf { name, depth } => format!("L{depth}:{name}"),
            })
            .collect()
    }

    #[test]
    fn expanded_tree_flattens_in_paint_order() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(
            names(&rows),
            vec!["P:sq", "L1:a", "L1:b", "P:md", "L1:c", "L0:t"]
        );
    }

    #[test]
    fn collapse_hides_exactly_that_parents_children() {
        let collapsed: HashSet<String> = ["sq".to_string()].into();
        let rows = visible_rows(&tree(), &collapsed);
        assert_eq!(names(&rows), vec!["P:sq", "P:md", "L1:c", "L0:t"]);
        assert!(matches!(
            rows[0].kind,
            RowKind::Parent { expanded: false, .. }
        ));
    }

    #[test]
    fn up_down_move_and_clamp() {
        let rows = visible_rows(&tree(), &HashSet::new()); // 6 rows
        assert_eq!(tree_nav(&rows, 0, "down"), NavAction::Move(1));
        assert_eq!(tree_nav(&rows, 5, "down"), NavAction::None);
        assert_eq!(tree_nav(&rows, 1, "up"), NavAction::Move(0));
        assert_eq!(tree_nav(&rows, 0, "up"), NavAction::None);
    }

    #[test]
    fn right_expands_or_steps_into_first_child() {
        let rows = visible_rows(&tree(), &HashSet::new());
        // expanded parent → first child (the next row)
        assert_eq!(tree_nav(&rows, 0, "right"), NavAction::Move(1));
        // leaf → None
        assert_eq!(tree_nav(&rows, 5, "right"), NavAction::None);
        // collapsed parent → expand
        let collapsed: HashSet<String> = ["sq".to_string()].into();
        let rows = visible_rows(&tree(), &collapsed);
        assert_eq!(tree_nav(&rows, 0, "right"), NavAction::Toggle("sq".into()));
    }

    #[test]
    fn left_collapses_or_jumps_to_parent() {
        let rows = visible_rows(&tree(), &HashSet::new());
        // expanded parent → collapse
        assert_eq!(tree_nav(&rows, 0, "left"), NavAction::Toggle("sq".into()));
        // child (row 2 = L1:b) → its parent (row 0)
        assert_eq!(tree_nav(&rows, 2, "left"), NavAction::Move(0));
        // child of the SECOND parent (row 4 = L1:c) → row 3, not row 0
        assert_eq!(tree_nav(&rows, 4, "left"), NavAction::Move(3));
        // top-level leaf → None
        assert_eq!(tree_nav(&rows, 5, "left"), NavAction::None);
    }

    #[test]
    fn enter_toggles_parents_and_opens_leaves() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(tree_nav(&rows, 0, "enter"), NavAction::Toggle("sq".into()));
        assert_eq!(tree_nav(&rows, 1, "enter"), NavAction::Open("a".into()));
        assert_eq!(tree_nav(&rows, 5, "space"), NavAction::Open("t".into()));
    }

    #[test]
    fn out_of_range_and_unknown_keys_are_none() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(tree_nav(&rows, 99, "down"), NavAction::None);
        assert_eq!(tree_nav(&[], 0, "down"), NavAction::None);
        assert_eq!(tree_nav(&rows, 0, "escape"), NavAction::None);
    }
}
```

Register in `crates/dat0-app/src/catalog/mod.rs` — the whole file becomes:

```rust
pub mod nav;
pub mod panel;
pub mod tree;
pub use tree::{CatalogNode, CatalogTree};
```

- [ ] **Step 2: Run the module tests**

```bash
cargo test -p dat0-app --lib catalog::nav
```
Expected: 7 passed.

- [ ] **Step 3: fmt + commit**

```bash
cargo fmt --all
git add -A
git commit -s -m "feat(catalog): pure visible_rows flatten + tree_nav transition model

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Grouping in `CatalogTree::build` + parent-aware `filter`

Pure model change. **This does NOT change the panel yet** — but it DOES change what the flat panel renders (cloud/sources sections now hold parent nodes whose children the old render ignores). Task 4 lands the hierarchy render; to keep THIS task's tree green, Task 3 also updates `tree.rs`'s own unit tests and verifies `motherduck_window` is NOT run between Tasks 3 and 4 — **Tasks 3 and 4 are one review unit and MUST land as consecutive commits with the focused motherduck gate run at the END of Task 4** (the intermediate commit is allowed to have a red `motherduck_window`; the workspace gate runs post-Task-4).

**Files:**
- Modify: `crates/dat0-app/src/catalog/tree.rs`

**Interfaces:**
- Consumes: `TableOrigin::Attached { alias, source }` (dat0-engine).
- Produces: `CatalogTree::build` with populated `children` on attach parents; `filter` with parent semantics. Node shapes unchanged (`CatalogNode` untouched).

- [ ] **Step 1: Update the `tree.rs` unit tests to the grouped shape (failing first)**

Replace the bodies of the three affected tests (keep `token_and_search_filters` as-is — Derived leaves are unaffected):

```rust
    #[test]
    fn groups_by_origin() {
        let tables = vec![
            t("sales", TableOrigin::File(PathBuf::from("/s.csv"))),
            t(
                "orders",
                TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
            ),
            t(
                "orders_open",
                TableOrigin::Derived(DerivedOrigin::Transform {
                    parent: "orders".into(),
                    ops: vec![],
                }),
            ),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "md".into(),
                    source: "md:".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables);
        // File stays a flat Sources leaf; the md: attach becomes a Cloud PARENT
        // named by its alias, with the table as a child.
        assert_eq!(tree.sources.len(), 1, "only the file source in Sources");
        assert!(tree.sources.iter().any(|n| n.name == "sales"));
        assert!(tree.tables.iter().any(|n| n.name == "orders"));
        assert!(tree.derived.iter().any(|n| n.name == "orders_open"));
        assert_eq!(tree.cloud.len(), 1);
        assert_eq!(tree.cloud[0].name, "md", "cloud node is the attach ALIAS");
        assert_eq!(tree.cloud[0].children, vec!["md_events".to_string()]);
    }

    #[test]
    fn motherduck_attaches_group_under_cloud_sqlite_stays_sources() {
        let tables = vec![
            t("sales", TableOrigin::File(PathBuf::from("/s.csv"))),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "sample_data".into(),
                    source: "md:".into(),
                },
            ),
            t(
                "local_sqlite",
                TableOrigin::Attached {
                    alias: "sq".into(),
                    source: "/tmp/x.db".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables);
        assert_eq!(tree.cloud.len(), 1, "only the md: attach is Cloud");
        assert_eq!(tree.cloud[0].name, "sample_data");
        assert_eq!(tree.cloud[0].children, vec!["md_events".to_string()]);
        // Sources holds the file leaf + the sqlite attach PARENT (sorted by name).
        assert_eq!(tree.sources.len(), 2);
        assert!(
            tree.sources
                .iter()
                .any(|n| n.name == "sales" && n.children.is_empty())
        );
        assert!(
            tree.sources
                .iter()
                .any(|n| n.name == "sq" && n.children == vec!["local_sqlite".to_string()])
        );
    }

    #[test]
    fn cloud_group_respects_token_and_search() {
        let tables = vec![
            t(
                "md_orders",
                TableOrigin::Attached {
                    alias: "db".into(),
                    source: "md:".into(),
                },
            ),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "db".into(),
                    source: "md:".into(),
                },
            ),
        ];
        // Child-match: the parent survives with ONLY the matching child.
        let tree = CatalogTree::build(&tables).filter("ord");
        assert_eq!(tree.cloud.len(), 1);
        assert_eq!(tree.cloud[0].name, "db");
        assert_eq!(tree.cloud[0].children, vec!["md_orders".to_string()]);
    }
```

Add three NEW tests after them:

```rust
    #[test]
    fn same_alias_tables_group_under_one_parent_sorted() {
        let tables = vec![
            t(
                "zeta",
                TableOrigin::Attached {
                    alias: "sq".into(),
                    source: "/tmp/x.db".into(),
                },
            ),
            t(
                "alpha",
                TableOrigin::Attached {
                    alias: "sq".into(),
                    source: "/tmp/x.db".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables);
        assert_eq!(tree.sources.len(), 1, "one parent per alias");
        assert_eq!(
            tree.sources[0].children,
            vec!["alpha".to_string(), "zeta".to_string()],
            "children sorted by name"
        );
    }

    #[test]
    fn filter_alias_match_keeps_all_children() {
        let tables = vec![
            t(
                "md_orders",
                TableOrigin::Attached {
                    alias: "warehouse".into(),
                    source: "md:".into(),
                },
            ),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "warehouse".into(),
                    source: "md:".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables).filter("ware");
        assert_eq!(tree.cloud.len(), 1);
        assert_eq!(tree.cloud[0].children.len(), 2, "alias match keeps ALL children");
    }

    #[test]
    fn filter_no_match_drops_the_parent() {
        let tables = vec![t(
            "md_orders",
            TableOrigin::Attached {
                alias: "db".into(),
                source: "md:".into(),
            },
        )];
        let tree = CatalogTree::build(&tables).filter("zzz");
        assert!(tree.cloud.is_empty());
    }
```

- [ ] **Step 2: Run to verify the updated tests fail**

```bash
cargo test -p dat0-app --lib catalog::tree
```
Expected: FAIL (`groups_by_origin` etc. — build still emits flat leaves).

- [ ] **Step 3: Implement grouping + parent-aware filter**

Replace `build` and `filter` in `tree.rs`:

```rust
impl CatalogTree {
    pub fn build(tables: &[TableInfo]) -> Self {
        let mut tree = CatalogTree::default();
        // Attached tables group by alias into one PARENT node per attach
        // (catalog-tree slice): parent.name = alias, children = table names.
        // (alias, source, children), first-seen order; sorted below.
        let mut attaches: Vec<(String, String, Vec<String>)> = Vec::new();
        for ti in tables {
            let leaf = CatalogNode {
                name: ti.name.clone(),
                schema: ti.schema.clone(),
                children: vec![],
            };
            match &ti.origin {
                TableOrigin::File(_) => tree.sources.push(leaf),
                TableOrigin::Attached { alias, source } => {
                    match attaches.iter_mut().find(|(a, _, _)| a == alias) {
                        Some((_, _, kids)) => kids.push(ti.name.clone()),
                        None => {
                            attaches.push((alias.clone(), source.clone(), vec![ti.name.clone()]))
                        }
                    }
                }
                TableOrigin::Derived(d) => match d {
                    dat0_engine::DerivedOrigin::Transform { .. } => tree.derived.push(leaf),
                    dat0_engine::DerivedOrigin::Sql(s) if !s.is_empty() => tree.derived.push(leaf),
                    _ => tree.tables.push(leaf),
                },
            }
        }
        for (alias, source, mut kids) in attaches {
            kids.sort();
            let parent = CatalogNode {
                name: alias,
                schema: String::new(),
                children: kids,
            };
            // MotherDuck attaches record `source = "md:…"` (duckdb_engine.rs:721-730);
            // Cloud ⇔ md: prefix, applied to the PARENT (rule unchanged from flat).
            if source.starts_with("md:") {
                tree.cloud.push(parent);
            } else {
                tree.sources.push(parent);
            }
        }
        // Deterministic paint/nav order: every section sorted by node name
        // (parents sort by alias among the leaves).
        for sec in [
            &mut tree.sources,
            &mut tree.cloud,
            &mut tree.tables,
            &mut tree.derived,
        ] {
            sec.sort_by(|a, b| a.name.cmp(&b.name));
        }
        tree
    }

    /// Token-AND filter over node names (case-insensitive). Leaves survive if
    /// every whitespace token is a substring of their lowercased name. Parents
    /// (attach nodes with children): an ALIAS match keeps the parent with ALL
    /// children; otherwise the children are filtered and the parent survives
    /// iff any child matched.
    pub fn filter(mut self, query: &str) -> Self {
        let toks: Vec<String> = query.split_whitespace().map(|s| s.to_lowercase()).collect();
        if toks.is_empty() {
            return self;
        }
        let matches = |name: &str| {
            let lc = name.to_lowercase();
            toks.iter().all(|t| lc.contains(t.as_str()))
        };
        let keep = |n: &mut CatalogNode| {
            if n.children.is_empty() {
                return matches(&n.name);
            }
            if matches(&n.name) {
                return true; // alias match keeps all children
            }
            n.children.retain(|c| matches(c));
            !n.children.is_empty()
        };
        self.sources.retain_mut(keep);
        self.tables.retain_mut(keep);
        self.derived.retain_mut(keep);
        self.cloud.retain_mut(keep);
        self
    }
}
```

(Note `retain_mut` — the closure must be `FnMut(&mut CatalogNode) -> bool`; if the borrow checker objects to the shared `keep` closure across four calls, inline a small `fn keep(n: &mut CatalogNode, toks: &[String]) -> bool` helper instead.)

- [ ] **Step 4: Run the tree tests**

```bash
cargo test -p dat0-app --lib catalog::
```
Expected: tree tests + nav tests all pass. Do NOT run `motherduck_window` here — it is expected red until Task 4's render lands (children invisible under the old flat render).

- [ ] **Step 5: fmt + commit**

```bash
cargo fmt --all
git add -A
git commit -s -m "feat(catalog): group attached tables under per-alias parent nodes

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Hierarchy render + toggle + full tree-nav wiring

The render half of the Task-3/4 unit: panel paints parents/children from `visible_rows`, mouse toggle, active ring, and the skeleton arrows are replaced by the full `tree_nav` match. `motherduck_window` MUST be green at the end of this task.

**Files:**
- Modify: `crates/dat0-app/src/catalog/panel.rs`
- Modify: `crates/dat0-app/src/window.rs` (new field + 2 methods + render call site + accessor)

**Interfaces:**
- Consumes: `visible_rows`/`tree_nav`/`NavAction`/`RowKind` (Task 2), grouped `build` (Task 3), `open_table_tab(name, window, cx)` (existing).
- Produces: `catalog_collapsed: HashSet<String>` (ephemeral until Task 5), `catalog_nav_key`, `toggle_catalog_parent`, `catalog_collapsed_for_test()`, final `render_catalog(tree, collapsed, active, fh, cx)` 5-arg signature.

- [ ] **Step 1: Add shell state + methods in `window.rs`**

Field after `catalog_active` (Task 1's):

```rust
    /// Collapsed attach-parent aliases in the Catalog panel (catalog-tree
    /// slice). Empty = all expanded. Mirrored to session v10
    /// `SessionUiState.catalog_collapsed` (Task 5 wires persistence).
    pub(crate) catalog_collapsed: std::collections::HashSet<String>,
```

Ctor (after `catalog_active: 0,`):

```rust
            catalog_collapsed: std::collections::HashSet::new(),
```

Methods — place near `set_inspector_target` (~window.rs:3035), in the same impl block:

```rust
    /// Apply one keyboard-nav key to the Catalog panel: flatten the current
    /// tree, clamp the active index (SINGLE clamp site — ring, arrows and
    /// Enter all use this same index), then act on the pure `tree_nav`
    /// transition. Both the container's `focus_stop` activate (enter/space)
    /// and the chained arrow handler route here (single source of truth).
    pub(crate) fn catalog_nav_key(
        &mut self,
        key: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let rows = crate::catalog::nav::visible_rows(&self.catalog_tree, &self.catalog_collapsed);
        if rows.is_empty() {
            return;
        }
        let active = self.catalog_active.min(rows.len() - 1);
        self.catalog_active = active;
        match crate::catalog::nav::tree_nav(&rows, active, key) {
            crate::catalog::nav::NavAction::Move(i) => {
                self.catalog_active = i;
                cx.notify();
            }
            crate::catalog::nav::NavAction::Toggle(alias) => {
                self.toggle_catalog_parent(alias, cx);
            }
            crate::catalog::nav::NavAction::Open(name) => {
                self.open_table_tab(name, window, cx);
            }
            crate::catalog::nav::NavAction::None => {}
        }
    }

    /// Flip an attach parent's expand/collapse state. Single source of truth —
    /// the parent row's mouse `on_click` AND the keyboard Toggle arm both call
    /// this (mouse and keyboard cannot drift). Clamps the active index against
    /// the post-toggle row count so a collapse can never dangle the ring.
    pub(crate) fn toggle_catalog_parent(&mut self, alias: String, cx: &mut gpui::Context<Self>) {
        if !self.catalog_collapsed.remove(&alias) {
            self.catalog_collapsed.insert(alias);
        }
        let rows = crate::catalog::nav::visible_rows(&self.catalog_tree, &self.catalog_collapsed);
        self.catalog_active = self.catalog_active.min(rows.len().saturating_sub(1));
        cx.notify();
    }
```

Accessor in the `a11y-capture` shim block (after `catalog_active_for_test`):

```rust
    pub fn catalog_collapsed_for_test(&self) -> Vec<String> {
        let mut v: Vec<String> = self.catalog_collapsed.iter().cloned().collect();
        v.sort();
        v
    }
```

- [ ] **Step 2: Rewrite `render_catalog` to paint from `visible_rows`**

Replace the whole of `panel.rs`'s render section (keep `section_label` + its tests):

```rust
use crate::a11y::A11yExt as _;
use crate::a11y::FocusStopExt as _;
use crate::catalog::nav::{visible_rows, RowKind};
use crate::window::WorkspaceShell;
use gpui::prelude::*;
use gpui::{Context, SharedString, div};
use std::collections::HashSet;

/// Render the catalog dock from the current tree + collapse/nav state. Called
/// from `WorkspaceShell::render`. The row list comes from the pure
/// [`visible_rows`] flatten — the SAME Vec the keyboard handlers use, so the
/// paint order and the nav index cannot drift.
pub fn render_catalog(
    tree: &crate::catalog::CatalogTree,
    collapsed: &HashSet<String>,
    active: usize,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let rows = visible_rows(tree, collapsed);
    let active = active.min(rows.len().saturating_sub(1));

    // ↑/↓/←/→: a SECOND on_key_down chained after focus_stop's own (gpui
    // pushes key-down listeners; both fire — T0-proven on this surface).
    let arrows = cx.listener(|ws, ev: &gpui::KeyDownEvent, window, cx| {
        if matches!(
            ev.keystroke.key.as_str(),
            "up" | "down" | "left" | "right"
        ) {
            let key = ev.keystroke.key.clone();
            ws.catalog_nav_key(&key, window, cx);
        }
    });
    // Enter/Space (focus_stop routes only those two here).
    let activate = cx.listener(|ws, ev: &gpui::KeyDownEvent, window, cx| {
        let key = ev.keystroke.key.clone();
        ws.catalog_nav_key(&key, window, cx);
    });

    let mut root = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .focus_stop("catalog-tree", fh, 0, activate)
        .on_key_down(arrows)
        .a11y(
            "catalog-tree",
            crate::a11y::AccessRole::Button,
            dat0_i18n::t("catalog.title"),
        )
        .child(div().child(SharedString::from(dat0_i18n::t("catalog.title"))));

    // Header `(n)` = TOP-LEVEL node count (an attach = 1 parent), collapse-
    // independent — keeps the Slice-5 "Cloud (1)" teeth semantics.
    let sections: [(String, &'static str, usize); 4] = [
        ("Sources".to_string(), "Sources", tree.sources.len()),
        (dat0_i18n::t("catalog.cloud"), "Cloud", tree.cloud.len()),
        ("Tables".to_string(), "Tables", tree.tables.len()),
        ("Derived".to_string(), "Derived", tree.derived.len()),
    ];

    let mut iter = rows.iter().enumerate().peekable();
    for (label, id, n) in sections {
        let header = section_label(&label, n);
        let mut section = div().flex().flex_col().gap_1().child(
            div()
                .a11y_label(crate::a11y::AccessRole::Label, header.clone())
                .child(SharedString::from(header)),
        );
        while iter.peek().is_some_and(|(_, r)| r.section == id) {
            let (i, row) = iter.next().expect("peeked");
            section = section.child(match &row.kind {
                RowKind::Parent {
                    alias,
                    expanded,
                    n_children,
                } => parent_row(id, alias, *expanded, *n_children, i == active, cx),
                RowKind::Leaf { name, depth } => {
                    catalog_row(id, name, *depth, i == active, cx)
                }
            });
        }
        root = root.child(section);
    }

    root.into_any_element()
}

/// An attach-parent row: chevron + alias + child count. Click toggles
/// expand/collapse via `toggle_catalog_parent` — the SAME method the keyboard
/// Toggle arm calls (single source of truth).
fn parent_row(
    section: &str,
    alias: &str,
    expanded: bool,
    n_children: usize,
    is_active: bool,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let chev = if expanded { "▾" } else { "▸" };
    let text = format!("{chev} {alias} ({n_children})");
    let alias_owned = alias.to_string();
    // `attach-` infix: a parent id can never collide with a same-named table
    // row id within the section.
    let mut row = div()
        .id(SharedString::from(format!("cat-{section}-attach-{alias}")))
        .px_2()
        .py_1()
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgba(0x80808022)))
        .child(SharedString::from(text.clone()))
        .a11y_label(crate::a11y::AccessRole::Label, text)
        .on_click(cx.listener(move |ws, _ev, _window, cx| {
            ws.toggle_catalog_parent(alias_owned.clone(), cx);
        }));
    if is_active {
        row = row
            .border_2()
            .border_color(gpui::rgb(crate::a11y::FOCUS_RING));
    }
    row
}

/// A clickable table row that opens `name` into the main grid. `depth == 1`
/// (child of an attach parent) indents. Active row paints the nav ring
/// (decoupled from gpui focus — grid `is_active` idiom).
fn catalog_row(
    section: &str,
    name: &str,
    depth: u8,
    is_active: bool,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let name = name.to_string();
    let mut row = div()
        .id(SharedString::from(format!("cat-{section}-{name}")))
        .px_2()
        .py_1()
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgba(0x80808022)))
        .child(SharedString::from(name.clone()))
        .a11y_label(crate::a11y::AccessRole::Label, name.clone())
        .on_click(cx.listener(move |ws, _ev, window, cx| {
            ws.open_table_tab(name.clone(), window, cx);
        }));
    if depth == 1 {
        row = row.pl_4();
    }
    if is_active {
        row = row
            .border_2()
            .border_color(gpui::rgb(crate::a11y::FOCUS_RING));
    }
    row
}
```

- [ ] **Step 3: Update the render call site in `window.rs` to the 5-arg signature**

```rust
                            .child(crate::catalog::panel::render_catalog(
                                &self.catalog_tree,
                                &self.catalog_collapsed,
                                self.catalog_active,
                                &catalog_fh,
                                cx,
                            ))
```

- [ ] **Step 4: Run the R3 gate — motherduck teeth + T0 test survive the hierarchy**

```bash
cargo test -p dat0-app --test motherduck_window
cargo test -p dat0-app --test catalog_nav
cargo test -p dat0-app --lib catalog::
```
Expected: ALL pass. `cloud_group_renders_md_table_not_file` in particular: `Cloud (1)` (1 parent) + `md_events` (child renders expanded-by-default) + `Sources (1)` (file leaf). If a teeth fails, fix render semantics — never edit the Slice-5 assertion without flagging it in the task report.

- [ ] **Step 5: fmt + commit**

```bash
cargo fmt --all
git add -A
git commit -s -m "feat(catalog): hierarchy render + expand/collapse + full tree keyboard nav

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Session v10 — persist `catalog_collapsed`

**Files:**
- Modify: `crates/dat0-app/src/session/mod.rs` (`SessionUiState`, `SESSION_SCHEMA_VERSION`, 2 unit tests)
- Modify: `crates/dat0-app/src/session/migrate.rs` (arm 9→10, new current arm 10, 1 new unit test)
- Modify: `crates/dat0-app/src/window.rs` (ctor restore, `persist_dock_ui`, `toggle_catalog_parent` persists)
- Modify: `crates/dat0-app/tests/session_migration.rs` (2 tests + snapshot fixture)
- Modify: `crates/dat0-app/tests/snapshots/session_migration__session_json_wire_format.snap` (regenerate)

**Interfaces:**
- Consumes: `toggle_catalog_parent` (Task 4), `persist_dock_ui` (window.rs:3935), `Session::set_ui`/`ui()`.
- Produces: v10 `SessionUiState` (shape in plan-top Interfaces block), `SESSION_SCHEMA_VERSION = 10`, `migrate_v9_to_v10`.

- [ ] **Step 1: Change the schema (mod.rs)**

`SessionUiState` (mod.rs:99) — replace the two dead fields:

```rust
/// Persisted catalog/inspector UI state (v8+, P6a; reshaped v10, catalog-tree
/// slice). Additive: a v7 file lacks the enclosing `ui` field, so the whole
/// struct serde-defaults to both-docks-hidden / all-expanded.
///
/// v10 REPLACED the never-read v8 forward-looking fields (`catalog_expanded`,
/// `catalog_selection`) with `catalog_collapsed`: the panel defaults to
/// expanded, so the persisted set is the COLLAPSED attach aliases (empty =
/// all expanded; absent-in-file = all expanded). Old keys in v8/v9 files are
/// silently dropped by serde on load (prod only ever wrote them empty).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionUiState {
    #[serde(default)]
    pub catalog_panel_visible: bool,
    #[serde(default)]
    pub inspector_panel_visible: bool,
    /// Collapsed attach-parent aliases, sorted (deterministic wire format).
    #[serde(default)]
    pub catalog_collapsed: Vec<String>,
}
```

Bump the constant (mod.rs:59):

```rust
pub const SESSION_SCHEMA_VERSION: u32 = 10;
```

Update the two mod.rs unit tests:
- `v7_file_loads_as_v8_with_default_ui` (line ~856): replace `assert!(state.ui.catalog_expanded.is_empty());` with `assert!(state.ui.catalog_collapsed.is_empty());`
- `ui_round_trips_through_persist_and_recover` (line ~1010): replace the `set_ui` struct literal + trailing asserts:

```rust
            sess.set_ui(SessionUiState {
                catalog_panel_visible: true,
                inspector_panel_visible: true,
                catalog_collapsed: vec!["sq".into(), "warehouse".into()],
            })
            .expect("set_ui");
```

and

```rust
        assert_eq!(
            ui.catalog_collapsed,
            vec!["sq".to_string(), "warehouse".to_string()]
        );
```

(delete the `catalog_selection` assert; update the test's doc comment to name `catalog_collapsed`).

- [ ] **Step 2: Add the migration arm (migrate.rs)**

In `load_str`'s match: change the `9` arm to a plain migration and move the forward-incompat transform scan into a new `10` arm:

```rust
        8 => migrate_v8_to_v9(raw),
        9 => migrate_v9_to_v10(raw),
        10 => {
            // Forward-incompat guard (see the long comment that lived on the v9
            // arm — semantics unchanged, now guarding the current version 10).
            let doc: serde_json::Value = serde_json::from_str(raw)?;
            if let Some(kind) = find_unknown_transform_kind(&doc) {
                return Err(SessionLoadError::ForwardIncompatTransform(kind));
            }
            let state: SessionState = serde_json::from_str(raw)?;
            Ok(state)
        }
```

(Keep the existing explanatory comment block on the current-version arm — move it wholesale from the 9 arm to the 10 arm.)

New helper after `migrate_v8_to_v9`:

```rust
/// Migrate a raw v9 JSON string to a v10 `SessionState`.
///
/// v10 reshapes `ui`: the never-read forward-looking `catalog_expanded` /
/// `catalog_selection` (v8) are REPLACED by `catalog_collapsed`
/// (serde-defaulted empty = all expanded). Serde silently drops the old keys
/// on parse — prod only ever wrote them at their empty defaults, so no data
/// is migrated. Re-parse + stamp the version.
fn migrate_v9_to_v10(raw: &str) -> Result<SessionState, SessionLoadError> {
    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    Ok(state)
}
```

New unit test in migrate.rs's test module:

```rust
    #[test]
    fn v9_session_migrates_to_v10_dropping_dead_ui_fields() {
        // A v9 document carrying the dead v8 forward-looking ui keys.
        let v9 = r#"{"schema_version":9,"tabs":[],
            "ui":{"catalog_panel_visible":true,
                  "catalog_expanded":["orders"],"catalog_selection":"orders"}}"#;
        let state = super::load_str(v9).expect("v9 migrates");
        assert_eq!(state.schema_version, super::SESSION_SCHEMA_VERSION);
        assert!(state.ui.catalog_panel_visible, "known ui keys survive");
        assert!(
            state.ui.catalog_collapsed.is_empty(),
            "new field defaults empty; dead keys dropped"
        );
    }
```

- [ ] **Step 3: Wire the shell (window.rs)**

Ctor: replace `catalog_collapsed: std::collections::HashSet::new(),` with:

```rust
            catalog_collapsed: ui.catalog_collapsed.iter().cloned().collect(),
```

`persist_dock_ui` (window.rs:3935) — replace the body (and its stale doc-comment sentence about `catalog_expanded`):

```rust
    /// Persist the catalog/inspector dock UI state to `session.json` (P6a T13;
    /// v10 adds the catalog collapse set). Sorted for a deterministic wire
    /// format (the insta snapshot gates it).
    pub(crate) fn persist_dock_ui(&self) {
        let mut catalog_collapsed: Vec<String> =
            self.catalog_collapsed.iter().cloned().collect();
        catalog_collapsed.sort();
        let ui = crate::session::SessionUiState {
            catalog_panel_visible: self.catalog_panel_visible,
            inspector_panel_visible: self.inspector_panel_visible,
            catalog_collapsed,
        };
        if let Err(e) = self.session.lock().set_ui(ui) {
            tracing::warn!(error = %e, "persist_dock_ui: set_ui failed");
        }
    }
```

`toggle_catalog_parent`: add `self.persist_dock_ui();` immediately before `cx.notify();`.

- [ ] **Step 4: Update `tests/session_migration.rs`**

- `v7_file_migrates_to_v8_with_default_ui` (~line 500): change `assert_eq!(state.schema_version, 9);` → `assert_eq!(state.schema_version, 10);` and the two dead-field asserts → `assert!(state.ui.catalog_collapsed.is_empty());`
- `v8_roundtrips_ui` (~line 521): keep the raw v8 document EXACTLY as-is (it still proves old files load); update the tail asserts:

```rust
    let state = migrate::load_str(raw).expect("v8 loads");
    assert_eq!(state.schema_version, SESSION_SCHEMA_VERSION);
    assert!(state.ui.catalog_panel_visible);
    assert!(state.ui.inspector_panel_visible);
    // v10: the v8 forward-looking keys are dead — dropped on load, replaced by
    // the (defaulted) collapse set.
    assert!(state.ui.catalog_collapsed.is_empty());
```

(rename the test `v8_loads_dropping_dead_tree_ui` and update its comment.)
- Wire-snapshot fixture (~line 634): replace the `ui:` literal:

```rust
        ui: SessionUiState {
            catalog_panel_visible: true,
            inspector_panel_visible: false,
            catalog_collapsed: vec!["local".into()],
        },
```

- [ ] **Step 5: Regenerate the wire snapshot**

```bash
cargo test -p dat0-app --test session_migration
```
Expected: FAIL only on `session_json_wire_format` (new `.snap.new` written). Inspect the diff — ONLY the `ui` block and `schema_version` may change. Then accept (cargo-insta is not installed on this box):

```bash
mv crates/dat0-app/tests/snapshots/session_migration__session_json_wire_format.snap.new \
   crates/dat0-app/tests/snapshots/session_migration__session_json_wire_format.snap
```
Strip the `assertion_line:` frontmatter key from the accepted file to match sibling snapshots, then re-run:

```bash
cargo test -p dat0-app --test session_migration
cargo test -p dat0-app --lib session
```
Expected: all pass.

- [ ] **Step 6: fmt + commit**

```bash
cargo fmt --all
git add -A
git commit -s -m "feat(session): v10 — persist catalog collapse set, drop dead v8 tree-ui fields

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: GPUI behavioral tests — the UAT payload

**Files:**
- Modify: `crates/dat0-app/tests/catalog_nav.rs`

**Interfaces:**
- Consumes: everything above; `catalog_active_for_test`/`catalog_collapsed_for_test`; seeding helpers from Task 1.

Seed for every test below (3 top-level nodes → 6 visible rows expanded):

```rust
fn seed_tree(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) {
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(vec![
                sqlite_tbl("alpha"),
                sqlite_tbl("zeta"),
                md_tbl("md_events"),
                file_tbl("local_sales"),
            ]);
        });
    });
    vcx.run_until_parked();
}
// Visible rows, paint order (sections: Sources sorted [local_sales, sq], Cloud, Tables, Derived):
//   0 L0:local_sales   (Sources — file leaf; "local_sales" < "sq")
//   1 P :sq            (Sources — sqlite attach parent)
//   2 L1:alpha
//   3 L1:zeta
//   4 P :sample_data   (Cloud — md attach parent)
//   5 L1:md_events
```

- [ ] **Step 1: Arrows walk the full tree + clamp (test 2)**

```rust
#[gpui::test]
#[serial]
fn arrows_walk_visible_rows_and_clamp(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    let active = |vcx: &mut VisualTestContext, shell: &Entity<WorkspaceShell>| {
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test())
    };
    assert_eq!(active(vcx, &shell), 0);
    vcx.simulate_keystrokes("up"); // clamp at top
    vcx.run_until_parked();
    assert_eq!(active(vcx, &shell), 0, "Up at row 0 clamps");
    for _ in 0..7 {
        vcx.simulate_keystrokes("down"); // 6 rows: 5 moves + 2 clamped
    }
    vcx.run_until_parked();
    assert_eq!(active(vcx, &shell), 5, "Down clamps at the last visible row");
    drop(state);
}
```

- [ ] **Step 2: ← child→parent, ← parent collapses (absence teeth), → re-expands (tests 3–5)**

```rust
#[gpui::test]
#[serial]
fn left_jumps_to_parent_then_collapses_children_vanish(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    // Walk to row 3 (child "zeta" of parent "sq").
    for _ in 0..3 {
        vcx.simulate_keystrokes("down");
    }
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        3
    );

    // ← on a child jumps to ITS parent (row 1), not any earlier row.
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        1,
        "Left on a child moves to its parent"
    );

    // Expanded children are painted before the collapse…
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("alpha"), "child renders while expanded");
    assert!(snap.has_label("zeta"));

    // ← on the (expanded) parent collapses it: children VANISH from the a11y
    // tree (absence teeth — render-conditioned seams, R2).
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        !snap.has_label("alpha") && !snap.has_label("zeta"),
        "collapsed children must not render"
    );
    assert!(
        snap.has_label("md_events"),
        "the OTHER parent's children are untouched"
    );
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_collapsed_for_test()),
        vec!["sq".to_string()]
    );

    // → on the collapsed parent re-expands; children return.
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("alpha"), "right re-expands the parent");
    assert!(
        shell
            .update(vcx, |ws, _cx| ws.catalog_collapsed_for_test())
            .is_empty()
    );
    drop(state);
}
```

- [ ] **Step 3: Enter on a parent toggles via the activate path (test 6)**

```rust
#[gpui::test]
#[serial]
fn enter_on_parent_toggles_collapse(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    vcx.simulate_keystrokes("down"); // row 1 = parent "sq"
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter"); // focus_stop activate → Toggle
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_collapsed_for_test()),
        vec!["sq".to_string()],
        "Enter on an expanded parent collapses it"
    );
    let snap = A11ySnapshot::capture(vcx);
    assert!(!snap.has_label("alpha"), "children vanish on Enter-collapse");
    drop(state);
}
```

- [ ] **Step 4: Collapse persists to session.json (test 7)**

```rust
/// Locate the session.json Session::new created under `state_root` (a
/// UUID-named scratch dir one level down).
fn find_session_json(state_root: &Path) -> PathBuf {
    for entry in std::fs::read_dir(state_root).expect("read state root") {
        let p = entry.expect("dir entry").path();
        if p.is_dir() && p.join("session.json").exists() {
            return p.join("session.json");
        }
    }
    panic!("no session.json under {state_root:?}");
}

#[gpui::test]
#[serial]
fn collapse_state_persists_to_session_json(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);

    // Toggle via the production single-source method (mouse + kbd both route
    // here); it calls persist_dock_ui → session.json.
    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.toggle_catalog_parent("sample_data".to_string(), cx);
        });
    });
    vcx.run_until_parked();

    let raw = std::fs::read_to_string(find_session_json(state.path())).expect("session.json");
    assert!(
        raw.contains(r#""catalog_collapsed""#) && raw.contains(r#""sample_data""#),
        "collapsed alias must be persisted in the session ui block; got: {raw}"
    );
    drop(state);
}
```

- [ ] **Step 5: Enter-on-leaf probe (R5 — decides drivable vs stays-human)**

Write this test, run it ONCE, and act on the outcome:

```rust
/// R5 probe: Enter on a LEAF routes to `open_table_tab`, which does off-thread
/// engine work. The seeded fakes name tables the test engine doesn't have —
/// the Slice-3 precedent says the open path warns+drops gracefully under a
/// tokio runtime. If this test panics/hangs instead: DELETE it and note the
/// honest cut in the task report (the `Open` arm is unit-covered in nav.rs;
/// the real open stays human, recents precedent).
#[gpui::test]
#[serial]
fn enter_on_leaf_reaches_open_table_tab_gracefully(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_tree(&shell, vcx);
    focus_shell_neutrally(vcx);
    tab_to_catalog(vcx);

    // Row 0 is the top-level leaf "local_sales"; Enter must not panic the
    // frame loop (graceful warn+drop with no dispatcher), and the nav state
    // must remain sane afterwards.
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(vcx, |ws, _cx| ws.catalog_active_for_test()),
        0,
        "nav state stays sane after Enter-on-leaf"
    );
    drop(state);
}
```

- [ ] **Step 6: Panel-hidden negative — the catalog stop is gated on visibility**

```rust
/// The catalog container is a tab stop ONLY while the panel is visible (the
/// focus_stop lives inside the `catalog_panel_visible.then(..)` render branch).
/// With no seed (panel hidden), a bounded Tab walk must never land on it —
/// this guards the hero/settings Tab sequences other suites assert.
#[gpui::test]
#[serial]
fn hidden_panel_is_not_a_tab_stop(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (_shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // NO seed_catalog_tree_for_test → catalog_panel_visible stays false.
    focus_shell_neutrally(vcx);

    let title = dat0_i18n::t("catalog.title");
    for _ in 0..20 {
        press_tab(vcx);
        let snap = A11ySnapshot::capture(vcx);
        assert_ne!(
            snap.focused_label(),
            Some(title.as_str()),
            "hidden catalog panel must not be Tab-reachable"
        );
    }
    drop(state);
}
```

- [ ] **Step 7: Run the suite**

```bash
cargo test -p dat0-app --test catalog_nav
```
Expected: 7 tests pass (or 6 + the documented R5 cut).

- [ ] **Step 8: fmt + commit**

```bash
cargo fmt --all
git add -A
git commit -s -m "test(harness): catalog tree hierarchy + keyboard-nav UAT automation

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Controller gate (CONTROLLER runs this, not an implementer subagent)

- [ ] **Step 1: Full workspace gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```
Expected: all green. Watch specifically for: `a11y_spike` node-count drift (expected none — catalog hidden in its scene), `keyboard_nav`/`recents_nav` Tab-sequence changes (catalog stop only exists when the panel is visible — their scenes keep it hidden), stale `schema_version: 9` fixtures anywhere (`rg '"schema_version":\s*9' crates/`).

- [ ] **Step 2: Release build, feature off**

```bash
cargo build --release -p dat0-app
```
Expected: compiles clean (accessors cfg'd out; production nav code compiles in release).

- [ ] **Step 3: Dependency-drift check**

```bash
git diff --stat main -- Cargo.lock NOTICE
```
Expected: empty (zero new deps, D-015 stays open).

- [ ] **Step 4: No-new-div / seam audit**

READ the diff (`git diff main -- crates/dat0-app/src/catalog/panel.rs`) — new elements are expected THIS slice (parent rows are a real feature), but confirm: no test-only wrapper divs in release paths; `.a11y`/`.a11y_label` only chained onto existing/feature elements; `use` statements NOT cfg-gated (unconditional chains).

- [ ] **Step 5: Commit any gate fixes, then hand to review**

Per-task reviews already ran (SDD); final opus whole-branch review → PR → green both platforms → squash-merge → **watch the post-merge main run** (macOS grid-scroll bench is push-to-main-only) + crash-e2e.

---

## Verification (whole-slice acceptance)

1. `cargo test -p dat0-app --test catalog_nav` — 7 (or 6 + documented R5 cut) pass.
2. `cargo test -p dat0-app --test motherduck_window` — 5 pass (Slice-5 teeth survive grouping).
3. `cargo test -p dat0-app --test session_migration` + `--lib session` — v10 arm + wire snap green.
4. `cargo test --workspace --no-fail-fast` + clippy `-D warnings` + fmt — green.
5. Owed human glance (recorded, not automated): chevron/indent pixels, active-row ring, container/row double-ring, WCAG ≥3:1 both themes; live attached-DB grouping with a real engine.
