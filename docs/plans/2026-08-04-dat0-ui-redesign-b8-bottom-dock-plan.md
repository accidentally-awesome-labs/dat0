# B8 — SQL console → bottom dock: Implementation Plan

> **For agentic workers:** this slice is executed INLINE by the controller (no
> subagents), matching every slice since A5. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Move the SQL console from a fixed 260 px strip above the grid into
the `DockArea`'s bottom dock, where it renders below the grid, is resizable,
and derives its visibility from the dock rather than a shell bool.

**Architecture:** `SqlConsole` gains `Panel`/`Focusable`/`EventEmitter<PanelEvent>`
impls directly (no wrapper entity). The DockArea's lazy construction is
extracted from `render` into `ensure_dock_area`, so `toggle_sql_console` can
mount the bottom dock on the first console open — `set_bottom_dock` exactly
once, `toggle_dock` thereafter. The `sql_console_visible` field is deleted and
replaced by a getter over `DockArea::is_dock_open(Bottom)`.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35`,
DuckDB. Design doc:
`docs/plans/2026-08-04-dat0-ui-redesign-b8-bottom-dock-design.md`.

## Global Constraints

- Branch `feat/ui-redesign-b8-bottom-dock` off main `9ceff53`. One commit per
  task. Sign off every commit (`git commit -s`).
- **Never write the literal CI-skip marker in any commit message**, even quoted
  in prose. Backticks in a `-m` string get command-substituted by zsh and
  vanish silently — use `git commit -s -F -` with a heredoc.
- `cargo clippy --workspace --all-targets -- -D warnings` must exit 0 at every
  commit. `-D warnings` makes an unused const or method a **hard error**, so a
  "declare now, use next commit" split does not build (B7).
- `tests/style_lint.rs` ratchet must stay **unchanged** at
  `[("window.rs", 1)]`. No new colour literals.
- `git diff` must stay **empty** for `src/grid` and `src/session`.
- Suite goes **115 → 116** binaries across `{plain, a11y-capture,
  a11y-capture+gallery}`.
- Drive keyboard behaviour with `simulate_keystrokes`, **never**
  `dispatch_action` — the latter bypasses the keymap, so a green test can hide
  a dead production key path.
- `cargo test --workspace` and `cargo bench` are **unrunnable on this machine**
  (macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift). Scope every run to
  `-p dat0-app`.
- After reverting any probe, **`touch` the reverted files and re-run** — an
  `mv`-revert backwards-dates the file and cargo reuses the stale binary,
  reporting a false red (A6).

---

## File Structure

| File | Responsibility | Task |
| --- | --- | --- |
| `src/panels/sql_console_panel.rs` | **Create.** `Panel`/`Focusable`/`EventEmitter<PanelEvent>` impls for `SqlConsole`, the frozen `PANEL_NAME`, and the closable-resolution pin. | T1 |
| `src/panels/mod.rs` | **Modify.** Declare the module; register the panel name with a degraded B9 builder. | T1 |
| `crates/dat0-i18n/src/strings/en.json` | **Modify.** One new key, `sql.console.title`. | T1 |
| `src/window.rs` | **Modify.** Extract `ensure_dock_area` (T2); delete the `sql_console_visible` field and the 260 px strip, add the derived getter, rewrite `toggle_sql_console` (T3). | T2, T3 |
| `tests/bottom_dock.rs` | **Create.** Mount, derived visibility, both upstream toggle paths, collapsed bar, frozen name, closable pin. | T4 |
| `tests/a11y_spike.rs` | **Modify.** Bump the exact node count with a comment naming the contributing nodes. | T3 |
| `tests/sql_console_nav.rs`, `tests/sql_console_transient_nav.rs`, `tests/input_nav.rs`, `tests/keyboard_nav.rs`, `tests/a11y_content.rs` | **Modify** only where the move genuinely changes them. | T5 |

---

## Task 0: Bottom-dock mechanics probe

Measure four things against the real tree, record them in the design doc, then
revert the throwaway wiring. B5, B6 and B7 each had a T0 and each found
something its design had wrong — B7's found a hard API constraint that
invalidated the design's central choice.

**Files:**
- Modify (throwaway, reverted in this same task): `src/window.rs`
- Create: `docs/plans/2026-08-04-dat0-ui-redesign-b8-bottom-dock-design.md` §14
  entries

**Interfaces:**
- Consumes: nothing.
- Produces: measured values that T3 and T4 depend on — the post-migration
  `a11y_spike` node count, the console's position in the whole-shell Tab order,
  and confirmation that `is_dock_open` tracks both upstream toggle paths.

- [ ] **Step 1: Wire a crude bottom dock**

In `src/window.rs`, inside the `if self.dock_area.is_none()` block (currently
`:7114-7242`), immediately before `self.dock_area = Some(dock);`, add a
throwaway mount. This is deliberately the *wrong* shape (eager, not lazy) —
it exists only to make the dock measurable this frame.

```rust
// T0 PROBE — REVERTED IN THIS SAME COMMIT. Do not ship.
let probe_console = cx.new(|cx| {
    crate::view::sql_console::SqlConsole::new(
        &[],
        0,
        self.sql_snapshot
            .get_or_insert_with(crate::query::completion::new_shared_snapshot)
            .clone(),
        window,
        cx,
    )
});
let bottom = gpui_component::dock::DockItem::tab(
    probe_console.clone(),
    &weak_dock,
    window,
    cx,
);
dock.update(cx, |dock, cx| {
    dock.set_bottom_dock(bottom, Some(gpui::px(320.)), true, window, cx);
});
```

This requires the `Panel` impls from T1 to compile. Write them **first, in this
working tree, uncommitted** (copy the T1 Step 1 code verbatim), and revert them
along with the probe at Step 6 — T1 re-adds them properly with tests.

- [ ] **Step 2: Measure (a) — the `a11y_spike` node count**

Run: `cargo test -p dat0-app --features a11y-capture --test a11y_spike`

Expected: **FAIL** on the exact-count assertion at `tests/a11y_spike.rs:126-127`
(`snap.click_ids.len()`, currently **12**). Record the *actual* number the
failure reports and which nodes account for the delta. Do **not** edit the
assertion in this task.

If it does **not** fail, stop and report: that would mean the TabPanel title
bar contributes no capture node, which contradicts B6's measurement that the
chrome is transparent but *present*, and the design's §8(a) prediction is
wrong in a way that needs understanding before T3.

- [ ] **Step 3: Measure (b) — nested scroll over the results table**

Write a throwaway test at `tests/bottom_dock.rs` (the file T4 will own
properly) that runs a `ResultTarget::Pane` query and asserts the console-owned
results `Table` still paints its rows inside TabPanel's `overflow_y_scroll` +
`.cached()` wrapper:

```rust
#[gpui::test]
#[serial]
fn probe_pane_results_render_inside_the_bottom_dock(cx: &mut TestAppContext) {
    let _h = enter_async_harness(cx);
    let (shell, vcx) = boot(cx);
    let console = vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.open_console_ready_for_test(/* window */, cx))
    });
    // Bind a synthetic pane source, settle, then assert the rows are captured.
    let snap = A11ySnapshot::capture(vcx, || settle(vcx));
    assert!(
        snap.labels().iter().any(|l| l.contains("row")),
        "pane results vanished inside the dock's scroll wrapper"
    );
}
```

Record whether the rows survive. If they do not, the design's §3 "accepted
cost" becomes a blocker and `DockItem::panel` must be reconsidered despite its
broken collapsed state — **stop and report** rather than proceeding to T1.

- [ ] **Step 4: Measure (c) — whole-shell Tab order**

Walk the order with `simulate_keystrokes("tab")` from a neutral click, and
record the full sequence.

**B7's rule is mandatory here:** a Tab-walk probe is meaningless unless the
reference point outside the tab group is itself a registered tab stop. Two
consecutive B7 probes "passed" while measuring nothing, and the more convincing
one was `tab_node_for_focus_id → None → next(None)` restarting from the
beginning. The activity rail is a registered stop outside the DockArea — use it
as the reference, and assert the walk *returns* to it.

Record the sequence verbatim. T5 reconciles the nav suites against it.

- [ ] **Step 5: Measure (d) — neither upstream toggle path desyncs**

For each of the two paths dat0 does not own — the title-bar chevron and
click-a-tab-while-collapsed — record:

1. `dock.read(app).is_dock_open(DockPlacement::Bottom, app)` before and after.
2. Whether a subsequent `toggle_dock` moves in the **correct** direction.

Expected per design §2.4: `set_open` assigns `self.open` synchronously and
defers only `set_collapsed`, so the read immediately after is the new value.
If it is not, §4's derived getter is unsound and needs a settle frame — record
that and adjust T3.

- [ ] **Step 6: Revert the probe, keep the findings**

```bash
git checkout -- src/window.rs
rm -f crates/dat0-app/tests/bottom_dock.rs
touch crates/dat0-app/src/window.rs
```

The `touch` is not optional — a revert that backwards-dates the file makes
cargo reuse the stale binary and report a false result (A6).

- [ ] **Step 7: Verify the tree is clean and green**

Run: `cargo test -p dat0-app --features a11y-capture --test a11y_spike`
Expected: PASS at 12, unchanged.

Run: `git status --short`
Expected: only the design doc is modified.

- [ ] **Step 8: Record the findings in the design doc**

Add a `## 14. As-built — T0 findings` section with the four measured values,
each stated as a number or a verbatim sequence, not a paraphrase. Explicitly
note any place the measurement **contradicted** the design, and amend the
design section it contradicts in the same commit.

