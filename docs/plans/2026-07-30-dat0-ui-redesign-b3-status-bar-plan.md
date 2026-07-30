# Slice B3 — Status bar: implementation plan

> **For agentic workers:** steps use checkbox (`- [ ]`) syntax for tracking. One commit
> per task. Design: `docs/plans/2026-07-30-dat0-ui-redesign-b3-status-bar-design.md`.

**Goal:** add a permanent, read-only status bar at the bottom of the workspace shell
showing `rows × cols`, selection count, last-query timing, and connection state.

**Architecture:** one new module `crates/dat0-app/src/view/status_bar.rs` holding a
snapshot struct (`StatusBarModel`), pure string builders, and one free render fn — the
`view/pipeline_bar.rs` shape. `WorkspaceShell::render` builds the snapshot from cached
scalars it already holds and mounts the bar as the last flow child. One new pure method
on `SelectionModel`. No entity, no state, no session schema change, no focus stops.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35`, `dat0_i18n`,
A2 token scales (`Sp` / `TextRole` / `Dat0Colors`), the `a11y-capture` test harness.

## Global Constraints

- **Do not bump the gpui-component rev.** There is no `StatusBar` component at
  `0f0ab35`; this slice hand-rolls one.
- **No colour literals.** Read tokens via `cx.theme()` / `cx.theme().d0()`. The
  `tests/style_lint.rs` ratchet must stay exactly `ALLOW = &[("window.rs", 1)]`.
  The scanner also matches banned colour-constructor names **in prose**, so do not
  spell them with call parens in doc comments.
- **No focus stops, no `tab_index`, no `tab_stop`, no click handlers** anywhere in this
  slice. Nav cycle counts must stay structurally unchanged.
- **Never add a second a11y label to an element that already has one** — `a11y()` and
  `a11y_label()` both push a new capture node.
- **i18n keys must be literal**, never `format!`-constructed. `en.json` silently
  overwrites duplicate keys, so check a key does not already exist before adding it.
- **`cargo test --workspace` and `cargo bench` are unrunnable on this machine**
  (pre-existing macOS 27 / Xcode 26.6 DuckDB-Thrift breakage, reproduces on `main`).
  The substitute gate is `-p dat0-app` plus the feature combos in Task 4.
- After reverting any probe edit, `touch` the file before re-running — an `mv`-revert
  backwards-dates it and cargo silently reuses the stale binary.

---

### Task 0: Mount gate — prove a new tree node breaks nothing

The status bar adds `Label` nodes to the AccessKit tree. `A11ySnapshot::has_label` and
`query_by_role` **panic on duplicate matches**, and some suites assert focus order or
node counts. Find out now, with a probe, before any real content exists.

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (in `WorkspaceShell::render`, after the body
  row's `.child(...)`, before `.children(popover_overlay)`)

**Interfaces:**
- Consumes: nothing.
- Produces: the mount point Task 3 replaces. No public API.

- [ ] **Step 1: Add the probe row**

In `WorkspaceShell::render`, immediately after the body-row `.child(` … `)` block that
ends with the charts dock `})),` and immediately before `.children(popover_overlay)`,
insert:

```rust
            // B3 Task 0 PROBE — replaced by the real status bar in Task 3.
            .child(
                gpui_component::h_flex()
                    .w_full()
                    .border_t_1()
                    .child(div().a11y_label(
                        crate::a11y::AccessRole::Label,
                        "b3-probe".to_string(),
                    )),
            )
```

`crate::a11y::A11yExt` is already imported in `window.rs`; if the compiler says
otherwise, add `use crate::a11y::A11yExt as _;` rather than a path-qualified call.

- [ ] **Step 2: Run the full capture suite**

```bash
cd /Users/salar/Projects/dat0
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b3-t0.log | tail -40
grep -c "test result: ok" /tmp/b3-t0.log
```

Expected: `111` and zero failures. **If anything fails, stop and report** — a collision
found here changes Task 3's design (segment wording, or a `#[cfg]`-gated bar in tests).
Do not "fix" a failing suite by weakening its assertion.

- [ ] **Step 3: Revert the probe**

Delete the block added in Step 1, then:

```bash
touch crates/dat0-app/src/window.rs
cargo test -p dat0-app --features a11y-capture 2>&1 | grep -c "test result: ok"
```

Expected: `111` again. Nothing is committed by this task — it is a gate, and its result
goes in the Task 1 commit message.

---

### Task 1: `SelectionModel::selected_cell_count`

