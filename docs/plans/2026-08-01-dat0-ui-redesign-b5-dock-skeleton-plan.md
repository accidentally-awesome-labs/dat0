# dat0 UI Redesign — Slice B5: DockArea skeleton (implementation plan)

> **For agentic workers:** steps use checkbox (`- [ ]`) syntax for tracking. This
> slice is executed INLINE by the controller (the series has run without
> subagents since A5, and this session is instructed not to dispatch agents).

**Goal:** Move the shell's grid center onto `gpui_component::dock::DockArea`
holding a single `GridPanel`, with no visible change and no grid-behaviour
change.

**Architecture:** One structural edit in `WorkspaceShell::render` —
`.child(div().flex_1().child(body))` becomes
`.child(div().flex_1().children(dock_area))`. The `body` match moves verbatim
into `WorkspaceShell::render_grid_body`, which a thin `GridPanel` (one field: a
`WeakEntity<WorkspaceShell>`) calls back into. The center mounts as
`DockItem::Panel`, which renders the panel's raw `AnyView` with no chrome, no
scroll wrapper, no cached element and no tab group.

**Tech Stack:** Rust 2024, gpui 0.2.2, gpui-component pinned rev `0f0ab35`,
criterion (CI-only), kittest/accesskit under `--features a11y-capture`.

**Design:** `docs/plans/2026-08-01-dat0-ui-redesign-b5-dock-skeleton-design.md`
(commit `85fe14a`). Read §2.1, §3.2 and §5 before starting.

## Global Constraints

- Branch `feat/ui-redesign-b5-dock-skeleton` off main `f389dc0`. One commit per task.
- gpui-component rev stays `0f0ab35`. **Do not bump it.** Every API fact below was read from that checkout.
- `crates/dat0-app/src/grid/**` is **byte-untouched** by this slice. Same for `benches/grid_scroll.rs`'s code (its module doc changes in T4).
- Session schema stays v10. No migration, no `dock_layout` (B9).
- Zero new test binaries — coverage lands in existing suites. Suite count stays **112**.
- `tests/style_lint.rs` ratchet stays exactly `[("window.rs", 1)]`. Read theme tokens, never colour literals. **The scanner matches banned colour names in prose too** — do not spell a banned constructor with call parens in a comment.
- No new i18n keys: nothing in this slice is user-visible.
- Keyboard behaviour is driven in tests with `simulate_keystrokes`, never `dispatch_action`.
- `GridPanel::PANEL_NAME` is `"GridPanel"` and is frozen from this slice onward (B9 serialization key).
- Local gate (`cargo test --workspace` and `cargo bench` are unrunnable on this machine — macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift, reproduces on `main`):
  ```
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test -p dat0-app
  cargo test -p dat0-app --features a11y-capture
  cargo test -p dat0-app --features a11y-capture,gallery
  cargo build -p dat0-app --bin dat0
  ```
  Run the full six only in T5; per-task scopes are named in each task.

---

## File Structure

| file | responsibility |
|---|---|
| `crates/dat0-app/src/panels/mod.rs` | **new** — panels module root; `register_panels(cx)` |
| `crates/dat0-app/src/panels/grid_panel.rs` | **new** — `GridPanel`: `Panel` + `Render` + `Focusable` + `EventEmitter<PanelEvent>`; one `WeakEntity<WorkspaceShell>` field |
| `crates/dat0-app/src/lib.rs` | +1 line: `pub mod panels;` |
| `crates/dat0-app/src/window.rs` | `render_grid_body` extraction; `dock_area`/`grid_panel` fields; the one body-row edit; `grid_focus_handle` accessor; `dock_mounted_for_test` shim; `register_panels` in `run_app` |
| `crates/dat0-app/tests/a11y_content.rs` | dock-mount + grid-content-through-panel assertions |
| `crates/dat0-app/tests/keyboard_nav.rs` | `init_components` gains `register_panels`; Tab-reach/arrows unchanged |
| `crates/dat0-app/benches/grid_scroll.rs` | module doc: what this bench does **not** measure |

---

## Task 0: The three T0 gates