- [ ] **Step 9: Commit**

```bash
git add docs/plans/2026-08-04-dat0-ui-redesign-b8-bottom-dock-design.md
git commit -s -F - <<'EOF'
docs(theme): B8 T0 — bottom-dock mechanics measured

Probe wired a throwaway bottom dock, took the four gate measurements, and
was reverted. Findings recorded in the design doc; see section 14.
EOF
```

---

## Task 1: `Panel` impls on `SqlConsole`

Additive only. Nothing mounts the console in a dock yet, so this task changes
no behaviour and no rendered output.

**Files:**
- Create: `crates/dat0-app/src/panels/sql_console_panel.rs`
- Modify: `crates/dat0-app/src/panels/mod.rs`
- Modify: `crates/dat0-i18n/src/strings/en.json`

**Interfaces:**
- Consumes: `SqlConsole` from `crate::view::sql_console` (fields `tabs: Vec<ConsoleTab>`,
  `active: usize`; `ConsoleTab.input: Entity<InputState>` — both `pub`).
- Produces, relied on by T3 and T4:
  - `SqlConsole::PANEL_NAME: &str` = `"SqlConsolePanel"`
  - `impl Focusable for SqlConsole` → `focus_handle(&self, cx: &App) -> FocusHandle`
  - `impl Panel for SqlConsole` → `panel_name`, `title`, `zoomable`
  - `impl EventEmitter<PanelEvent> for SqlConsole`