**Files:**
- Modify: `crates/dat0-app/src/grid/selection.rs` (new method after `resolved_cells`,
  new tests in the file's existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `SelectionModel::{ranges, contains}` (private fields, same module).
- Produces: `pub fn selected_cell_count(&self) -> usize` — used by Task 3.

- [ ] **Step 1: Write the failing tests**

Append to `selection.rs`'s existing `mod tests`:

```rust
    #[test]
    fn selected_cell_count_is_zero_with_no_selection() {
        let m = SelectionModel::new(10, 10);
        assert_eq!(m.selected_cell_count(), 0);
    }

    #[test]
    fn selected_cell_count_counts_a_single_cell() {
        let mut m = SelectionModel::new(10, 10);
        m.click(CellCoord { row: 2, col: 3 });
        assert_eq!(m.selected_cell_count(), 1);
    }

    #[test]
    fn selected_cell_count_counts_a_rectangle() {
        let mut m = SelectionModel::new(10, 10);
        m.click(CellCoord { row: 1, col: 1 });
        m.extend_to(CellCoord { row: 3, col: 4 });
        // 3 rows (1..=3) x 4 cols (1..=4)
        assert_eq!(m.selected_cell_count(), 12);
    }

    #[test]
    fn selected_cell_count_normalises_a_backwards_rectangle() {
        let mut m = SelectionModel::new(10, 10);
        m.click(CellCoord { row: 3, col: 4 });
        m.extend_to(CellCoord { row: 1, col: 1 });
        assert_eq!(m.selected_cell_count(), 12);
    }

    #[test]
    fn selected_cell_count_sums_disjoint_ranges() {
        let mut m = SelectionModel::new(10, 10);
        m.click(CellCoord { row: 0, col: 0 });
        m.extend_to(CellCoord { row: 1, col: 1 }); // 4 cells
        m.add_click(CellCoord { row: 5, col: 5 });
        m.extend_to(CellCoord { row: 5, col: 7 }); // 3 cells
        assert_eq!(m.selected_cell_count(), 7);
    }

    #[test]
    fn selected_cell_count_counts_overlapping_ranges_once() {
        let mut m = SelectionModel::new(10, 10);
        m.click(CellCoord { row: 0, col: 0 });
        m.extend_to(CellCoord { row: 2, col: 2 }); // 9 cells
        m.add_click(CellCoord { row: 1, col: 1 });
        m.extend_to(CellCoord { row: 3, col: 3 }); // 9 cells, 4 of them shared
        // Union = 9 + 9 - 4. A sum-of-areas implementation returns 18.
        assert_eq!(m.selected_cell_count(), 14);
    }

    #[test]
    fn selected_cell_count_ignores_a_click_inside_an_existing_range() {
        let mut m = SelectionModel::new(10, 10);
        m.click(CellCoord { row: 0, col: 0 });
        m.extend_to(CellCoord { row: 4, col: 4 }); // 25 cells
        m.add_click(CellCoord { row: 2, col: 2 }); // already inside
        assert_eq!(m.selected_cell_count(), 25);
    }

    /// The whole reason this method exists: `select_all` spans the entire grid
    /// (the model is built with `rows = data_source.row_count`), and the status
    /// bar calls this EVERY FRAME. A `resolved_cells().count()` implementation
    /// would not fail this test — it would hang it.
    #[test]
    fn selected_cell_count_is_arithmetic_not_per_cell() {
        let mut m = SelectionModel::new(1_000_000, 20);
        m.select_all();
        assert_eq!(m.selected_cell_count(), 20_000_000);
    }
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd /Users/salar/Projects/dat0
cargo test -p dat0-app --lib selected_cell_count 2>&1 | tail -20
```

Expected: compile error — `no method named 'selected_cell_count'`.

- [ ] **Step 3: Implement it**

Insert into `impl SelectionModel`, directly after `resolved_cells`:

```rust
    /// Number of distinct selected cells, computed arithmetically from the
    /// rectangles: overlapping ranges are counted once, and the cost depends on
    /// the number of RANGES, never on the number of selected cells.
    ///
    /// Deliberately NOT `resolved_cells().count()`. That materialises one set
    /// entry per selected cell, and [`Self::select_all`] spans the whole grid
    /// (the model is constructed with `rows = GridDataSource::row_count`), so a
    /// per-frame caller — the status bar — would build a 20-million-element set
    /// every frame on a 1M-row table.
    ///
    /// Coordinate compression: the rectangles' edges cut the plane into bands,
    /// and every band-block is either wholly inside a range or wholly outside
    /// it, so one `contains` probe on the block's lower corner decides it.
    pub fn selected_cell_count(&self) -> usize {
        if self.ranges.is_empty() {
            return 0;
        }
        let mut rows: Vec<usize> = Vec::with_capacity(self.ranges.len() * 2);
        let mut cols: Vec<usize> = Vec::with_capacity(self.ranges.len() * 2);
        for rg in &self.ranges {
            rows.push(rg.r0.min(rg.r1));
            rows.push(rg.r0.max(rg.r1).saturating_add(1));
            cols.push(rg.c0.min(rg.c1));
            cols.push(rg.c0.max(rg.c1).saturating_add(1));
        }
        rows.sort_unstable();
        rows.dedup();
        cols.sort_unstable();
        cols.dedup();

        let mut total: usize = 0;
        for r_band in rows.windows(2) {
            for c_band in cols.windows(2) {
                if self.contains(r_band[0], c_band[0]) {
                    let cells = (r_band[1] - r_band[0]).saturating_mul(c_band[1] - c_band[0]);
                    total = total.saturating_add(cells);
                }
            }
        }
        total
    }
```

- [ ] **Step 4: Run them and watch them pass**

```bash
cargo test -p dat0-app --lib selected_cell_count 2>&1 | tail -20
```

Expected: `test result: ok. 8 passed`. The 1M-row test must return in milliseconds; if
it hangs, the implementation went through `resolved_cells`.

- [ ] **Step 5: Commit**

```bash
cd /Users/salar/Projects/dat0
git add crates/dat0-app/src/grid/selection.rs
git commit -s -F - <<'EOF'
feat(theme): B3 T1 — exact selection cell count without per-cell work

SelectionModel::selected_cell_count computes the union area of the selected
rectangles by coordinate compression: overlapping ranges count once, and cost
scales with the number of RANGES, never the number of selected cells.

resolved_cells() is O(selected cells) — it builds one set entry per cell — and
select_all spans the whole grid, so the status bar (which reads this every
frame) would build a 20-million-element set per frame on a 1M-row table.

Task 0's mount gate ran clean beforehand: a probe node added to the shell's
render tree left all 111 a11y-capture binaries green, so new Label nodes do not
collide with the suite's unique-match queries.
EOF
```

---

### Task 2: `view/status_bar.rs` — model, pure string builders, i18n keys

**Files:**
- Create: `crates/dat0-app/src/view/status_bar.rs`
- Modify: `crates/dat0-app/src/view/mod.rs` (add `pub mod status_bar;`)
- Modify: `crates/dat0-i18n/src/strings/en.json` (9 new `status.*` keys)

**Interfaces:**
- Consumes: `SelectionModel::selected_cell_count` (Task 1, called by Task 3);
  `crate::connections::{ConnectionManager, ConnectionStatus}`;
  `crate::connections::routing::Routing` (`Copy`).
- Produces, all used by Task 3:
  - `pub enum QueryStatus { Idle, Running, Done { ms: u64, routing: Option<Routing> } }`
  - `pub struct StatusBarModel { rows: Option<u64>, cols: Option<usize>, selected_cells: Option<usize>, query: QueryStatus, connection: String }` (all fields `pub`)
  - `pub fn format_count(n: u64) -> String`
  - `pub fn describe_connection(conns: &ConnectionManager) -> String`
  - `pub fn StatusBarModel::segments(&self) -> Vec<String>`

- [ ] **Step 1: Confirm the i18n keys are free**

```bash
cd /Users/salar/Projects/dat0
python3 -c "
import json; d=json.load(open('crates/dat0-i18n/src/strings/en.json'))
print([k for k in d if k.startswith('status.')])"
```

Expected: `[]`. (A non-empty list means a key would be silently overwritten — stop and
rename.) Then add these nine entries to `en.json`, keeping the file's existing ordering
convention (append in a `status.*` block):

```json
  "status.rows": "rows",
  "status.cols": "cols",
  "status.cells_selected": "cells selected",
  "status.query": "Query",
  "status.query_running": "Query running…",
  "status.conn_local": "Local",
  "status.conn_md": "MotherDuck",
  "status.conn_connecting": "Connecting…",
  "status.conn_error": "Connection error",
  "status.attached": "attached",
```

(That is ten lines: nine content keys plus `status.attached`.)

- [ ] **Step 2: Write the failing tests**

Create `crates/dat0-app/src/view/status_bar.rs` containing ONLY the module doc comment
and this test module for now:

```rust
//! Status bar: permanent, read-only chrome along the bottom of the workspace
//! shell (UI redesign slice B3). Shows `rows × cols`, the selection size, the
//! last SQL run's timing, and the connection state.
//!
//! Shape mirrors [`crate::view::pipeline_bar`]: a snapshot struct plus pure
//! string builders that need no window, and one free render fn. The bar owns no
//! state, mints no focus handles, and registers no click handlers — it is
//! chrome, not a control, and every keyboard-nav cycle count in the suite is
//! unchanged by construction.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::{ConnectionManager, ConnectionStatus};
    use crate::connections::routing::Routing;

    #[test]
    fn format_count_groups_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(1), "1");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn describe_connection_maps_every_status() {
        let mut m = ConnectionManager::default();
        assert_eq!(describe_connection(&m), "Local");
        m.set_md_status(ConnectionStatus::Connecting);
        assert_eq!(describe_connection(&m), "Connecting…");
        m.set_md_status(ConnectionStatus::Connected);
        assert_eq!(describe_connection(&m), "MotherDuck");
        m.set_md_status(ConnectionStatus::Error("nope".into()));
        assert_eq!(describe_connection(&m), "Connection error");
    }

    /// The error text itself must never reach the bar — it can carry a server
    /// message, and this surface is 12px muted chrome with no room for one.
    #[test]
    fn describe_connection_never_leaks_the_error_body() {
        let mut m = ConnectionManager::default();
        m.set_md_status(ConnectionStatus::Error("token rejected by md".into()));
        assert!(!describe_connection(&m).contains("token"));
    }

    #[test]
    fn describe_connection_appends_attachment_count() {
        let mut m = ConnectionManager::default();
        m.add_sqlite("shop".into(), "/tmp/shop.db".into());
        m.add_sqlite("logs".into(), "/tmp/logs.db".into());
        assert_eq!(describe_connection(&m), "Local · 2 attached");
    }

    fn model() -> StatusBarModel {
        StatusBarModel {
            rows: None,
            cols: None,
            selected_cells: None,
            query: QueryStatus::Idle,
            connection: "Local".to_string(),
        }
    }

    #[test]
    fn segments_of_an_empty_model_are_connection_only() {
        assert_eq!(model().segments(), vec!["Local".to_string()]);
    }

    #[test]
    fn segments_render_shape_only_when_both_dimensions_are_known() {
        let mut m = model();
        m.rows = Some(1_234);
        assert_eq!(m.segments(), vec!["Local".to_string()]);
        m.cols = Some(12);
        assert_eq!(
            m.segments(),
            vec!["1,234 rows × 12 cols".to_string(), "Local".to_string()]
        );
    }

    #[test]
    fn segments_hide_an_empty_selection() {
        let mut m = model();
        m.selected_cells = Some(0);
        assert_eq!(m.segments(), vec!["Local".to_string()]);
        m.selected_cells = Some(84);
        assert_eq!(
            m.segments(),
            vec!["84 cells selected".to_string(), "Local".to_string()]
        );
    }

    #[test]
    fn segments_render_query_state() {
        let mut m = model();
        m.query = QueryStatus::Running;
        assert_eq!(
            m.segments(),
            vec!["Query running…".to_string(), "Local".to_string()]
        );
        m.query = QueryStatus::Done {
            ms: 12,
            routing: Some(Routing::Md),
        };
        assert_eq!(
            m.segments(),
            vec!["Query 12 ms · md".to_string(), "Local".to_string()]
        );
        m.query = QueryStatus::Done {
            ms: 12,
            routing: None,
        };
        assert_eq!(
            m.segments(),
            vec!["Query 12 ms · local".to_string(), "Local".to_string()]
        );
    }

    #[test]
    fn segments_render_in_a_fixed_order() {
        let m = StatusBarModel {
            rows: Some(2),
            cols: Some(2),
            selected_cells: Some(4),
            query: QueryStatus::Done {
                ms: 7,
                routing: None,
            },
            connection: "Local".to_string(),
        };
        assert_eq!(
            m.segments(),
            vec![
                "2 rows × 2 cols".to_string(),
                "4 cells selected".to_string(),
                "Query 7 ms · local".to_string(),
                "Local".to_string(),
            ]
        );
    }
}
```

Note `sql.md` resolves to `md` and `sql.local` to `local` in `en.json` — that is where
the routing tail's wording comes from, and it is why the console chip and this segment
can never disagree.

- [ ] **Step 3: Run them and watch them fail**

```bash
cd /Users/salar/Projects/dat0
sed -i '' 's/^pub mod sql_console;/pub mod sql_console;\npub mod status_bar;/' crates/dat0-app/src/view/mod.rs
grep -n "status_bar" crates/dat0-app/src/view/mod.rs
cargo test -p dat0-app --lib status_bar 2>&1 | tail -20
```

Expected: compile errors — `cannot find function 'format_count'` and friends. If the
`sed` did not match, add `pub mod status_bar;` to `view/mod.rs` by hand in alphabetical
position and re-run.

- [ ] **Step 4: Implement the module body**

Insert above the `#[cfg(test)] mod tests` block:

```rust
use crate::connections::routing::Routing;
use crate::connections::{ConnectionManager, ConnectionStatus};

/// What the last SQL run is doing, as far as the status bar is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryStatus {
    /// No console, or no run has completed yet.
    #[default]
    Idle,
    Running,
    Done { ms: u64, routing: Option<Routing> },
}

/// Everything the bar renders, snapshotted from the shell once per frame.
///
/// `rows`/`cols` are `None` until a data source is mounted; `selected_cells` is
/// `None` until the grid has a selection. `connection` is pre-rendered by
/// [`describe_connection`] because the shell owns the [`ConnectionManager`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusBarModel {
    pub rows: Option<u64>,
    pub cols: Option<usize>,
    pub selected_cells: Option<usize>,
    pub query: QueryStatus,
    pub connection: String,
}

/// Group a count into thousands with `,` separators. dat0 ships English only
/// (`dat0_i18n::t` has no interpolation), so this is deliberately not locale-aware.
pub fn format_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.char_indices() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// One-line connection summary: the MotherDuck state, plus the number of
/// attached SQLite databases when there are any.
///
/// The `Error` variant's payload is deliberately dropped — it can carry a server
/// message, and this surface has no room for one. The Connections panel shows it.
pub fn describe_connection(conns: &ConnectionManager) -> String {
    let state = match conns.md_status() {
        ConnectionStatus::Connected => dat0_i18n::t("status.conn_md"),
        ConnectionStatus::Connecting => dat0_i18n::t("status.conn_connecting"),
        ConnectionStatus::Error(_) => dat0_i18n::t("status.conn_error"),
        ConnectionStatus::Disconnected => dat0_i18n::t("status.conn_local"),
    };
    let attached = conns.sqlite().len();
    if attached == 0 {
        state
    } else {
        format!("{state} · {attached} {}", dat0_i18n::t("status.attached"))
    }
}

impl StatusBarModel {
    /// The bar's segment strings, in render order. Pure, so the entire rendered
    /// text of the bar is assertable with no window.
    pub fn segments(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(4);
        if let (Some(rows), Some(cols)) = (self.rows, self.cols) {
            out.push(format!(
                "{} {} × {} {}",
                format_count(rows),
                dat0_i18n::t("status.rows"),
                cols,
                dat0_i18n::t("status.cols"),
            ));
        }
        if let Some(n) = self.selected_cells.filter(|n| *n > 0) {
            out.push(format!(
                "{} {}",
                format_count(n as u64),
                dat0_i18n::t("status.cells_selected"),
            ));
        }
        match self.query {
            QueryStatus::Idle => {}
            QueryStatus::Running => out.push(dat0_i18n::t("status.query_running")),
            QueryStatus::Done { ms, routing } => {
                // Same tail as the SQL console's own chip (`sql.local`/`sql.md`/
                // `sql.mixed`), so the two surfaces can never word it differently.
                let key = routing.map(|r| r.i18n_key()).unwrap_or("sql.local");
                out.push(format!(
                    "{} {ms} ms · {}",
                    dat0_i18n::t("status.query"),
                    dat0_i18n::t(key),
                ));
            }
        }
        out.push(self.connection.clone());
        out
    }
}
```

- [ ] **Step 5: Run them and watch them pass**

```bash
cargo test -p dat0-app --lib status_bar 2>&1 | tail -20
cargo clippy -p dat0-app --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: `test result: ok. 9 passed` and clippy exit 0. `char_indices` (not
`chars().enumerate()`) is what makes the `(len - i) % 3` arithmetic correct for
multi-byte input — digits are ASCII here, but the byte/char distinction is exactly the
kind of thing that rots later.

- [ ] **Step 6: Commit**

```bash
cd /Users/salar/Projects/dat0
git add crates/dat0-app/src/view/status_bar.rs crates/dat0-app/src/view/mod.rs crates/dat0-i18n/src/strings/en.json
git commit -s -F - <<'EOF'
feat(theme): B3 T2 — status bar model and pure segment builders

StatusBarModel snapshots what the bar shows (rows/cols, selection size, query
state, connection) and segments() turns it into the exact strings that get
rendered — pure, so the bar's whole content is assertable with no window.

format_count groups thousands; describe_connection maps all four
ConnectionStatus variants and appends the SQLite attachment count, dropping the
Error payload so a server message can never land in 12px muted chrome.

The query segment reuses the SQL console chip's routing tail (sql.local /
sql.md / sql.mixed) so the two surfaces cannot word it differently.

