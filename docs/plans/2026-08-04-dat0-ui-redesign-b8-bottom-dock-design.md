# Slice B8 — SQL console → bottom dock (UI redesign)

Date: 2026-08-04
Branch: `feat/ui-redesign-b8-bottom-dock`, off main `9ceff53` (B7).
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B8, size M.

Predecessors: B5 (`cda2974`, DockArea skeleton) → B6 (`921dde8`, right dock) →
B7 (`9ceff53`, left dock + activity rail). Successor: B9 (`dock_layout`
persistence, session v10→v11).

---

## 1. Goal

Move the SQL console out of the shell's own layout column and into the
`DockArea`'s **bottom dock**, so it renders below the grid and becomes
resizable. This is the last panel migration of the dock series; after it,
every dat0 surface that the master plan wants docked is docked, and B9 can
persist the whole layout as one blob.

Today the console is a fixed 260 px strip that the shell mounts *above* the
body row (`window.rs:7497-7512` builds it, `:7690` places it), so it spans the
entire window width and sits between the pipeline bar and the grid. After this
slice it is a `Panel` inside `DockPlacement::Bottom`, which upstream renders
inside the center column — below the grid, and horizontally bounded by the left
and right docks.

## 2. Verified upstream facts

All read from the pinned rev `0f0ab35`
(`~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/crates/ui/src/dock/`),
not assumed. Three of them change the design.

### 2.1 The bottom dock lives inside the center column

`DockArea::render` (`mod.rs:1120-1140`) builds a `flex_row` of
`left_dock │ center │ right_dock`, and `center` is a `flex_col` of
`render_items()` (flex_1) followed by `bottom_dock`. So the bottom dock is a
sibling of the *center item*, not of the whole row.

⇒ **The console stops spanning the full window width.** It will not extend
under the activity rail, the left dock, or the right dock. This is inherent to
upstream's layout and cannot be changed without forking.

### 2.2 A closed bottom dock still paints 29 px

`Dock::render` (`dock.rs:370-380`):

```rust
if !self.open && !self.placement.is_bottom() {
    return div();
}
…
.map(|this| match self.placement {
    DockPlacement::Bottom => this.w_full().h(self.size),
    …
})
// Bottom Dock should keep the title bar, then user can click the Toggle button
.when(!self.open && self.placement.is_bottom(), |this| this.h(px(29.)))
```

`TabPanel` carries the matching half (`tab_panel.rs:740-752`): clicking a tab
while `is_bottom_dock && is_collapsed` calls
`dock_area.toggle_dock(DockPlacement::Bottom, …)`.

⇒ A collapsed bottom dock is **upstream-intended VSCode behaviour**: the title
bar survives as the affordance that reopens it. It is also the master plan's
listed risk ("residual bottom-dock bar") and is **unavoidable** — a
`Panel::visible()` returning `false` only blanks the title bar's contents; the
`Dock` still reserves the 29 px.

### 2.3 `set_bottom_dock` leaks exactly like left and right

`mod.rs:625-647` runs `subscribe_item`, which `push`es onto the `DockArea`'s
`_subscriptions` and recurses over the item tree; nothing removes them (B6's
finding, re-confirmed for `Bottom`). ⇒ **called exactly once**; every
open/close goes through `toggle_dock`.

### 2.4 `Dock::set_open` is synchronous for `open`, deferred only for collapse

`dock.rs:259-266`:

```rust
pub fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
    self.open = open;                       // synchronous
    let item = self.panel.clone();
    cx.defer_in(window, move |_, window, cx| {
        item.set_collapsed(!open, window, cx);   // deferred
    });
    cx.notify();
}
```

⇒ Reading `DockArea::is_dock_open(Bottom, cx)` immediately after `toggle_dock`
returns the **new** value. This is what makes §4's derived getter safe to read
in the same call that toggles.

(The deferred `set_collapsed` is also the exact mechanism behind B6's
double-capture finding; `A11ySnapshot::capture` already pumps a settle frame
for it, so nothing new is owed here.)

### 2.5 `closable` is suppressed by the dock lock, not by any dat0 override

`Panel::closable` defaults to **`true`** (`panel.rs:90-93`) and no B6/B7 panel
overrides it — yet no ✕ has ever appeared. The reason is
`TabPanel::closable` (`tab_panel.rs:100-113`):

```rust
if !self.draggable(cx) && !self.in_tiles { return false; }
```

with `draggable = !is_locked(cx) && !is_last_panel(cx)` (`:423`). dat0 calls
`dock.set_locked(true, …)` at DockArea construction (`window.rs:7122`), so the
lock is what has been suppressing the close button for five panels already.