- [ ] **Step 1: Write the panel file**

Create `crates/dat0-app/src/panels/sql_console_panel.rs`:

```rust
//! B8: the SQL console's `Panel` impls — the bottom dock.
//!
//! Unlike every sibling in this module there is **no wrapper entity**. B5-B7
//! wrap because the grid, inspector, charts, catalog, connections and AI
//! bodies are render fns ON the shell; the console is already a standalone
//! entity owning its own state, so a wrapper would add a hop and force
//! `focus_handle` to walk shell → console → active tab. `Panel` is a foreign
//! trait but `SqlConsole` is a local type, so these impls are legal here and
//! stay beside their siblings.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _,
    SharedString, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::view::sql_console::SqlConsole;

impl SqlConsole {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B8 onward** — upstream's `Panel`
    /// docs say a panel name must not change once defined.
    pub const PANEL_NAME: &str = "SqlConsolePanel";
}

impl EventEmitter<PanelEvent> for SqlConsole {}

impl Focusable for SqlConsole {
    /// The ACTIVE TAB'S EDITOR handle, so a `window.focus(panel)` from dock
    /// code lands where the user types.
    ///
    /// Never a private handle: one minted here would be tracked by no element,
    /// and focusing it silently SWALLOWS focus rather than moving it (B5).
    /// Index-guarded because `active` is a plain `usize` — the console
    /// maintains at least one tab, but a panic in a `&App` accessor called
    /// every frame is not worth the assumption.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.tabs
            .get(self.active)
            .or_else(|| self.tabs.first())
            .map(|tab| tab.input.read(cx).focus_handle(cx))
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for SqlConsole {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// PLAIN text, deliberately WITHOUT an `a11y_label` — same reasoning as
    /// `CatalogPanel::title`. A dynamic title carrying the active tab's name
    /// was rejected at brainstorm: tab titles are user-editable, and a user
    /// naming a tab to match another node makes
    /// `A11ySnapshot::query_by_role` PANIC on a duplicate match
    /// (`tests/support/mod.rs:139`), taking whole suites down rather than
    /// failing one assertion.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(SharedString::from(dat0_i18n::t("sql.console.title")))
    }

    /// v1 dock scope is resize + collapse only.
    ///
    /// As `CatalogPanel` records, this does NOT remove the ⋯ button —
    /// `tab_panel.rs:483` renders it unconditionally — it only makes that
    /// menu's "Zoom In" row disabled.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    // `closable` is deliberately NOT overridden. `TabPanel::closable`
    // (`tab_panel.rs:100-113`) short-circuits on `!self.draggable(cx)`, and
    // `draggable = !is_locked(cx) && !is_last_panel(cx)`; dat0 calls
    // `dock.set_locked(true, ..)` at DockArea construction, so the lock has
    // been suppressing the close button for all five B6/B7 panels already.
    // That dependency is load-bearing and appears nowhere else in dat0 — the
    // test below pins it.

    // `visible` is deliberately NOT overridden either: the dock's own
    // open/closed state IS the console's visibility (design §4). Returning
    // `false` would blank the title bar's contents while `Dock::render` still
    // reserves its height, which is strictly worse than showing the bar.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key — a rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        assert_eq!(SqlConsole::PANEL_NAME, "SqlConsolePanel");
    }
}
```

- [ ] **Step 2: Add the i18n key**

In `crates/dat0-i18n/src/strings/en.json`, beside the other `sql.*` keys, add:

```json
  "sql.console.title": "SQL Console",
```