Ten new literal status.* i18n keys; verified none existed beforehand (en.json
silently overwrites duplicates).
EOF
```

---

### Task 3: Render the bar and mount it on the shell

**Files:**
- Modify: `crates/dat0-app/src/view/status_bar.rs` (add `render_status_bar`)
- Modify: `crates/dat0-app/src/window.rs` (build the model in `render`, mount the bar)

**Interfaces:**
- Consumes: Task 2's `StatusBarModel::segments`, Task 1's `selected_cell_count`.
- Produces: `pub fn render_status_bar(model: &StatusBarModel, cx: &gpui::App) -> impl IntoElement`.

- [ ] **Step 1: Write the render fn**

Add to `status_bar.rs`, above the test module (extend the `use` block at the top):

```rust
use crate::a11y::{A11yExt as _, AccessRole};
use crate::theme::tokens::{Dat0Theme as _, Sp, SpStyled as _, TextRole, TypoStyled as _};
use gpui::{App, IntoElement, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, h_flex};
```

```rust
/// Render the status bar: one muted text segment per [`StatusBarModel::segments`]
/// entry, separated by a thin vertical rule.
///
/// The fill is the plain `background` token: `theme_contrast_gate` gates
/// `muted.foreground` against `background` at 4.5:1 on all three builtins, while
/// muted text on a raised fill is recorded there as measuring ≈4.0 in dark. A
/// distinct fill would mean retuning the palette.
///
/// Every segment carries a content-only a11y `Label` node. Nothing here is
/// focusable or clickable — the bar adds no tab stops.
pub fn render_status_bar(model: &StatusBarModel, cx: &App) -> impl IntoElement {
    let border = cx.theme().border;
    let segments = model.segments();
    let last = segments.len().saturating_sub(1);
    h_flex()
        .w_full()
        .items_center()
        .flex_shrink_0()
        .bg(cx.theme().background)
        .border_t_1()
        .border_color(border)
        .px_sp(Sp::S12)
        .py_sp(Sp::S4)
        .gap_sp(Sp::S12)
        .text_role(TextRole::Small)
        .text_color(cx.theme().d0().text_muted)
        .children(segments.into_iter().enumerate().map(move |(i, text)| {
            h_flex()
                .items_center()
                .gap_sp(Sp::S12)
                .child(
                    div()
                        .a11y_label(AccessRole::Label, text.clone())
                        .child(SharedString::from(text)),
                )
                .children((i < last).then(|| {
                    div()
                        .w(Sp::S1.pixels())
                        .h(Sp::S12.pixels())
                        .bg(border)
                }))
        }))
}
```

- [ ] **Step 2: Build the model in the shell**

In `crates/dat0-app/src/window.rs`, inside `WorkspaceShell::render`, immediately before
the final `div().id("workspace-shell")` builder chain (next to where `catalog_fh` and
`ai_handles` are hoisted), insert:

```rust
        // B3 status bar. Reads cached scalars only — no query, no I/O, and (per
        // `SelectionModel::selected_cell_count`'s doc comment) no per-cell work,
        // which matters because this runs every frame.
        let status_bar_model = {
            let rows = self.data_source.as_ref().map(|ds| ds.row_count);
            let cols = self.data_source.as_ref().map(|ds| {
                // `column_view` is the POST-projection column list — what the grid
                // actually paints. It is refreshed on every rebind and stack
                // change, but fall back to the source's own count rather than
                // rendering "× 0 cols" if it is ever momentarily empty.
                if self.column_view.is_empty() {
                    ds.visible_column_count()
                } else {
                    self.column_view.len()
                }
            });
            let selected_cells = self
                .selection
                .as_ref()
                .filter(|s| s.has_selection())
                .map(|s| s.selected_cell_count());
            let query = match self.sql_console.as_ref().map(|c| c.read(cx)) {
                Some(c) if c.running => crate::view::status_bar::QueryStatus::Running,
                Some(c) => match c.last_elapsed_ms {
                    Some(ms) => crate::view::status_bar::QueryStatus::Done {
                        ms,
                        routing: c.last_routing,
                    },
                    None => crate::view::status_bar::QueryStatus::Idle,
                },
                None => crate::view::status_bar::QueryStatus::Idle,
            };
            crate::view::status_bar::StatusBarModel {
                rows,
                cols,
                selected_cells,
                query,
                connection: crate::view::status_bar::describe_connection(&self.connections),
            }
        };