**Files:**
- Temporary probe: `crates/dat0-app/tests/b5_t0_probe.rs` (**deleted before the task's commit** — it must never reach the suite count)
- Commit: findings note appended to the design doc

**Interfaces:**
- Consumes: nothing.
- Produces: a go/no-go for Tasks 1–4, and (if gate 1 is red) the §3.2 snapshot fallback becomes the design.

**STOP clauses are armed. Do not start Task 1 with any gate red.**

- [ ] **Step 1: Write the probe — gate 1 (update-through re-entrancy)**

The question: may a child entity call `shell.update(...)` from inside its own
`render`, while the shell's render is what put it on screen? Reading is proven
(`grid/mod.rs:503-505`); updating is not, and B4 showed the failure mode
("cannot read WorkspaceShell while it is already being updated").

```rust
// crates/dat0-app/tests/b5_t0_probe.rs  — THROWAWAY
//
// Gate 1: does `shell.update()` from a descendant entity's render panic?
mod support;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, WeakEntity, Window, div,
};
use std::rc::Rc;
use std::cell::RefCell;

struct Parent {
    child: Option<Entity<Child>>,
    renders: usize,
}

struct Child {
    parent: WeakEntity<Parent>,
}

impl Render for Parent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.child.is_none() {
            let weak = cx.entity().downgrade();
            self.child = Some(cx.new(|_| Child { parent: weak }));
        }
        div().size_full().children(self.child.clone())
    }
}

impl Render for Child {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(parent) = self.parent.upgrade() else {
            return div();
        };
        // THE LINE UNDER TEST: mutate + read the parent from the child's render.
        parent.update(cx, |p, _cx| {
            p.renders += 1;
            div().child(format!("renders={}", p.renders))
        })
    }
}

#[gpui::test]
fn child_may_update_parent_from_its_own_render(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let slot: Rc<RefCell<Option<Entity<Parent>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |_window, cx| {
        let p = cx.new(|_| Parent { child: None, renders: 0 });
        *slot2.borrow_mut() = Some(p.clone());
        p
    });
    vcx.run_until_parked();
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    let parent = slot.borrow().clone().expect("parent");
    let n = cx.update(|cx| parent.read(cx).renders);
    assert!(n >= 1, "child never rendered through the parent update path");
}
```

- [ ] **Step 2: Run gate 1**

```
cargo test -p dat0-app --test b5_t0_probe -- --nocapture
```

Expected if GREEN: passes, no panic.
Expected if RED: a panic naming "already being updated" / "already borrowed".

**STOP if red** → the primary mechanism is dead. Amend the design doc §3.2 to
make the *snapshot fallback* the design (shell pushes `HeroHandles`,
`recents_empty`, `first_run_done`, `recents_active` into the panel via
`panel.update` in the shell's own render; panel builds elements from that plus
a weak read of `data_source`/`table_state`/`selection`), then rewrite Task 1
Step 5 and Task 2 accordingly before writing any production code.

- [ ] **Step 3: Add gate 3 to the probe (chrome absence)**

Gate 3 asserts what §2.1 read from the source: `DockItem::Panel` puts **nothing**
between the dock and the panel's own view. It is asserted rather than cited
because the whole no-visible-change premise rests on it.

```rust
// appended to crates/dat0-app/tests/b5_t0_probe.rs — THROWAWAY
use gpui_component::dock::{DockArea, DockItem, Panel, PanelEvent};
use gpui::{App, EventEmitter, FocusHandle, Focusable};
use std::sync::Arc;

struct Probe;
impl EventEmitter<PanelEvent> for Probe {}
impl Focusable for Probe {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        cx.focus_handle()
    }
}
impl Panel for Probe {
    fn panel_name(&self) -> &'static str {
        "B5Probe"
    }
}
impl Render for Probe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().id("b5-probe-body").size_full().child("probe")
    }
}

#[gpui::test]
fn dock_item_panel_adds_no_chrome(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, vcx) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|_| Probe);
        let dock = cx.new(|cx| {
            let mut d = DockArea::new("b5-probe", Some(1), window, cx);
            d.set_locked(true, window, cx);
            d
        });
        let item = DockItem::panel(Arc::new(panel));
        dock.update(cx, |d, cx| d.set_center(item, window, cx));
        dock
    });
    vcx.run_until_parked();
    // The probe body must be reachable; no TabPanel title bar means no
    // "Dock.Unnamed" title text and no zoom/menu toolbar button in the tree.
    let text = vcx.update(|_window, _cx| String::new());
    let _ = text; // geometry oracle below is the real assertion
    assert!(
        vcx.debug_bounds("b5-probe-body").is_some(),
        "panel body did not render under DockItem::Panel"
    );
}
```

If `debug_bounds` needs a `.debug_selector`, add `.debug_selector(|| "b5-probe-body".into())`
to the probe's `div` — that is what `a11y()` chains internally
(`src/a11y/mod.rs`), and `VisualTestContext::debug_bounds` resolves it.

- [ ] **Step 4: Run gate 3**

```
cargo test -p dat0-app --test b5_t0_probe -- --nocapture
```

Expected: PASS. **STOP if red** — the slice's premise is void; report the
`DockItem::Tabs` trade-off (30 px title bar) to the owner before writing code.

- [ ] **Step 5: Gate 2 — single-frame a11y capture, run against the WHOLE suite**

This is the most informative gate in the slice and it costs one command. B3's
mount gate fired on exactly one file (`tests/a11y_spike.rs`, which asserts an
EXACT captured-node count of 8 as a frame-bracket double-render proof) and
grepping for "what asserts on labels" would have missed it entirely. So: run
everything, not a chosen subset.

Temporarily mount the probe dock inside the real shell — the cheapest version
is to do Task 2's one-line body-row edit against a `DockItem::panel` holding a
`GridPanel` stub that renders `div()`, then:

```
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b5-t0-gate2.txt
grep -c "test result: ok" /tmp/b5-t0-gate2.txt
grep -n "FAILED\|panicked" /tmp/b5-t0-gate2.txt | head -40
```

Expected: 112 `test result: ok`, and `tests/a11y_spike.rs` still reading
exactly **8** nodes on the hero.

**STOP if the count doubles (16, or any multiple)** → the dock re-renders
children per forced frame. Build the pre-designed generation counter first
(`src/a11y/mod.rs:24`: bump on `begin_frame()`, keep only max-gen nodes) as its
own task, then resume.

⚠ Do NOT pipe cargo's output through `head` to count — `head` SIGPIPEs cargo
mid-write and truncates (A6 counted 51 binaries instead of 109 that way).
Redirect to a file and count in the file.

- [ ] **Step 6: Revert every probe edit and delete the probe**

```bash
git checkout -- crates/dat0-app/src/window.rs crates/dat0-app/src/lib.rs
rm -f crates/dat0-app/tests/b5_t0_probe.rs
touch crates/dat0-app/src/window.rs
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b5-t0-revert.txt
grep -c "test result: ok" /tmp/b5-t0-revert.txt
```

⚠ **`touch` after reverting is not optional.** A6 lost time to a correctly
reverted file whose mtime went backwards: cargo reused the stale binary and
reported a false red, and a commit was briefly made on that false signal.
Expected: 112 ok, tree identical to `85fe14a`.

- [ ] **Step 7: Record the findings and commit**

Append a `## 9. As-built: T0 gate findings (measured 2026-08-01)` section to the
design doc with, for each gate: what was run, the exact observed result, and
whether the design changed. Then:

```bash
git add docs/plans/2026-08-01-dat0-ui-redesign-b5-dock-skeleton-design.md
git commit -s -m "docs(theme): B5 T0 — dock gates measured"
```

---

## Task 1: `panels` module and `GridPanel`

**Files:**
- Create: `crates/dat0-app/src/panels/mod.rs`
- Create: `crates/dat0-app/src/panels/grid_panel.rs`
- Modify: `crates/dat0-app/src/lib.rs` (add `pub mod panels;` in alphabetical position, between `overlay` and `package`)
- Modify: `crates/dat0-app/src/window.rs` — extract `render_grid_body`, add `grid_focus_handle`
- Test: in-file `#[cfg(test)] mod tests` in `grid_panel.rs` (lib test binary — **no new test binary**)

**Interfaces:**
- Consumes: nothing from Task 0 but its verdict.
- Produces, for Task 2:
  - `crate::panels::grid_panel::GridPanel::new(shell: WeakEntity<WorkspaceShell>) -> GridPanel`
  - `crate::panels::grid_panel::GridPanel::PANEL_NAME: &str` (`"GridPanel"`)
  - `crate::panels::register_panels(cx: &mut App)`
  - `WorkspaceShell::render_grid_body(&mut self, cx: &mut Context<Self>) -> AnyElement`
  - `WorkspaceShell::grid_focus_handle(&self) -> FocusHandle` (`pub(crate)`)

- [ ] **Step 1: Write the failing test**

In `crates/dat0-app/src/panels/grid_panel.rs`, at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::dock::Panel as _;

    /// The panel name is B9's serialization key: `DockArea::load` resolves it
    /// through the global `PanelRegistry`, and upstream's trait docs say it
    /// must never change once defined. This is a rename ratchet, not a
    /// tautology — the string is load-bearing for a slice that has not been
    /// written yet.
    #[test]
    fn panel_name_is_frozen() {
        let panel = GridPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "GridPanel");
        assert_eq!(GridPanel::PANEL_NAME, "GridPanel");
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

```
cargo test -p dat0-app --lib panel_name_is_frozen
```

Expected: FAIL — `unresolved module` / `cannot find struct GridPanel`.

- [ ] **Step 3: Write `src/panels/mod.rs`**

```rust
//! Dock panels — `gpui_component::dock::Panel` implementors.
//!
//! B5 introduces the first one ([`grid_panel::GridPanel`]); B6-B8 add the
//! inspector, charts, catalog, connections, AI and SQL-console panels. They
//! live here rather than in `src/view/` because a `Panel` is a different kind
//! of thing from a free render fn: it is an entity with a stable `panel_name`
//! that `DockArea::load` resolves through a global registry (B9).

pub mod grid_panel;

use gpui::{AppContext as _, App};

/// Register every dat0 panel with gpui-component's global `PanelRegistry`.
///
/// Called from `run_app` AND from each test binary's `init_components`: a
/// registration performed only in production is silently absent under test
/// (the `register_modal_keys` lesson from B1/B2).
///
/// Nothing calls `DockArea::load` until B9, so the builder below is currently
/// unreachable. It returns a shell-less panel rather than panicking — a
/// `WeakEntity::new_invalid()` upgrade fails and `GridPanel::render` paints an
/// empty div, which is a graceful degradation instead of a landmine. B9
/// replaces it with a builder that resolves the live shell.
pub fn register_panels(cx: &mut App) {
    gpui_component::dock::register_panel(
        cx,
        grid_panel::GridPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| grid_panel::GridPanel::new(gpui::WeakEntity::new_invalid())))
        },
    );
}
```

- [ ] **Step 4: Write `src/panels/grid_panel.rs`**

```rust
//! B5: the DockArea center panel — a thin wrapper over the shell's grid body.
//!
//! The panel owns NO grid state. [`crate::window::WorkspaceShell`] keeps
//! `data_source`, `table_state`, `selection`, `recents_active`, the hero focus
//! handles, the root focus handle and the arrow-key handler; this panel's
//! `render` delegates straight back into `WorkspaceShell::render_grid_body`.
//!
//! Hero and focus-handle migration into the panel is deliberately deferred to
//! B7 (master plan §6, the declared focus-migration slice). Doing it here would
//! leave two suspects behind any red keyboard-nav result in the one slice whose
//! entire premise is that nothing changed.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    Styled as _, WeakEntity, Window, div,
};
use gpui_component::dock::{Panel, PanelEvent};

use crate::window::WorkspaceShell;

pub struct GridPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl GridPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B5 onward** — upstream's `Panel`
    /// docs say a panel name must not change once defined.
    pub const PANEL_NAME: &str = "GridPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for GridPanel {}

impl Focusable for GridPanel {
    /// Returns the SHELL's root focus handle, not one of our own.
    ///
    /// That handle is the grid's tab stop and the host of the arrow-key
    /// handler, so a `window.focus(panel)` from dock code lands on the real
    /// grid. A private handle would be tracked by no element, and focusing it
    /// would silently swallow focus rather than move it.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for GridPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }
}

impl Render for GridPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder, which
            // hands out a shell-less panel (see `panels::register_panels`).
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_grid_body(cx))
    }
}
```

- [ ] **Step 5: Extract `render_grid_body` from `WorkspaceShell::render`**

In `src/window.rs`, the `body` local is a three-arm match at **`:6650-6755`**
(`let body = match (self.data_source.as_ref(), self.table_state.as_ref()) { … };`).
Move the match body verbatim into a new method on the `impl WorkspaceShell`
block that holds `hero_focus_handle` (`:6457`):

```rust
    /// B5: the grid center's element tree — the Table, the promotion
    /// placeholder, or the empty-state hero.
    ///
    /// Extracted from `render` VERBATIM so `GridPanel` can call it: the panel
    /// is a thin wrapper and this shell still owns every piece of grid state.
    /// It needs `&mut self` because the hero arm mints the hero focus handles
    /// through `hero_focus_handle` and can flip `tour_auto_shown`.
    pub(crate) fn render_grid_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        match (self.data_source.as_ref(), self.table_state.as_ref()) {
            // …the three arms, unchanged…
        }
    }
```

In `render`, delete the `let body = match … };` block **by content bounds**:
find the `let body = match (` line and the closing `};` that precedes the
`// Slice 6 Task 3: is a REAL grid mounted this frame` comment, and assert both
anchors before deleting. ⚠ **Never delete a code block by heuristic end-match** —
in B2 a script keyed on "the next `};`" ran 163 lines past its target and took
the tab strip and the whole grid key handler with it; it was caught by reading
the diff, not by the compiler. After deleting, read the diff.

Also add, near `hero_focus_handle`:

```rust
    /// B5: the shell's root focus handle — the grid's tab stop and the host of
    /// the arrow-key handler. `GridPanel::focus_handle` returns this so a
    /// focus request routed at the panel lands on the real grid.
    pub(crate) fn grid_focus_handle(&self) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
```

And in `src/lib.rs`, between `pub mod overlay;` and `pub mod package;`:

```rust
pub mod panels;
```

- [ ] **Step 6: Run the test and the compile gate**

```
cargo test -p dat0-app --lib panel_name_is_frozen
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: PASS, and clippy exit 0.

⚠ The `body` local is consumed at `:7242` (`.child(div().flex_1().child(body))`),
so deleting the match leaves that call site dangling. For **this task only**,
point it at the extracted method — `.child(div().flex_1().child(self.render_grid_body(cx)))`
— so T1 is independently green; T2 Step 5 replaces it with the dock.

- [ ] **Step 7: Prove the extraction changed nothing**

```
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b5-t1.txt
grep -c "test result: ok" /tmp/b5-t1.txt
grep -n "FAILED\|panicked" /tmp/b5-t1.txt | head -20
```

Expected: **112** ok, 0 failures, `a11y_spike` still at 8 nodes. This is a pure
code move, so anything red here is the move being non-verbatim.

- [ ] **Step 8: Commit**

```bash
git add crates/dat0-app/src/panels crates/dat0-app/src/lib.rs crates/dat0-app/src/window.rs
git commit -s -m "feat(theme): B5 T1 — GridPanel and the render_grid_body extraction"
```

---

## Task 2: Mount the DockArea

**Files:**
- Modify: `crates/dat0-app/src/window.rs` — two fields, the lazy build, the body-row edit, `register_panels` in `run_app`, the a11y-capture shim
- Modify: `crates/dat0-app/tests/a11y_content.rs` — red-first mount assertion
- Modify: `crates/dat0-app/tests/keyboard_nav.rs` — `init_components` gains `register_panels`

**Interfaces:**
- Consumes: `GridPanel::new`, `GridPanel::PANEL_NAME`, `register_panels`, `render_grid_body`, `grid_focus_handle` (Task 1).
- Produces, for Task 3: `WorkspaceShell::dock_mounted_for_test(&self) -> bool` (gated `#[cfg(feature = "a11y-capture")]`).

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/a11y_content.rs` (it already has
`open_shell_window` at `:114` and an `AsyncHarness`; reuse them — this is why
the slice needs no new binary):

```rust
/// B5: the grid center is mounted through a `DockArea`, and the grid still
/// paints its cells. The dock is an implementation detail of `render`, so the
/// mount itself is observed through an `a11y-capture` shim; the CONTENT
/// assertion is the part that would catch a dock that mounts but renders
/// nothing.
#[gpui::test]
fn grid_renders_through_the_dock_panel(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _guard = h.enter();
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    let session = build_empty_session_in(&h, tmp.path());
    init_components(cx);
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert!(
        cx.update(|cx| shell.read(cx).dock_mounted_for_test()),
        "the shell rendered without building its DockArea"
    );
}
```

⚠ Match the file's existing harness idioms exactly — copy the setup preamble
from `grid_renders_cell_values_as_a11y_cells` (`:210`) rather than inventing
one, including how it seeds a data source and drains the dispatcher.

- [ ] **Step 2: Run it and watch it fail**

```
cargo test -p dat0-app --features a11y-capture --test a11y_content grid_renders_through_the_dock_panel
```

Expected: FAIL — `no method named dock_mounted_for_test`.

- [ ] **Step 3: Add the two shell fields**

In `struct WorkspaceShell` (`src/window.rs:2113`), near the other view state:

```rust
    /// B5: the DockArea hosting the grid center. Built lazily on the first
    /// render because `DockArea::new` needs a `&mut Window`, which only exists
    /// inside `render` — the same constraint that makes `table_state` a lazy
    /// promotion.
    dock_area: Option<gpui::Entity<gpui_component::dock::DockArea>>,
    /// B5: the center panel. Held across frames; rebuilding it per frame would
    /// mint a fresh entity every render and throw away the panel's identity.
    grid_panel: Option<gpui::Entity<crate::panels::grid_panel::GridPanel>>,
```

And in the constructor (`:2469` neighbourhood), initialise both to `None`.

- [ ] **Step 4: Build the dock lazily in `render`**

Place this immediately after the `table_state` promotion block (which ends near
`:6623`), so both lazy builds sit together:

```rust
        // B5: lazily build the DockArea and its center GridPanel. `DockArea::new`
        // needs a `&mut Window`, available only here.
        //
        // The center is `DockItem::Panel`, NOT `DockItem::Tabs`: `Tabs` renders a
        // `TabPanel`, which always paints a 30px title bar (under `PanelStyle::Auto`
        // a single visible panel gets a title row, not "no chrome"), wraps the panel
        // in a scroll container plus a cached element, and marks the container a tab
        // group. `DockItem::Panel` renders the panel's raw view instead — no chrome,
        // nothing between this shell and the virtualized `Table`. See the design doc
        // §2.1; asserted by the T0 chrome gate.
        if self.dock_area.is_none() {
            let weak_shell = cx.entity().downgrade();
            let panel = cx.new(|_| crate::panels::grid_panel::GridPanel::new(weak_shell));
            let dock = cx.new(|cx| {
                let mut dock = gpui_component::dock::DockArea::new(
                    "dat0-workspace",
                    Some(1),
                    window,
                    cx,
                );
                // v1 is resize + collapse only, never drag-rearrange. With
                // `DockItem::Panel` there is no tab bar to drag; this is for B6+.
                dock.set_locked(true, window, cx);
                dock
            });
            let item = gpui_component::dock::DockItem::panel(std::sync::Arc::new(panel.clone()));
            dock.update(cx, |dock, cx| dock.set_center(item, window, cx));
            self.grid_panel = Some(panel);
            self.dock_area = Some(dock);
        }
```

- [ ] **Step 5: Make the one body-row edit**

Hoist next to the other render hoists (`catalog_fh`, `ai_handles`):

```rust
        // B5: the grid center now renders through the DockArea.
        let dock_el = self.dock_area.clone();
```

Then at `:7242`, replace

```rust
                    .child(div().flex_1().child(self.render_grid_body(cx)))
```

with

```rust
                    .child(div().flex_1().children(dock_el))
```

`children(Option<Entity<DockArea>>)` is used rather than `child(...)` so a
hypothetical pre-build frame renders an empty flex cell instead of panicking;
the lazy build above runs earlier in the same render, so in practice it is
always `Some`.

- [ ] **Step 6: Register the panel in production and in tests**

In `run_app` (`src/window.rs:1777`, right after `register_modal_keys`):

```rust
        // B5: register the dock panels so `DockArea::load` can resolve them by
        // name (B9). Tests must call this too — see `panels::register_panels`.
        crate::panels::register_panels(cx);
```

In `tests/keyboard_nav.rs::init_components` (`:133`) and
`tests/a11y_content.rs::init_components` (`:165`):

```rust
    cx.update(dat0_app::panels::register_panels);
```

- [ ] **Step 7: Add the test shim**

In the existing `#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block
(`src/window.rs:7300`):

```rust
    /// B5: has the shell built its DockArea? The dock is an implementation
    /// detail of `render`, and the integration tests live in another crate.
    pub fn dock_mounted_for_test(&self) -> bool {
        self.dock_area.is_some()
    }
```

- [ ] **Step 8: Run the test and watch it pass**

```
cargo test -p dat0-app --features a11y-capture --test a11y_content grid_renders_through_the_dock_panel
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: PASS, clippy exit 0.

- [ ] **Step 9: Prove non-vacuity**

Temporarily make the lazy build a no-op (`if false && self.dock_area.is_none()`),
re-run, confirm RED, revert, **`touch src/window.rs`**, re-run, confirm GREEN.
A shim that reports `true` for a dock nobody renders is exactly the failure this
step exists to rule out.

- [ ] **Step 10: Full suite — this is the real gate for this task**

```
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b5-t2.txt
grep -c "test result: ok" /tmp/b5-t2.txt
grep -n "FAILED\|panicked" /tmp/b5-t2.txt | head -40
```

Expected: **112** ok, 0 failures. Specifically expected to still pass, and to be
read individually in the log rather than trusted from the summary:
`a11y_spike` (exactly 8 nodes), `keyboard_nav::grid_tab_reach_then_arrow_moves_active_cell`,
`keyboard_nav::hero_tab_cycle_visits_every_button`,
`a11y_content::grid_renders_cell_values_as_a11y_cells`.

- [ ] **Step 11: Commit**

```bash
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/a11y_content.rs crates/dat0-app/tests/keyboard_nav.rs
git commit -s -m "feat(theme): B5 T2 — mount the grid center in a DockArea"
```

---

## Task 3: Regression coverage for the re-parenting

**Files:**
- Modify: `crates/dat0-app/tests/a11y_content.rs`
- Modify: `crates/dat0-app/tests/keyboard_nav.rs`
- Modify: `crates/dat0-app/tests/a11y_spike.rs` (comment only)

**Interfaces:**
- Consumes: `dock_mounted_for_test` (Task 2).
- Produces: nothing consumed downstream.

The existing suites already cover the behaviour; what they lack is any statement
that the coverage now runs *through a dock*. This task makes the intent explicit
so a future slice cannot delete the coverage by accident.

- [ ] **Step 1: Extend the grid-content test with a through-the-panel assertion**

In `a11y_content.rs::grid_renders_through_the_dock_panel` (added in Task 2), add
the content half — the dock is worthless if the grid stops painting:

```rust
    // The grid's cells must still reach the capture tree through the panel
    // indirection. Mirrors `grid_renders_cell_values_as_a11y_cells`, which is
    // the canonical grid-content assertion in this file; that test now
    // exercises the dock path too, and this one states it deliberately.
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label("Loading grid…") || snap.node_count() > 0,
        "the dock mounted but the grid body painted nothing"
    );