⇒ That is load-bearing and undocumented in dat0. B8 pins it with a test rather
than adding a sixth silent dependency.

### 2.6 The `…` menu button is unconditional

The single-visible-panel title bar (`tab_panel.rs:625-687`) calls
`render_toolbar`, which renders the `Ellipsis` button with no gate
(`:483-511`). Its dropdown holds a *disabled* "Zoom In" when `zoomable` is
`None`, plus "Close" only when closable.

⇒ **B6/B7's five docked panels already ship a dead `…` menu.** B8 inherits it
as a sixth instance; it cannot be removed without forking. Recorded, not
fought.

### 2.7 The collapse chevron lands in the right title bar

`render_dock_toggle_button(Bottom, …)` returns `Some` only for the tab panel
registered as `DockArea::toggle_button_panels.bottom`, which
`update_toggle_button_tab_panels` (`mod.rs:1078-1083`) sets from
`bottom_dock.panel.left_top_tab_panel(cx)` — the console's own tab panel. The
center is a `DockItem::Panel`, so it contributes no toggle buttons at all.

⇒ The console's own title bar gets the collapse chevron, which is where it
belongs.

## 3. Mount shape

**`DockItem::tab(console)` — TabPanel chrome.** Owner-decided.

The alternative, `DockItem::panel`, is supported by `Dock::render`
(`dock.rs:392`, wrapped in `.cached(absolute size_full)`) and would give zero
chrome — but §2.2 means collapsing would squeeze the whole console into a
29 px sliver with nothing to click to reopen. TabPanel chrome is the only shape
whose collapsed state is coherent.

Accepted costs:

- **Double chrome.** A 30 px "SQL Console" title bar sits above the console's
  own SQL tab strip. Folding the tab strip *into* `Panel::title` was considered
  and rejected: B6 established that upstream forces `tab_stop(false)` on
  title-bar buttons, which would kill the `tabstrip_focus` stop and break
  `sql_console_nav`.
- **Nested scroll.** `TabPanel::render_active_panel` (`tab_panel.rs:850-861`)
  wraps the panel in `overflow_y_scroll()` + `.cached(…)`, and the console
  contains a virtualized results `Table` (`pane_table_state`) with its own
  viewport — the exact nesting B5 dodged for the center. Gated by T0 (b).

## 4. Visibility becomes derived

The field `sql_console_visible: bool` (`window.rs:2325`) is **deleted** and
replaced by a getter over the dock:

```rust
pub(crate) fn sql_console_visible(&self, cx: &App) -> bool {
    self.dock_area
        .as_ref()
        .is_some_and(|d| d.read(cx).is_dock_open(DockPlacement::Bottom, cx))
}
```

`is_dock_open` (`mod.rs:691-706`) returns `false` when `bottom_dock` is `None`,
so the pre-mount state is correct without a special case.

**Why derived rather than B6/B7's shell-bool + reconciler.** Upstream gives the
bottom dock two toggle paths dat0 does not own — the title-bar chevron (§2.7)
and click-a-tab-while-collapsed (§2.2). Either flips `Dock::open` without
dat0's knowledge. A cached state tuple would then desync, `sync_bottom_dock`
would see `want == state` and do nothing, and the next ⌘⇧C would toggle
**backwards**. There is no reconciler-shaped fix that survives an external
writer; making the dock the single source of truth removes the class.

This costs nothing in persistence: `sql_console_visible` is set `false` at
construction and **never restored from the session** — only the console's
*tabs* persist (`persist_sql_console`). Deleting the field is free.

`toggle_sql_console` becomes:

1. `let dock = self.ensure_dock_area(window, cx);`
2. If `self.sql_console.is_none()`: build the console exactly as today
   (persisted tabs, shared autocomplete snapshot, `subscribe_in`, `ai_ready`
   hydration, the `on_window_should_close` persist backstop), then
   `dock.set_bottom_dock(DockItem::tab(console, …), Some(px(320.)), true, …)`
   — **the one and only call**.
3. Else: `dock.toggle_dock(DockPlacement::Bottom, window, cx)`.
4. Then the existing autocomplete refresh and catalog refresh, with the refresh
   gate now reading the derived getter (safe per §2.4).

**No focus change.** `toggle_sql_console` does not focus anything today; focus
is driven by the console's own `pending_focus` drain in `render`. Keeping that
makes B8 a pure move, so any focus difference at review is a real regression
rather than an intended change competing with it — the same reasoning B5 used
to defer hero focus migration out of its own slice.

## 5. `ensure_dock_area`

The lazy DockArea construction currently inlined at `window.rs:7114-7242` moves
verbatim into:

```rust
fn ensure_dock_area(&mut self, window: &mut Window, cx: &mut Context<Self>)
    -> Entity<DockArea>
```

`render` calls it and discards the return; `toggle_sql_console` calls it and
uses it. The heavily-commented B5/B6/B7 rationale blocks move with the code,
unedited.

**Why this is safe against B7's re-entrancy panic.** B7 proved `DockItem::tabs`
with >1 panel cannot be built while the shell is leased, because every
`add_panel` after the first runs `set_active_ix` → `Panel::visible` →
`shell.read`. `toggle_sql_console` holds `&mut self`, so the shell is leased
there too — but the bottom dock holds **exactly one** panel, and single-panel
`DockItem::tab` is immune: `set_active_ix(0)` early-returns on an unchanged
index (`tab_panel.rs:208-211`).

Side benefit: `window.rs` is 8293 lines and this is its densest block; the
extraction also gives B9's `DockArea::load` restore a clean seam.

## 6. Panel impls

New file `src/panels/sql_console_panel.rs`. `Panel` is a foreign trait but
`SqlConsole` is a local type, so the impls may live in any module of this
crate; keeping them under `src/panels/` matches every sibling and the module
doc in `src/panels/mod.rs`, which already reads *"B8 adds the SQL console"*.

`impl Panel for SqlConsole` **directly** (master plan §6), not a thin delegate.
B5–B7 used wrappers because grid/inspector/charts/catalog/connections/AI bodies
were render fns on the shell; the console is already a standalone entity owning
its own state, so a wrapper would add a hop and force `focus_handle` to walk
shell → console → tab.