```

- [ ] **Step 3: Mount it**

In the same fn, after the body-row `.child(` … `)` block (the `flex_row` that ends with
the charts dock, closing with `})),` then `),`) and immediately before
`.children(popover_overlay)`, insert:

```rust
            // B3: status bar spans the full width UNDER every dock, so it is a
            // sibling of the body row rather than a child of it. The three
            // overlays below are `.absolute()`, so they still paint above it.
            .child(crate::view::status_bar::render_status_bar(
                &status_bar_model,
                cx,
            ))
```

`cx` here is `&mut Context<Self>`, which deref-coerces to the `&App` the fn takes —
same as the existing `render_banner(b, cx)` call in this file.

- [ ] **Step 4: Build and run the app-level suites**

```bash
cd /Users/salar/Projects/dat0
cargo clippy -p dat0-app --all-targets -- -D warnings 2>&1 | tail -5
cargo test -p dat0-app 2>&1 | tail -5
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b3-t3.log | tail -5
grep -c "test result: ok" /tmp/b3-t3.log
```

Expected: clippy exit 0, `111`, zero failures. **This is the content-collision gate** —
Task 0 proved a new node is structurally safe, this proves the real strings
(`Local`, `2 rows × 2 cols`, …) do not collide with another suite's unique-match query.

- [ ] **Step 5: Confirm the ratchet did not move**

```bash
cargo test -p dat0-app --test style_lint 2>&1 | tail -5
```

Expected: `test result: ok. 4 passed`. If a colour count moved, a literal slipped in —
fix it by reading a token, never by raising the allowance.

- [ ] **Step 6: Commit**

```bash
cd /Users/salar/Projects/dat0
git add crates/dat0-app/src/view/status_bar.rs crates/dat0-app/src/window.rs
git commit -s -F - <<'EOF'
feat(theme): B3 T3 — render the status bar and mount it on the shell