```

⚠ Use this file's real snapshot API. `A11ySnapshot::query_by_role(role, label)`
is the query API (`tests/support/mod.rs:128`) and it **panics on duplicate
matches**; check what `grid_renders_cell_values_as_a11y_cells` uses and use the
same calls rather than inventing `has_label`/`node_count` if they do not exist.

- [ ] **Step 2: Run it, prove it red first**

Perturb: make `render_grid_body` return `div().into_any_element()` for the grid
arm, run the test, confirm RED, revert, `touch`, confirm GREEN.

```
cargo test -p dat0-app --features a11y-capture --test a11y_content
```

- [ ] **Step 3: Name the dock in the keyboard test**

In `keyboard_nav.rs::grid_tab_reach_then_arrow_moves_active_cell` (`:965`), add
an assertion at the top that the path under test is the dock path, plus a
comment recording why the test is load-bearing for B5:

```rust
    // B5: the grid now renders inside a `DockArea` → `DockItem::Panel` →
    // `GridPanel`, which delegates back to `WorkspaceShell::render_grid_body`.
    // The shell root keeps the focus handle and the arrow-key handler, so this
    // test is the proof that the indirection did not break either. Keys are
    // driven with `simulate_keystrokes`, never `dispatch_action` — the latter
    // bypasses the keymap and a green test could hide a dead production path.
    assert!(
        cx.update(|cx| shell.read(cx).dock_mounted_for_test()),
        "grid keyboard nav is no longer exercising the dock path"
    );