| Item | Value | Why |
| --- | --- | --- |
| `EventEmitter<PanelEvent>` | new impl | Required by `Panel`. Additive — the console keeps emitting `SqlConsoleEvent`; two emitters on one entity is fine. |
| `Focusable` | active tab's editor `InputState` handle | There is no `impl Focusable for SqlConsole` today. Must be a handle some element actually tracks — B5: a private handle is tracked by no element and focusing it *swallows* focus. Index-guarded (`tabs.get(active).or(tabs.first())`). |
| `panel_name` | `"SqlConsolePanel"` | B9's serialization key; frozen from here, ratchet test per sibling precedent. |
| `title` | `sql.console.title` = "SQL Console" | Static. A dynamic "SQL Console — Query 2" duplicates the tab strip two pixels below, and the active tab's title is user-editable — a user naming a tab to match another node trips `query_by_role`'s duplicate-match **panic**, which takes whole suites down (B7's rail lesson). |
| `zoomable` | `None` | Matches every B6/B7 panel. `DockArea::zoom_view` swaps the dock's entire child tree — an untested path against the single-frame a11y capture, and scope this slice does not need. |
| `closable` | **not overridden** | Resolved `false` by the dock lock (§2.5). Pinned by a test instead of adding a sixth silent dependency. |
| `visible` | default `true` | Dock open/close *is* the visibility mechanism. Returning `false` blanks the title bar's contents while the `Dock` still reserves its height (§2.2) — strictly worse. |

`register_panels` gains the name with a degraded builder, matching the existing
B9 placeholder comment: it hands back a console over empty tabs and a fresh
shared snapshot rather than panicking. B9 replaces all seven with builders that
resolve the live shell.

## 7. Layout and traversal delta

```
BEFORE                                  AFTER
shell root (flex_col)                   shell root (flex_col)
├ banner / tab strip / pipeline bar     ├ banner / tab strip / pipeline bar
├ CONSOLE   260px, FULL WIDTH           ├ body row: rail │ DockArea
├ body row: rail │ DockArea             │              ┌ left │ grid  │ right ┐
│           ┌ left │ grid │ right ┐     │              │      ├───────┤       │
└ status bar                            │              │      │CONSOLE│       │
                                        │              └      └ 320px ┘       ┘
                                        └ status bar
```

The console's ~18 focus stops move from **before** the activity rail to
**after** the grid center and before the right dock. This is intended and
forced by the master plan's "console moves below grid"; every *other* order
movement is a regression, per the standing rule from B7.

Height is **320 px** (owner-chosen over the 260 px status quo), resizable via
upstream's handle, and **not persisted** — `dock_layout` is B9's.

## 8. T0 gate

Four probes, all owner-selected, landing before any migration commit. B5, B6
and B7 each had a T0 and each found something the design had wrong.

**(a) `a11y_spike` node-count movement.** The exact count (now **12**) is a
frame-bracket double-render proof, not a content check — it reacts to any added
capture site anywhere in the shell. Predict the delta, then bump with a comment
naming the contributing nodes. Never loosen to `>=`; that destroys the proof.

**(b) Nested scroll.** Run a `ResultTarget::Pane` query and prove the
console-owned results `Table` still scrolls inside TabPanel's
`overflow_y_scroll` + `.cached()` wrapper (§3).

**(c) Whole-shell Tab order with the console docked.** Walk the real order and
pin it. B7's rule is mandatory here: *a Tab-walk probe is meaningless unless
the reference point outside the tab group is itself a registered tab stop* —
two consecutive B7 probes "passed" while measuring nothing, and the more
convincing one was the `tab_node_for_focus_id → None → next(None)` fallback
restarting from the beginning. The activity rail is a registered stop outside
the DockArea and serves as that reference.

**(d) Neither upstream toggle path desyncs anything.** Drive the title-bar
chevron and the click-a-tab-while-collapsed path, and assert that
`sql_console_visible(cx)` tracks both and that the *next* ⌘⇧C toggles in the
correct direction rather than backwards (§4).

Per the standing lesson: drive keyboard behaviour with `simulate_keystrokes`,
not `dispatch_action` — the latter bypasses the keymap and a green test can
hide a dead production key path.

## 9. Tests

New `tests/bottom_dock.rs`, mirroring `left_dock.rs` (15 tests) and
`right_dock.rs`: mount, toggle, derived visibility, both upstream toggle paths,
collapsed-bar behaviour, the frozen panel name, and the resolved-`closable`
pin from §2.5. One new test binary takes the suite **115 → 116**; macOS CI disk
sits at 17 Gi against the 2.9 Gi failure line, so this is free.

Existing suites updated **only where the move genuinely changes them**:
`sql_console_nav` (9), `sql_console_transient_nav` (17), `input_nav` (7),
`keyboard_nav` (8), `a11y_spike` (4), `a11y_content`.

The Escape ladder needs no new coverage: `key_context("SqlConsole")` lives on
the console's own render root (`sql_console.rs:1174`), key contexts survive
re-parenting (master-plan-verified), and the standing 17-test transient-bar
suite already gates the full ladder.

## 10. Local gate

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo test -p dat0-app` × `{plain, a11y-capture, a11y-capture+gallery}`,
  **116 binaries, 0 failures**
- `tests/style_lint.rs` ratchet **unchanged** at `[("window.rs", 1)]`
- `git diff` empty for `src/grid` and `src/session`
- `cargo build -p dat0-app --bin dat0` **and boot it**, diffing the log against
  a `main` build — this is how B5's silent first-run-tour regression was found,
  and a silent success logs nothing, so the diff is the entire signal

`cargo test --workspace` and `cargo bench` remain unrunnable on this machine
(macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift).

**Bench.** The master plan lists a bench gate on B8, but B5 settled that
`benches/grid_scroll.rs` is a `render_cell` watchdog that never builds a
`Window`, a `WorkspaceShell` or the `Table` widget — it structurally cannot see
a mounting change. Verify it post-merge at **step** level (a green job can mask
a skipped bench), download the artifact for the series, and read no meaning
into the number.

## 11. Risks — recorded, not fought

| Risk | Disposition |
| --- | --- |
| Residual 29 px collapsed bar | Upstream-intended (§2.2). Lazy mount means it is invisible until the console is first opened, so the first-run hero is untouched. |
| Double chrome above the console's tab strip | Accepted (§3); the alternative kills a focus stop. |
| Dead `…` menu | Inherited from B6/B7, sixth instance (§2.6). Not fixable without forking. |
| Nested scroll over the results `Table` | T0 (b). |
| Narrow window: rail 48 + left 384 + right 848 = 1280 px of chrome, and the console now shares the remainder with the grid | Folded into the owed human glance, alongside B7's and B5's existing narrow-window items. Dock widths and any min-size policy belong to B10, which owns `window.rs` styling; upstream exposes no dock size setter (B6). |
| `set_bottom_dock` subscription leak | Called exactly once (§2.3). |

## 12. Out of scope

B9 persistence (`dock_layout`, session v10→v11) · auto-focus on open · a
minimum-center-width policy · panel zoom · removing the dead `…` menu · any
change to `src/grid` or `src/session`.

## 13. Owed human glance

All three themes, high contrast most of all: the 30 px title bar against the
console's own tab strip; the collapsed 29 px bar; the new resize splitter
between grid and console; the 320 px default against the old 260 px; the
console at narrow width with both side docks open. Plus the still-owed B4
palette, B5 diff-the-pixels / narrow-window / file-drop, B6 title-bars and B7
rail passes.

---

## 14. As-built

_(filled in at the end of the slice)_