render_status_bar paints one muted segment per StatusBarModel::segments entry,
separated by a thin rule, on the plain background token — theme_contrast_gate
gates muted.foreground against background at 4.5:1 on all three builtins, while
muted text on a raised fill is recorded there as measuring ~4.0 in dark.

The shell builds the snapshot from cached scalars it already holds and mounts
the bar as a sibling of the body row, so it spans the full width under every
dock. No focus handles, no tab stops, no click handlers: nav cycle counts are
unchanged by construction.

Column count reads the post-projection column_view (what the grid actually
paints), falling back to the source's own count so a momentarily empty fold
cannot render "x 0 cols".

All 111 a11y-capture binaries green with the real segment strings mounted, and
the style_lint colour ratchet is unmoved.
EOF
```

---

### Task 4: Rendered-content test, non-vacuity, and the full local gate

**Files:**
- Modify: `crates/dat0-app/tests/a11y_content.rs` (one new `#[gpui::test]`)

**Interfaces:**
- Consumes: the file's existing `open_shell_window`, `build_empty_session_in`,
  `ensure_dispatcher`, `init_components`, `enter_async_harness`, `set_config_dir`
  helpers, and `A11ySnapshot::{capture, has_label_contains}`.
- Produces: nothing.

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/a11y_content.rs`:

```rust
// ----------------------------------------------------------------------------
// B3: the status bar renders its segments as AccessKit content.
// ----------------------------------------------------------------------------