```

- [ ] **Step 4: Record the B5 recount in the frame-bracket proof**

In `tests/a11y_spike.rs`, extend the comment above the `click_ids.len() == 7`
/ node-count assertion (`:80-105`) with one line: B5 re-parents the grid center
under a `DockArea` and the count is UNCHANGED, which is this slice's
single-render proof. Do not touch the number.

- [ ] **Step 5: Run the affected suites**

```
cargo test -p dat0-app --features a11y-capture --test a11y_content
cargo test -p dat0-app --features a11y-capture --test keyboard_nav
cargo test -p dat0-app --features a11y-capture --test a11y_spike
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: all green, clippy exit 0.

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/tests
git commit -s -m "test(theme): B5 T3 — assert the grid renders and navigates through the dock"
```

---

## Task 4: Retire the vacuous bench claim

**Files:**
- Modify: `crates/dat0-app/benches/grid_scroll.rs` (module doc only — **no code change**)

**Interfaces:** none.

This is the slice's answer to the question B3 opened and the master plan
deferred twice. It is documentation, and it is the deliverable that stops the
next three slices from citing a measurement that cannot contain their changes.

- [ ] **Step 1: Extend the module doc**

Append to the existing module doc (which already carries the T13 note about
`render_td_cell` needing `&mut Window`):

```rust
//! ⚠ UI-redesign B5 ruling — what this bench does NOT measure.
//!
//! This harness calls `renderers::render_cell` in a plain loop over a synthetic
//! Arrow batch. It never builds a `Window`, a `WorkspaceShell`, or the
//! `gpui_component::Table` widget, so it is blind to everything about how the
//! grid is MOUNTED: the `TableDelegate`, `render_td`, per-cell theme reads, and
//! the element tree above the table. Its sensitivity surface is `render_cell`
//! plus Arrow column access, and nothing else.
//!
//! Consequently the A5 and A6 readings — "the bench held with `grid/mod.rs` in
//! the diff" — were measuring something that structurally could not contain
//! those changes. They are not evidence of no regression; they are evidence of
//! nothing, and they should not be cited as reassurance.
//!
//! B5 (DockArea adoption) keeps this bench as a `render_cell` watchdog and
//! bases its own no-regression claim on structure instead: `grid/mod.rs` is
//! byte-untouched, and `DockItem::Panel` puts zero elements between the shell
//! and the `Table`. Real per-frame timing remains D-013's perf runner, which
//! already owns it (P10-exit gap).
```

- [ ] **Step 2: Verify it still compiles as a bench target**

```
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: exit 0. (`cargo bench` itself is unrunnable on this machine — it
builds a fresh `libduckdb-sys` release tree straight into the macOS 27 / Xcode
26.6 Thrift breakage. Clippy `--all-targets` does compile-check the bench.)