⚠ **JSON silently overwrites duplicate keys** (A5). `sql.console_toggle`
already exists and is a *different* key (the menu item's label) — confirm
`sql.console.title` is genuinely absent first:

Run: `grep -c '"sql.console.title"' crates/dat0-i18n/src/strings/en.json`
Expected: `0` before the edit, `1` after.

- [ ] **Step 3: Declare and register the panel**

In `crates/dat0-app/src/panels/mod.rs`, add to the module list:

```rust
pub mod sql_console_panel;
```

and append inside `register_panels`:

```rust
    // B8: the bottom dock. Degraded builder, same contract as the six above —
    // it hands back a console over ZERO persisted tabs and its own fresh
    // autocomplete snapshot rather than panicking. B9 replaces all seven with
    // builders that resolve the live shell and the real session.
    gpui_component::dock::register_panel(
        cx,
        crate::view::sql_console::SqlConsole::PANEL_NAME,
        |_dock_area, _state, _info, window, cx| {
            Box::new(cx.new(|cx| {
                crate::view::sql_console::SqlConsole::new(
                    &[],
                    0,
                    crate::query::completion::new_shared_snapshot(),
                    window,
                    cx,
                )
            }))
        },
    );
```

- [ ] **Step 4: Add the closable-resolution pin**

The dock lock is what suppresses the ✕. Pin it where the lock is set, in
`tests/dock_chrome_spike.rs` (which already builds a locked `DockArea` at
`:229`), so the assertion sits next to its subject:

```rust
/// B8: dat0 never overrides `Panel::closable`, which upstream defaults to
/// `true`. The ✕ is suppressed only because `dock.set_locked(true, ..)` makes
/// `TabPanel::draggable` false, which short-circuits `TabPanel::closable`
/// (`tab_panel.rs:100-113`). Nothing else in dat0 mentions `closable`, so this
/// pins the dependency: if a future slice drops the lock, six docked panels
/// silently grow a button that removes them from the dock with no way back.
#[gpui::test]
#[serial]
fn a_locked_dock_suppresses_the_panel_close_button(cx: &mut TestAppContext) {
    // Build the locked DockArea exactly as `:229` does, mount a single-panel
    // tab, and assert the resolved closable is false.
}
```

Fill in the body against the existing harness in that file.

- [ ] **Step 5: Prove the pin is non-vacuous**

Temporarily flip `set_locked(true, ..)` to `set_locked(false, ..)` in the test's
own harness and confirm the new assertion goes **RED**. Then restore it,
`touch` the file, and re-run.

Run: `cargo test -p dat0-app --test dock_chrome_spike`
Expected: RED with the lock off, PASS with it back on.

- [ ] **Step 6: Full local gate**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app > /tmp/b8-t1-plain.txt 2>&1; grep -c "test result: ok" /tmp/b8-t1-plain.txt
cargo test -p dat0-app --features a11y-capture > /tmp/b8-t1-a11y.txt 2>&1; grep -c "test result: ok" /tmp/b8-t1-a11y.txt
```

Expected: clippy exit 0; **115** in each count, 0 failures. The suite does not
grow yet — T4 adds the new binary.

⚠ **Do not pipe cargo's output through `head`** — it SIGPIPEs cargo mid-write
and truncates the count (A6: counted 51 instead of 109). Redirect to a file.

- [ ] **Step 7: Commit**

```bash
git add crates/dat0-app/src/panels/ crates/dat0-i18n/src/strings/en.json crates/dat0-app/tests/dock_chrome_spike.rs
git commit -s -F - <<'EOF'
feat(theme): B8 T1 — Panel impls for the SQL console

Adds Panel, Focusable and EventEmitter<PanelEvent> for SqlConsole directly,
with no wrapper entity: the console already owns its state, so a wrapper
would only add a hop.

Focusable returns the active tab's editor handle, never a private one — a
handle tracked by no element swallows focus instead of moving it.

Also pins the closable dependency. Upstream defaults Panel::closable to
true; the close button is suppressed only by the dock lock, and nothing
else in the tree records that.

Nothing mounts the console in a dock yet, so no behaviour changes.
EOF
```

---

## Task 2: Extract `ensure_dock_area`

Pure refactor. Zero behaviour change, zero rendered-output change.

**Files:**
- Modify: `crates/dat0-app/src/window.rs:7114-7242`

**Interfaces:**
- Consumes: T1's registrations (not directly — this task is independent of T1
  and may be reordered before it).
- Produces, relied on by T3:
  `fn ensure_dock_area(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<DockArea>`

- [ ] **Step 1: Move the block into a method**

Cut `window.rs:7114-7242` — the whole `if self.dock_area.is_none() { … }`
block, including every comment — and paste it verbatim into a new method
placed next to `sync_left_dock`/`sync_right_dock`:

```rust
    /// Lazily build the `DockArea` and everything mounted at construction
    /// time, returning it.
    ///
    /// Extracted from `render` at B8 because `toggle_sql_console` also needs a
    /// dock: it mounts the bottom dock on the console's first open, and it
    /// cannot rely on `render` having run first without making a
    /// toggle-before-first-draw silently no-op.
    ///
    /// ⚠ This runs with the shell LEASED (both callers hold `&mut self`), so
    /// B7's constraint applies in full: a `DockItem::tabs` of more than one
    /// panel CANNOT be built here — every `add_panel` after the first runs
    /// `set_active_ix`, which reaches `Panel::visible` → `shell.read` and
    /// panics. Single-panel `DockItem::tab` is immune because
    /// `set_active_ix(0)` early-returns on an unchanged index
    /// (`tab_panel.rs:208-211`). The bottom dock added by
    /// `toggle_sql_console` holds exactly one panel for that reason.
    fn ensure_dock_area(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<gpui_component::dock::DockArea> {
        if self.dock_area.is_none() {
            // … the moved block, verbatim …
        }
        self.dock_area.clone().expect("built above")
    }
```

- [ ] **Step 2: Call it from `render`**

Replace the excised block at its original site with:

```rust
        self.ensure_dock_area(window, cx);
```

Leave `sync_left_dock` / `sync_right_dock` immediately after it, unchanged.

- [ ] **Step 3: Verify the move is verbatim**

Run: `git diff --stat crates/dat0-app/src/window.rs`

Expected: roughly equal insertions and deletions (the moved block plus the new
signature, doc comment and call site). If insertions substantially exceed
deletions, something was rewritten rather than moved — re-check.

- [ ] **Step 4: Full local gate**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app --features a11y-capture > /tmp/b8-t2.txt 2>&1; grep -c "test result: ok" /tmp/b8-t2.txt
grep -c "test result: FAILED" /tmp/b8-t2.txt
```

Expected: clippy exit 0; **115** ok, **0** FAILED. A pure refactor that moves
dock construction must leave every dock test untouched — any red here is a
real behaviour change hiding in a "move".

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src/window.rs
git commit -s -F - <<'EOF'
refactor(theme): B8 T2 — extract ensure_dock_area from render

Moves the lazy DockArea construction out of WorkspaceShell::render into a
method both render and toggle_sql_console can call. Verbatim move; no
behaviour change.

toggle_sql_console needs this at T3 to mount the bottom dock on the
console's first open. Without it a toggle before the first draw would
silently no-op, which is a trap for test authors even though production
cannot reach it.
EOF
```

---

## Task 3: The migration

The behaviour change. Console leaves the shell's layout column; visibility
becomes derived.

**Files:**
- Modify: `crates/dat0-app/src/window.rs` — field `:2325`, init `:2553`,
  `toggle_sql_console` `:3080-3153`, the strip `:7497-7512`, its placement
  `:7690`, and the readers at `:7850` and `:7953`
- Modify: `crates/dat0-app/tests/a11y_spike.rs:126-127`

**Interfaces:**
- Consumes: T1's `SqlConsole::PANEL_NAME` and `Panel` impls; T2's
  `ensure_dock_area`; T0's measured node count and Tab order.
- Produces, relied on by T4 and T5:
  `pub(crate) fn sql_console_visible(&self, cx: &App) -> bool`

- [ ] **Step 1: Delete the field and add the getter**

Remove `pub(crate) sql_console_visible: bool` (`:2325`, with its doc comment)
and its initializer (`:2553`). Add, next to `inspector_visible()` /
`chart_visible()`:

```rust
    /// Whether the SQL console is showing — **derived from the dock**, never a
    /// parallel bool.
    ///
    /// Upstream owns two toggle paths dat0 does not: the title-bar chevron
    /// (`tab_panel.rs:616`) and clicking a tab while the bottom dock is
    /// collapsed (`tab_panel.rs:740-752`). Either flips `Dock::open` without
    /// dat0's knowledge, so a cached bool would desync and the next
    /// `SqlConsoleToggle` would move BACKWARDS. Making the dock the single
    /// source of truth removes the class rather than patching it.
    ///
    /// `is_dock_open` returns false while `bottom_dock` is `None`, so the
    /// pre-mount state needs no special case.
    pub(crate) fn sql_console_visible(&self, cx: &gpui::App) -> bool {
        self.dock_area.as_ref().is_some_and(|d| {
            d.read(cx)
                .is_dock_open(gpui_component::dock::DockPlacement::Bottom, cx)
        })
    }
```

- [ ] **Step 2: Rewrite `toggle_sql_console`**

Keep the console-construction body (persisted tabs, shared snapshot,
`subscribe_in`, `ai_ready` hydration, the `on_window_should_close` persist
backstop) **exactly as it is**. Change only what surrounds it:

```rust
    pub(crate) fn toggle_sql_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock = self.ensure_dock_area(window, cx);

        if self.sql_console.is_none() {
            // … existing construction body, unchanged, minus the line
            //     self.sql_console_visible = true;
            // … through the on_window_should_close hook …

            // B8: mount the bottom dock. ⚠ `set_bottom_dock` runs
            // `subscribe_item`, which pushes onto the DockArea's
            // `_subscriptions` and recurses over the item tree
            // (`dock/mod.rs:955-963`); nothing ever removes them. Called
            // EXACTLY ONCE, here — every later open/close is `toggle_dock`.
            //
            // Lazy rather than alongside the left and right docks so a user
            // who never opens the console never sees the residual 29px bar
            // upstream keeps for a collapsed bottom dock (`dock.rs:378`).
            let weak_dock = dock.downgrade();
            let item = gpui_component::dock::DockItem::tab(
                console.clone(),
                &weak_dock,
                window,
                cx,
            );
            dock.update(cx, |dock, cx| {
                dock.set_bottom_dock(item, Some(gpui::px(320.)), true, window, cx);
            });
        } else {
            dock.update(cx, |dock, cx| {
                dock.toggle_dock(gpui_component::dock::DockPlacement::Bottom, window, cx);
            });
        }

        // Refresh the autocomplete schema whenever the console is (re)shown so
        // tables created or dropped while it was hidden are reflected (P5b T2).
        // Safe to read straight after the toggle: `Dock::set_open` assigns
        // `self.open` synchronously and defers only `set_collapsed`
        // (`dock.rs:259-266`).
        if self.sql_console_visible(cx) {
            self.refresh_completion_snapshot(cx);
        }
        if self.catalog_panel_visible {
            self.refresh_catalog(cx);
        }
        cx.notify();
    }
```

- [ ] **Step 3: Delete the 260 px strip**

Remove the `sql_console_panel` binding (`:7497-7512`) entirely, and remove
`.children(sql_console_panel)` from the shell root's child list (`:7690`).

- [ ] **Step 4: Fix the two remaining readers**

`open_console_with_timing_for_test` (`:7850`) and `open_console_for_test`
(`:7953`) both do `if !self.sql_console_visible { self.toggle_sql_console(..) }`.
Both have a `cx`, so both become `if !self.sql_console_visible(cx) { … }`.

- [ ] **Step 5: Compile and let the compiler enumerate the rest**

Run: `cargo build -p dat0-app`

Expected: any reader of the deleted field is now a hard error. Fix each by
threading `cx`. **Change the signature first and let the compiler find the call
sites** — a grep would have missed some and cannot prove completeness (A6:
the master plan guessed ~15 `focus_stop` sites, reality was 29).

- [ ] **Step 6: Bump the `a11y_spike` count**

Run: `cargo test -p dat0-app --features a11y-capture --test a11y_spike`
Expected: FAIL at the exact-count assertion, reporting T0 Step 2's number.

Update `tests/a11y_spike.rs:126-127` to that number, with a comment naming the
contributing nodes:

```rust
    // B8 took this from 12 to N: the bottom dock's TabPanel title bar
    // contributes <name the exact nodes measured at T0>.
    //
    // ⚠ This is an EXACT count on purpose — it is the frame-bracket
    // DOUBLE-RENDER proof, not a content check. Loosening it to `>=` would
    // destroy the proof (B3).
    assert_eq!(snap.click_ids.len(), N, …);
```

If the observed number differs from T0's measurement, **stop and report** —
something changed between the probe and the real mount.

- [ ] **Step 7: Full local gate**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app --features a11y-capture > /tmp/b8-t3.txt 2>&1
grep -c "test result: ok" /tmp/b8-t3.txt; grep -c "test result: FAILED" /tmp/b8-t3.txt
cargo test -p dat0-app --test style_lint
git diff --stat -- crates/dat0-app/src/grid crates/dat0-app/src/session
```

Expected: clippy exit 0; style_lint 4/4 with the ratchet unchanged; `git diff`
**empty** for `src/grid` and `src/session`.

Nav-suite failures **are expected here** and are T5's job — record which
binaries went red and why. Do not paper over them in this task.

- [ ] **Step 8: Commit**

```bash
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/a11y_spike.rs
git commit -s -F - <<'EOF'
feat(theme): B8 T3 — SQL console moves into the bottom dock

Deletes the fixed 260px strip the shell mounted above the grid and mounts
the console as a Panel in DockPlacement::Bottom at 320px, resizable.

Visibility is now derived from DockArea::is_dock_open rather than a shell
bool. Upstream owns two toggle paths dat0 does not — the title-bar chevron
and clicking a tab while collapsed — and a cached bool would desync on
either, making the next toggle move backwards.

set_bottom_dock is called exactly once, on the console's first open; it
leaks subscriptions the same way set_left_dock and set_right_dock do.
Mounting it lazily also keeps the residual collapsed bar off the screen
for a user who never opens the console.
EOF
```

---

## Task 4: `tests/bottom_dock.rs`

**Files:**
- Create: `crates/dat0-app/tests/bottom_dock.rs`

**Interfaces:**
- Consumes: T3's `sql_console_visible(cx)`; T1's `SqlConsole::PANEL_NAME`.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Copy the harness preamble**

Copy `tests/left_dock.rs:1-115` — `mod support`, `set_config_dir`,
`init_components`, `AsyncHarness` / `enter_async_harness`,
`build_empty_session`, `open_shell_window`, `boot`, `settle` — adjusting the
module doc to B8. This per-binary duplication is the established precedent
(`left_dock.rs` itself copied it from `ai_nav.rs`).

Keep `enter_async_harness`: `toggle_sql_console` reaches
`refresh_completion_snapshot`, which spawns off-thread work, and without an
ambient runtime that panics with "there is no reactor running".

- [ ] **Step 2: Write the tests**

Assert through the **dock's own open flag and the a11y capture**, never by
re-reading a value the test just wrote — that would prove only that assignment
works (B6's rule).

```rust
/// The console is absent from the tree until first opened, and so is the
/// bottom dock's residual bar — which is the entire reason the dock is
/// mounted lazily rather than beside the left and right docks.
#[gpui::test]
#[serial]
fn no_bottom_dock_before_the_console_is_ever_opened(cx: &mut TestAppContext) { … }

/// First toggle builds the console AND mounts the dock open.
#[gpui::test]
#[serial]
fn first_toggle_mounts_the_bottom_dock_open(cx: &mut TestAppContext) { … }

/// Second toggle closes it — and the derived getter follows the DOCK, not a
/// bool the test wrote.
#[gpui::test]
#[serial]
fn second_toggle_closes_the_dock_and_visibility_follows(cx: &mut TestAppContext) { … }

/// set_bottom_dock is called exactly once no matter how many times the
/// console is toggled. It leaks subscriptions per call (dock/mod.rs:955-963),
/// so a regression here is unbounded growth, not a wrong pixel.
#[gpui::test]
#[serial]
fn repeated_toggles_never_remount_the_dock(cx: &mut TestAppContext) { … }

/// Upstream toggle path 1: the title-bar chevron. Drives the real element,
/// then asserts a SUBSEQUENT Cmd+Shift+C moves in the correct direction —
/// the reversal is the failure mode a cached bool would have.
#[gpui::test]
#[serial]
fn the_chevron_does_not_reverse_the_next_keyboard_toggle(cx: &mut TestAppContext) { … }

/// Upstream toggle path 2: clicking a tab while collapsed reopens the dock
/// (tab_panel.rs:740-752).
#[gpui::test]
#[serial]
fn clicking_the_collapsed_title_bar_reopens_the_console(cx: &mut TestAppContext) { … }

/// The panel's accessible name is the static title, and it does NOT collide
/// with any SQL tab name — query_by_role panics on a duplicate match.
#[gpui::test]
#[serial]
fn the_title_bar_names_the_panel_exactly_once(cx: &mut TestAppContext) { … }
```

Fill in each body against the harness. Use `simulate_keystrokes("cmd-shift-c")`
for the keyboard path, **not** `dispatch_action` — the keymap must be exercised
or a dead production key path stays hidden.

- [ ] **Step 3: Run them**

Run: `cargo test -p dat0-app --features a11y-capture --test bottom_dock`
Expected: all PASS.

- [ ] **Step 4: Prove non-vacuity**

For each test, perturb the thing it claims to measure and confirm it goes RED —
then restore, `touch`, and re-run. Specifically: flip the expected open state,
and rename the title key so the name assertion has nothing to find.

B7 found a test that stayed green under its own non-vacuity probe while four
siblings went red; the finding was worth more than the test. If one here is
vacuous, **record why in its doc comment** rather than deleting or trusting it.

- [ ] **Step 5: Confirm the suite grew by exactly one**

```bash
cargo test -p dat0-app --features a11y-capture > /tmp/b8-t4.txt 2>&1
grep -c "test result: ok" /tmp/b8-t4.txt
```

Expected: **116** (was 115).

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/tests/bottom_dock.rs
git commit -s -F - <<'EOF'
test(theme): B8 T4 — bottom dock suite

Covers lazy mount, derived visibility, the exactly-once set_bottom_dock
contract, and both upstream toggle paths dat0 does not own — including the
reversal a cached visibility bool would have produced.

Asserts through the dock's own open flag and the a11y capture rather than
re-reading values the tests wrote.
EOF
```

---

## Task 5: Nav-suite reconciliation and the nested-scroll proof

**Files:**
- Modify: `crates/dat0-app/tests/sql_console_nav.rs`,
  `tests/sql_console_transient_nav.rs`, `tests/input_nav.rs`,
  `tests/keyboard_nav.rs`, `tests/a11y_content.rs` — **only where the move
  genuinely changes them**
- Modify: `crates/dat0-app/tests/bottom_dock.rs` — add the nested-scroll test

**Interfaces:**
- Consumes: T0 Step 3's nested-scroll measurement and T0 Step 4's recorded Tab
  order; T3's derived getter.
- Produces: nothing.

- [ ] **Step 1: Run the console suites and triage**

```bash
for t in sql_console_nav sql_console_transient_nav input_nav keyboard_nav a11y_content; do
  cargo test -p dat0-app --features a11y-capture --test $t > /tmp/b8-t5-$t.txt 2>&1
  echo "$t: $(grep -c 'test result: FAILED' /tmp/b8-t5-$t.txt) failed"
done
```

For **each** failure, classify before touching it:

- **Intended** — the console's stops moved from before the rail to after the
  grid center (design §7). Update the asserted order to match T0 Step 4's
  recorded sequence.
- **Regression** — anything else. A stop that vanished, changed name, stopped
  being reachable, or reordered *within* the console is a real defect. Fix the
  production code, not the test.

Write the classification down per failing test. The standing rule from B7 is
that a docked panel's stops stay reachable in document order, so "the test
needed updating" is a claim that must be argued, not assumed.

- [ ] **Step 2: Add the nested-scroll test**

Promote T0 Step 3's throwaway into a real test in `tests/bottom_dock.rs`:

```rust
/// A `ResultTarget::Pane` run renders its results grid INSIDE the dock.
///
/// TabPanel wraps the active panel in `overflow_y_scroll()` + `.cached(..)`
/// (`tab_panel.rs:850-861`), and the console's results grid is a virtualized
/// `Table` with its own viewport — the exact nesting B5 avoided for the
/// center by mounting it as `DockItem::Panel`. This is the standing guard
/// that the nesting stays benign.
#[gpui::test]
#[serial]
fn pane_results_still_render_inside_the_dock(cx: &mut TestAppContext) { … }
```

- [ ] **Step 3: Prove it non-vacuous**

Perturb the needle so the assertion has nothing to match, confirm RED, restore,
`touch`, re-run.

- [ ] **Step 4: Full local gate, all three feature combos**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
for f in "" "--features a11y-capture" "--features a11y-capture,gallery"; do
  cargo test -p dat0-app $f > /tmp/b8-t5-combo.txt 2>&1
  echo "$f -> ok:$(grep -c 'test result: ok' /tmp/b8-t5-combo.txt) failed:$(grep -c 'test result: FAILED' /tmp/b8-t5-combo.txt)"
done
cargo test -p dat0-app --test style_lint
git diff --stat -- crates/dat0-app/src/grid crates/dat0-app/src/session
```

Expected: **116 ok / 0 failed** in each combo; style_lint 4/4 with the ratchet
at `[("window.rs", 1)]`; `git diff` empty for `src/grid` and `src/session`.

- [ ] **Step 5: Build and BOOT the binary**

```bash
cargo build -p dat0-app --bin dat0
DAT0_CONFIG_DIR=/tmp/dat0-b8-boot ./target/debug/dat0
```

Then build `main` in a clean tree and boot it the same way, and **diff the two
logs**. This is how B5's silent first-run-tour regression was found — not by
any test and not by reading the diff. A silent success logs nothing, so "no
line on main, a WARN on the branch" is the entire signal.

⚠ Do **not** bracket the checkout in `git stash` / `git stash pop`: a stash on
an already-clean tree stashes nothing, and the later pop restores an unrelated
pre-existing stash (this bit B7). Commit first, then `git checkout main`.

Exercise: hero → Open demo → ⌘⇧C to open the console → run a query → collapse
via the chevron → reopen by clicking the title bar → ⌘⇧C twice.

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/tests/
git commit -s -F - <<'EOF'
test(theme): B8 T5 — nav suites reconciled, nested scroll guarded

The console's focus stops move from before the activity rail to after the
grid center, which is the intended consequence of putting the console below
the grid. Suites updated to the measured order; every other movement was
treated as a defect rather than a test to update.

Adds a standing guard that a Pane-target result grid still renders inside
TabPanel's scroll-and-cache wrapper.
EOF
```

---

## Task 6: As-built and whole-branch review

**Files:**
- Modify: `docs/plans/2026-08-04-dat0-ui-redesign-b8-bottom-dock-design.md` §14

- [ ] **Step 1: Whole-branch review**

Read `git diff main...HEAD` in one pass. Per-task review structurally cannot
catch cross-cutting defects — this has caught something in every slice of the
series (B3's "1 cells selected", B7's dead Escape ladder, A4's two credibility
gaps in the gate itself). Look specifically for:

- Any remaining reader of the deleted `sql_console_visible` field semantics
  that now means something subtly different.
- Whether every `toggle_sql_console` entry point (menu, ⌘⇧C, command palette
  `ids::CONSOLE_TOGGLE` at `window.rs:5228`) still reaches the dock.
- Whether the console's Escape ladder still fires from inside the dock.

- [ ] **Step 2: Write §14 as-built**

Record every deviation from the design with its reason, the final `a11y_spike`
number, the measured Tab order, and anything a future slice must not
rediscover.

- [ ] **Step 3: Commit and open the PR**

```bash
git add docs/plans/
git commit -s -F - <<'EOF'
docs(theme): B8 as-built
EOF
git push -u origin feat/ui-redesign-b8-bottom-dock
gh pr create --title "feat(theme): B8 — SQL console into the bottom dock (UI redesign)" --body-file <(…)
```

Poll with `gh pr checks`, **not** `gh run watch`. On merge, squash with
explicit `--subject` and `--body-file` so no commit-message text leaks a
CI-skip marker, then **watch the post-merge main run**: the macOS grid-scroll
bench is push-to-main-only and can redden main silently. Verify it at **step**
level (reclaim → bench → upload all success) and `gh run download` the
artifact — and per B5's ruling, read no meaning into the number.

---

## Self-Review

**Spec coverage.** Design §3 mount shape → T0/T3. §4 derived visibility → T3
Steps 1-2, T4. §5 `ensure_dock_area` → T2. §6 panel impls, every table row →
T1 (`closable` row → T1 Steps 4-5). §7 layout and traversal → T3 Step 3, T5
Step 1. §8 all four T0 probes → T0 Steps 2-5, with (b) promoted to a standing
test at T5 Step 2. §9 tests → T4, T5. §10 local gate → T5 Step 4, plus the
boot diff at Step 5. §11 risks → recorded; the narrow-window row is glance-only
by decision. §13 owed glance → carried to memory at merge, not a task.

**Placeholder scan.** T4 Step 2 and T5 Step 2 give test names, doc comments and
exact assertions but leave bodies to be filled against a harness that is copied
verbatim in T4 Step 1 — that is a fill-in-the-harness instruction, not a TBD.
T0's steps are measurements whose values do not exist until run; each names the
exact command, the expected outcome, and what to do if it differs.

**Type consistency.** `SqlConsole::PANEL_NAME` (T1) is the same symbol used in
`register_panels` (T1 Step 3) and asserted in T1's test.
`sql_console_visible(&self, cx: &App) -> bool` (T3 Step 1) is the signature used
at T3 Steps 2 and 4 and in T4. `ensure_dock_area(&mut self, window, cx) ->
Entity<DockArea>` (T2) is called as such in T3 Step 2.
`DockItem::tab(panel, &weak_dock, window, cx)` matches the existing B6/B7 call
sites at `window.rs:7144` and `:7201`.