/// Mount a real `WorkspaceShell` over a two-row, two-column CSV and assert the
/// status bar's shape and connection segments reached the capture tree.
///
/// Duplicate-tolerant `has_label_contains` throughout: the query segment shares
/// its `N ms · local` tail with the SQL console's own timing chip, so a
/// unique-match query here would be a latent panic the moment a console is open
/// in the same window.
#[gpui::test]
#[serial]
fn status_bar_renders_segments_as_a11y_content(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let session = build_empty_session_in(&harness, state.path());

    let csv = state.path().join("bar.csv");
    std::fs::write(&csv, "a,b\n1,2\n3,4\n").unwrap();

    let (shell, cx) = open_shell_window(cx, Arc::clone(&session));
    cx.run_until_parked();

    // With no data source the bar still paints — the connection segment is
    // unconditional. This is the "always mounted" decision, asserted.
    let snap = A11ySnapshot::capture(cx);
    assert!(
        snap.has_label_contains("Local"),
        "the connection segment must render before any data is loaded"
    );
    assert!(
        !snap.has_label_contains("rows ×"),
        "the shape segment must be absent with no data source"
    );

    let sess = Arc::clone(&session);
    let csv2 = csv.clone();
    let task = cx.cx.spawn(async move |_app| {
        let _ = dat0_app::file_drop::handle_drop(vec![csv2], sess).await;
    });
    cx.executor().block_test(task);

    let engine = session.lock().engine.clone();
    let tables = harness
        .block_on(async { engine.get_tables().await })
        .expect("get_tables");
    let table_name = tables
        .iter()
        .map(|t| t.name.clone())
        .next()
        .expect("the CSV import must register exactly one table");
    let ds = harness
        .block_on(async { GridDataSource::new(Arc::clone(&engine), table_name).await })
        .expect("GridDataSource::new");
    shell.update(cx, |view, cx| {
        view.set_data_source(Arc::new(ds));
        cx.notify();
    });
    cx.run_until_parked();

    // The shape segment reads the source's row count and the post-projection
    // column list: "2 rows × 2 cols".
    let snap = A11ySnapshot::capture(cx);
    assert!(
        snap.has_label_contains("2 rows × 2 cols"),
        "the status bar must render the mounted grid's shape"
    );
    assert!(
        snap.has_label_contains("Local"),
        "the connection segment must survive the data-source mount"
    );
    // No run has happened and nothing is selected, so neither segment exists.
    assert!(
        !snap.has_label_contains("cells selected"),
        "no selection means no selection segment"
    );
    assert!(
        !snap.has_label_contains("Query "),
        "no completed run means no query segment"
    );
}
```

- [ ] **Step 2: Run it**

```bash
cd /Users/salar/Projects/dat0
cargo test -p dat0-app --features a11y-capture --test a11y_content status_bar 2>&1 | tail -20
```

Expected: PASS. (It is written after the implementation, so it should be green
immediately — Step 3 is what gives it teeth.)

- [ ] **Step 3: Prove it non-vacuous**

Temporarily change the shape assertion's needle to `"2 rows × 3 cols"` and re-run:

```bash
cargo test -p dat0-app --features a11y-capture --test a11y_content status_bar 2>&1 | tail -12
```

Expected: FAIL on `the status bar must render the mounted grid's shape`. Then revert the
needle, and:

```bash
touch crates/dat0-app/tests/a11y_content.rs
cargo test -p dat0-app --features a11y-capture --test a11y_content status_bar 2>&1 | tail -6
```

Expected: PASS. The `touch` is mandatory — a reverted file can end up older than the
built binary, and cargo will happily re-run the stale one.

- [ ] **Step 4: Full local gate**

```bash
cd /Users/salar/Projects/dat0
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5
cargo test -p dat0-app 2>&1 | tee /tmp/b3-plain.log | tail -3
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b3-a11y.log | tail -3
cargo test -p dat0-app --features a11y-capture,gallery 2>&1 | tee /tmp/b3-gallery.log | tail -3
for f in /tmp/b3-plain.log /tmp/b3-a11y.log /tmp/b3-gallery.log; do
  echo "$f: $(grep -c 'test result: ok' $f) ok / $(grep -c 'test result: FAILED' $f) failed"
done
bash scripts/i18n-check.sh
```

Expected: fmt silent, clippy exit 0, `111` ok and `0` failed in each of the three
combos, i18n check clean. Do **not** pipe a cargo test count through `head` — it
SIGPIPEs cargo mid-write and silently truncates the count.

- [ ] **Step 5: Commit**

```bash
cd /Users/salar/Projects/dat0
git add crates/dat0-app/tests/a11y_content.rs
git commit -s -F - <<'EOF'
test(theme): B3 T4 — assert the status bar's rendered content

One case appended to the existing a11y_content binary (no new test binary, so
no macOS CI disk cost): mount a real WorkspaceShell, assert the connection
segment paints before any data is loaded and the shape segment does not, then
import a 2x2 CSV and assert "2 rows x 2 cols" reaches the capture tree while
the selection and query segments stay absent.

Duplicate-tolerant has_label_contains throughout — the query segment shares its
"N ms . local" tail with the SQL console's own chip, so a unique-match query
would be a latent panic whenever a console is open in the same window.

Proven non-vacuous by perturbing the expected shape and confirming red.
EOF
```