- [ ] **Step 3: Commit**

```bash
git add crates/dat0-app/benches/grid_scroll.rs
git commit -s -m "docs(bench): record what grid_scroll cannot measure (B5 ruling)"
```

---

## Task 5: Whole-branch review and the full local gate

**Files:** none changed unless the review finds something.

**Interfaces:** none.

Every slice since A3 has had the cross-cutting final review catch something the
per-task reviews structurally could not — A3's two-muted-greys divergence, B3's
"1 cells selected", B4's global-chord-over-a-modal crash. Budget for it finding
something.

- [ ] **Step 1: Read the whole branch diff as one change**

```bash
git diff f389dc0...HEAD
```

Look specifically for: anything in the `render_grid_body` extraction that is not
verbatim; a hoist that changed evaluation order; the deleted `body` block having
taken a neighbour with it (B2's 163-line near-miss); `self.dock_area` read before
the lazy build; and any comment that spells a banned colour constructor with call
parens (the style-lint prose trap, three prior instances).

- [ ] **Step 2: Run the full local gate**

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app 2>&1 | tee /tmp/b5-final-plain.txt
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b5-final-a11y.txt
cargo test -p dat0-app --features a11y-capture,gallery 2>&1 | tee /tmp/b5-final-gallery.txt
cargo build -p dat0-app --bin dat0
grep -c "test result: ok" /tmp/b5-final-plain.txt /tmp/b5-final-a11y.txt /tmp/b5-final-gallery.txt
grep -n "FAILED\|panicked" /tmp/b5-final-*.txt | head -40
```

Expected: fmt clean, clippy exit 0, **112 / 112 / 112** ok with 0 failures,
binary builds.

- [ ] **Step 3: Confirm the invariants explicitly**

```bash
git diff f389dc0...HEAD --stat -- crates/dat0-app/src/grid    # expect: EMPTY
grep -n 'ALLOW' crates/dat0-app/tests/style_lint.rs | head     # expect: [("window.rs", 1)]
cargo test -p dat0-app --test style_lint                        # expect: 4 passed
git diff f389dc0...HEAD --stat -- crates/dat0-app/src/session crates/dat0-i18n  # expect: EMPTY
```

- [ ] **Step 4: Boot the app and look at it**

```
cargo build -p dat0-app --bin dat0
DAT0_CONFIG_DIR=/tmp/dat0-b5-glance ./target/debug/dat0
```

This slice's glance is a **diff-the-pixels** check, not a feel check: hero, grid,
and the four fixed docks (catalog / connections / AI / inspector / charts) must
look identical to `f389dc0`. Any title bar, any extra border, any changed
spacing above the table means `DockItem::Panel` is not doing what §2.1 says.
Check one theme now; the full 3-theme pass is the owed human glance.

- [ ] **Step 5: Fix whatever the review found, then commit**

```bash
git add -A
git commit -s -m "fix(theme): B5 — whole-branch review findings"
```

If the review found nothing, skip the commit and record that in the PR body.

- [ ] **Step 6: Push and open the PR**

```bash
git push -u origin feat/ui-redesign-b5-dock-skeleton
gh pr create --title "feat(theme): B5 — DockArea skeleton (UI redesign)" --body-file <path>
```

⚠ **Never write the CI skip marker in any commit message or PR body, even
quoted in prose** — it has silently skipped a main run twice. On merge, pass
explicit `--subject` and `--body-file` to the squash.

Then: watch **both** platforms on the PR → squash-merge → **watch the post-merge
main run**, verifying the bench at STEP level (a green job can mask a skipped
bench) and downloading the artifact with `gh run download` for the ns/iter, while
remembering that per Task 4 the number is a `render_cell` watchdog reading and
not evidence about this slice. Watch macOS `DISK[after-live-ai]` (4.6 Gi at B4;
the #65 hotfix line is 2.9 Gi) — this slice adds no test binary, so it should
hold.

---

## Self-review

**Spec coverage:** design §2 → T2 Step 5; §2.1 → T0 gate 3 + T2 Step 4 comment;
§2.2 → design only, B9 constraint recorded, no code owed here; §2.3 → T2 Step 4;
§3 → T1 Steps 3-4; §3.1 → T1 Step 5; §3.2 → T0 gate 1 with the fallback named;
§4 → T0 all three gates; §5 → T4; §6 → T2 Step 10 + T3 + T5 Step 2; §7 risk
table → T0 gates 1-3, T1 Step 7, T5 Step 3; §8 non-goals → nothing in any task
touches docks, persistence, drag, or hero migration.

**Type consistency:** `GridPanel::new(WeakEntity<WorkspaceShell>)`,
`GridPanel::PANEL_NAME`, `panels::register_panels(&mut App)`,
`WorkspaceShell::render_grid_body(&mut self, &mut Context<Self>) -> AnyElement`,
`WorkspaceShell::grid_focus_handle(&self) -> FocusHandle`,
`WorkspaceShell::dock_mounted_for_test(&self) -> bool` are spelled identically
in every task that references them.

**Known plan-time risk:** T3 Step 1's snapshot assertion is written against
`has_label`/`node_count`, which may not be this file's real API — the step says
so explicitly and instructs the implementer to copy
`grid_renders_cell_values_as_a11y_cells`'s calls instead. That is the one place
in this plan where the exact code must be read from the tree rather than trusted
from here.