---

## Self-review

**Spec coverage.** Design §2.1 module + model → Task 2 and Task 3; §2.2 selection count
→ Task 1; §2.3 segment content → Task 2 (strings) and Task 3 (mount, which supplies
`rows`/`cols`/`selected_cells`/`query`); §2.4 visuals → Task 3; §2.5 a11y → Task 3's
per-segment `a11y_label` and the absence of any focus API; §2.6 non-changes → nothing in
any task touches `grid/mod.rs`, the session schema, actions, or key bindings; §3 tests →
Tasks 1, 2, 4; §4 gates → Task 0 (mount gate), Task 3 Step 4/5, Task 4 Step 4.

**Type consistency.** `QueryStatus`, `StatusBarModel`, `format_count`,
`describe_connection`, `segments`, `render_status_bar`, `selected_cell_count` are spelled
identically in every task that mentions them, and Task 3's window.rs snippet constructs
`StatusBarModel` with exactly the five fields Task 2 declares.

**Known deviation from the master plan.** The module is `src/view/status_bar.rs`, not
`src/status_bar.rs` — rationale in the design doc §2.1.

**Owed after merge.** Watch the post-merge main run and verify the macOS bench at STEP
level (reclaim → bench → upload all success) even though `grid/mod.rs` is untouched;
download the artifact for the ns/iter. Then the owed human glance in design §5.
