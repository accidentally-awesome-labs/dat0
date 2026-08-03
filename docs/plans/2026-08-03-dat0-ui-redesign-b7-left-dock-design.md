# UI Redesign — Slice B7: left dock (Catalog + Connections + AI) and the activity rail

Status: approved (owner, 2026-08-03), not yet implemented
Branch: `feat/ui-redesign-b7-left-dock` off main `921dde8`
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B7
Predecessors: B5 (`cda2974`, DockArea skeleton), B6 (`921dde8`, right dock)

## 1. What this slice does

Moves the three hand-rolled left docks — Catalog, Connections, AI — out of
`WorkspaceShell::render`'s body row and into the `DockArea`'s **left** dock, and
adds a **VSCode-style activity rail**: a 48 px vertical icon strip, always
visible, that selects which of the three panels is showing.

This is a deliberate scope increase over the master plan's B7 row, approved by
the owner at brainstorm. The plan's row described three side-by-side dock panels
mirroring B6. The rail changes the model to **one panel at a time**, which is
both better UX for three mutually-exclusive tool panels and — because it means
one `TabPanel` instead of three — materially less exposure in the slice the
master plan already flagged as the focus-migration hot spot.

Size: **L** (the plan said M-L; the rail is the difference).

### 1.1 Before and after

```
BEFORE (main @ 921dde8)                AFTER (B7)
┌────┬────┬────┬──────┬────┐          ┌──┬──────┬────────┬──────┐
│Cat │Conn│ AI │ grid │ R  │          │▓ │ Cat  │  grid  │  R   │
│256 │256 │256 │flex_1│dock│          │▓ │ 384  │ flex_1 │ dock │
│    │    │    │      │848 │          │▓ │      │        │ 848  │
└────┴────┴────┴──────┴────┘          └──┴──────┴────────┴──────┘
 3 fixed .w_64() blocks in the         rail 48px │ left dock, one
 body row, independently toggled       panel at a time, rail-selected
```

## 2. Verified upstream facts (pinned rev `0f0ab35`)

Each was read from the checkout, not assumed. Facts B5/B6 already established are
cited, not re-derived (per the B7 kickoff note).

1. **`set_left_dock` leaks exactly like `set_right_dock`** (`dock/mod.rs:603-623`):
   it calls `subscribe_item`, which `push`es onto `_subscriptions` and recurses
   over the item tree; nothing removes them. ⇒ call it **exactly once** at mount;
   use `toggle_dock` per toggle. Dock width is therefore **fixed at construction**
   — `DockArea` keeps `left_dock` private and exposes no size setter.
2. **`DockItem::tabs` + visibility needs no `active_ix` plumbing.**
   `TabPanel::active_panel` (`tab_panel.rs:192-204`) returns the panel at
   `active_ix` *only if it is visible*, and otherwise **falls back to the first
   visible panel**. So flipping the shell's visibility bools is sufficient to
   change which panel renders; `active_ix` can stay 0 forever.
3. **Exactly one visible panel renders a 30 px title row, not a tab bar**
   (`tab_panel.rs:623-640`): the title-bar branch is taken when
   `visible_panels.len() == 1 && panel_style == PanelStyle::default()`. Two or
   more visible ⇒ a horizontal tab bar appears. This is why §5's radio invariant
   is load-bearing rather than cosmetic.
4. **`.tab_group()` is applied once, to the whole `TabPanel` container**
   (`tab_panel.rs:1192`). One `DockItem::tabs` therefore introduces **one** tab
   group regardless of how many panels it holds.
5. **A hidden child of a split collapses** (`stack_panel.rs:427-431`, via
   `resizable_panel().visible(..)`) — the fallback path in §3 relies on this; it
   is B6's mechanism, unchanged.
6. **`DockPlacement::Left` exists** (`dock/dock.rs:29-37`), so `toggle_dock` and
   `is_dock_open` work per placement and the right dock is unaffected.
7. **Tooltips are available at this rev**: gpui core has
   `InteractiveElement::tooltip` (`gpui-0.2.2/src/elements/div.rs:1161`) and
   gpui-component ships a `Tooltip` view (`tooltip.rs:15`). Two comments in
   `view/sql_console.rs` (`:703`, `:861`) say "no `.tooltip()` helper exists at
   this gpui-component rev" — true of gpui-component's *element* helpers, and
   misleading about the capability. Those comments get corrected in this slice.
8. Settled by B6, not re-derived: TabPanel chrome is **transparent to the a11y
   capture**; `A11ySnapshot::capture` already pumps a settle frame, so opening a
   dock does not duplicate the tree; the generation counter at `a11y/mod.rs:24`
   stays unbuilt.

## 3. Alternatives considered

**`DockItem::split` of three (B6's exact shape).** Rejected for the rail model.
It gives three `TabPanel`s, hence **three** `.tab_group()`s and three 30 px title
bars, tripling Tab-order churn in the slice where Tab order is the declared risk.
Its one advantage — a stray tab bar is structurally impossible — is bought more
cheaply by the §5 invariant plus a test. **Retained as the T0 fallback** (§12).

**Horizontal tabs at the top of the dock (no rail).** The dock's own tab bar as
the selector; zero new chrome. Rejected by the owner in favour of the VSCode
rail, which keeps the selector visible when the dock is collapsed.

**CatalogPanel owning tree/collapsed/active** (the master plan's literal wording).
Rejected: that row was written 2026-07-21, before B5 proved the thin-panel
template. Moving `catalog_tree` / `catalog_collapsed` / `catalog_active` out of
the shell would also touch `catalog_nav_key`, session persistence and three a11y
test shims — real risk, no user-visible gain, and it would land in the same slice
as the focus migration. Panels stay thin; the state move is not scheduled.

**Dock width 256 (today's `w_64`) or 768 (sum of three).** Owner chose **384**.
With one panel at a time the sum has no meaning, and 384 gives the Catalog tree
room to breathe without the 768 px land-grab that a summed width would cause.

## 4. Layout

The body row loses its three `.children(...)` blocks entirely and becomes:

```rust
div().flex().flex_row().flex_1()
    .child(activity_rail::render_rail(&rail_model, cx))   // 48 px, always visible
    .child(div().flex_1().children(dock_el))              // DockArea: left | center | right
```

The rail is a **sibling of the `DockArea`, not a dock panel**. That is what lets
it stay visible when the dock is collapsed — the point of the VSCode model — and
it keeps the rail out of the `.tab_group()` entirely.

The left dock is built once, next to the existing right dock, at the same mount
site in `render`:

```rust
let left = DockItem::tabs(
    vec![Arc::new(catalog), Arc::new(connections), Arc::new(ai)],
    &weak_dock, window, cx,
);
dock.update(cx, |dock, cx| {
    dock.set_left_dock(left, Some(px(LEFT_DOCK_WIDTH)), any_left_visible, window, cx);
});
```

`LEFT_DOCK_WIDTH = 384.0`, a module const beside `INSPECTOR_DOCK_WIDTH` /
`CHARTS_DOCK_WIDTH`.

## 5. The radio invariant: at most one left panel visible

The three existing shell bools (`catalog_panel_visible`,
`connections_panel_visible`, `ai_panel_visible`) remain the single source of
truth — the dock derives from them, never the reverse (master plan §6's rule,
B6's `sync_right_dock` precedent). B7 adds an invariant:

> **At most one of the three is `true` at any time.**

Fact 3 in §2 is why: two `true` makes upstream paint a horizontal tab bar
directly beside the rail — two selectors for one choice.

### 5.1 One method owns every transition

```rust
pub(crate) enum LeftPanel { Catalog, Connections, Ai }

pub(crate) fn activate_left_panel(&mut self, target: LeftPanel, cx: &mut Context<Self>) {
    let already_open = self.left_panel_visible(target);
    self.catalog_panel_visible = false;
    self.connections_panel_visible = false;
    self.ai_panel_visible = false;
    if !already_open {
        // ...set the target's bool true
        if matches!(target, LeftPanel::Catalog) { self.refresh_catalog(cx); }
    }
    self.persist_dock_ui();
    cx.notify();
}
```

Activating the panel that is already open clears all three — the dock then has no
visible panel and `sync_left_dock` closes it. That is the owner-chosen
click-active-to-collapse behaviour, and it falls out of the invariant rather than
being a special case.

Every entry point funnels through this method:

- the three rail items (click, and rail Enter/Space);
- the three View-menu actions `menu_macos::{CatalogToggle, ConnectionsToggle,
  AiPanelToggle}` — their current handlers each flip one bool inline
  (`window.rs:7371-7387`, `:5640`) and are replaced by a single call. The
  `refresh_catalog` + `persist_dock_ui` calls the Catalog handler does today move
  into `activate_left_panel` so no path loses them. `tests/menu_reachability.rs`
  keeps all three actions alive.

### 5.2 ⚠ The three a11y test shims must be routed through it too

This is the one place B6's guidance does not transfer. B6 recorded that the
capture shims "write a bool, the next frame reconciles" — safe there because the
right dock has no invariant. Here, three existing shims set a left bool
**directly**:

| Shim | `window.rs` | Sets |
|---|---|---|
| `seed_catalog_tree_for_test` | ~`:7563` | `catalog_panel_visible = true` |
| `open_connections_for_test` | ~`:7571` | `connections_panel_visible = true` |
| `seed_ai_panel_for_test` | ~`:7664` | `ai_panel_visible = true` |

A test calling two of them would produce two `true` bools and a tab bar — a
test-only path silently violating a production invariant.

The fix is a two-level split rather than three shim rewrites that each have to
remember the rule. `activate_left_panel` is decomposed into:

```rust
/// Flips the three bools and nothing else. The ONLY writer of all three —
/// which is what makes the at-most-one invariant structural.
fn set_left_panel_exclusive(&mut self, target: Option<LeftPanel>) { .. }

/// The user-facing transition: collapse-if-open, refresh, persist, notify.
pub(crate) fn activate_left_panel(&mut self, target: LeftPanel, cx: &mut Context<Self>) { .. }
```

Production paths (rail, menu) call `activate_left_panel`. The three shims call
`set_left_panel_exclusive`, which preserves the invariant while skipping the
refresh — `seed_catalog_tree_for_test` **must** keep bypassing `refresh_catalog`,
since it seeds fakes that the real off-thread `get_tables` would clobber
(`window.rs:2999`, documented on the shim itself). No path outside these two
methods may write a left-panel bool; that is asserted by a test, not just by
review.

### 5.3 Syncing

`sync_left_dock` mirrors `sync_right_dock` exactly: it runs in `render`, is
guarded by a new `left_dock_state: (bool, bool, bool)` tuple so it does work only
on an actual change, and calls `toggle_dock(DockPlacement::Left, ..)` when
`is_dock_open` disagrees with `any of the three`. It never re-runs
`set_left_dock`.

### 5.4 Session

Unchanged. `SessionUiState` still persists `catalog_panel_visible` only (the other
two have always defaulted `false` at construction, `window.rs:2527`/`:2536`).
Schema stays **v10** — additive `dock_layout` work is B9's, per the master plan.

## 6. Panels

Three new files on B5/B6's thin template, `src/panels/{catalog_panel,
connections_panel, ai_dock_panel}.rs`. Each holds **one** field, a
`WeakEntity<WorkspaceShell>`, and:

- `PANEL_NAME` frozen from this slice (`"CatalogPanel"`, `"ConnectionsPanel"`,
  `"AiDockPanel"`), with the same rename-ratchet unit test B6 wrote;
- `visible(cx)` reads the shell bool through a getter, `false` on a dead weak
  handle (the B9 placeholder-builder path);
- `zoomable()` → `None` (v1 dock scope is resize + collapse);
- `Focusable::focus_handle` → **the shell's** handle, never a private one (B5: a
  private handle is tracked by no element and focusing it swallows focus);
- `render` → `shell.update(|ws, cx| ws.render_*_body(cx))`.

`register_panels` in `src/panels/mod.rs` gains all three names — dual-site
registration (`run_app` and each test binary's `init_components`), as B5 built it.

### 6.1 The shell's three new body methods

`render_catalog_body`, `render_connections_body`, `render_ai_body` — each the
corresponding body-row block moved **verbatim**, minus the `.w_64().border_r_1()`
wrapper (sizing and borders are the dock's job now).

All three take `&mut self`. That is not incidental: `catalog_fh` and the eight
`ai-*` handles are minted from the shell's `hero_focus` map via
`hero_focus_handle(&mut self, ..)`, which is why they are currently hoisted at
`window.rs:7244` and `:7252` before the body row is built. Moving the mint into
the body methods keeps the map — and therefore **handle identity** — on the shell.
That is precisely what keeps `catalog_nav` and `ai_nav` meaningful across the
move: the same `FocusHandle` instances end up on the same elements, just under a
different parent.

### 6.2 Titles

Each panel today draws its own title as a bare `.child(SharedString::from(t(..)))`
(`catalog/panel.rs:73`, `connections/panel.rs:199`, `ai/panel.rs:234`). Per A5's
rule a bare child contributes **nothing** to the capture tree, so these are
capture-invisible today. All three move into `Panel::title()` — leaving them in
the body would print the word twice under the dock's 30 px title bar (B6's
Inspector lesson).

Accessible naming differs per panel, deliberately:

| Panel | `title()` | Why |
|---|---|---|
| Catalog | plain text, **no** `a11y_label` | its root already carries `.a11y("catalog-tree", Button, t("catalog.title"))` (`catalog/panel.rs:68-72`); a second node named "Catalog" would make `A11ySnapshot::query_by_role` **panic on a duplicate match** |
| Connections | `a11y_label` | nothing else names this panel |
| AI | `a11y_label` | nothing else names this panel |

Net capture delta from titles: **+2 nodes**, no duplicates.

## 7. The activity rail

New file `src/view/activity_rail.rs`. `src/view/` — not `src/` — is where every
rendered shell surface lives; B3 recorded this after the master plan guessed
`src/status_bar.rs`.

### 7.1 Structure

48 px wide, full height, always rendered — including on the first-run hero, where
it is the path to Connections for a user with no data yet. Three items, top to
bottom: Catalog, Connections, AI.

### 7.2 Keyboard: the listbox pattern

One container `focus_stop("activity-rail", fh, 0, ring, activate)` plus a chained
`on_key_down` for `up`/`down`, exactly as `catalog/panel.rs:45-67` does. This adds
**one** tab stop to the shell, not three — the reason it was chosen over
per-item stops in a slice already migrating nine handles.

Two independent pieces of state, and conflating them is the easiest way to get
this wrong:

- **`rail_cursor: usize`** (new shell field) — the keyboard cursor. Moved by
  ↑/↓. Exists even when the dock is collapsed and nothing is open.
- **which panel is open** — derived from the three bools.

Enter/Space activates the panel under the cursor via `activate_left_panel`, so
Enter on the open panel collapses the dock, matching the mouse. Clicking an item
sets the cursor **and** activates, so the two never drift after a mouse
interaction. This is the same two-state model as the catalog tree's active row vs
its selection.

### 7.3 Visuals

- open panel: a 2 px accent bar on the item's leading edge plus a raised
  background (VSCode's idiom);
- cursor: the standard focus ring, drawn by `focus_stop`;
- icon color follows ambient text style for free (A5: `Icon` inherits
  `window.text_style().color`).

Every colour comes from `Dat0Colors` / `cx.theme()`; **no new colour literals**,
so `tests/style_lint.rs`'s ratchet stays at `[("window.rs", 1)]`.

### 7.4 Naming — and a collision the first draft of this design walked into

The obvious choice is to label each rail item with the panel's own name, reusing
`catalog.title` / `connections.title` / `ai.title`. **That breaks the a11y tests.**
`A11ySnapshot::query_by_role(role, label)` **panics on a duplicate match**
(`tests/support/mod.rs:128`), and while the Catalog panel is open the tree would
hold two nodes named "Catalog": the rail item and the catalog tree's own
`.a11y("catalog-tree", Button, t("catalog.title"))`. §6.2 avoided exactly this
collision for the panel titles and then the rail reintroduced it — the same trap,
one section later.

Rail items are therefore named for the **action**, not the panel:

| Item | Accessible name and tooltip | Key |
|---|---|---|
| Catalog | "Show Catalog" | `rail.show_catalog` |
| Connections | "Show Connections" | `rail.show_connections` |
| AI | "Show AI" | `rail.show_ai` |

This is also the more honest name — the item is a button that reveals a panel, not
the panel — and it keeps the tooltip text identical to the accessible name, which
is what a screen-reader user and a sighted user should hear and see.

Tooltips are built with `gpui_component::Tooltip` via gpui's `.tooltip(..)`. They
render into an overlay only while hovering, so they contribute nothing to a
captured frame.

## 8. Icons and i18n

Three new Lucide SVGs vendored into `crates/dat0-app/assets/icons/` — the path A5
built and proved:

| Rail item | Asset | Enum variant |
|---|---|---|
| Catalog | `icons/database.svg` | `Dat0IconName::Database` |
| Connections | `icons/plug.svg` | `Dat0IconName::Plug` |
| AI | `icons/sparkles.svg` | `Dat0IconName::Sparkles` |

`Dat0IconName::ALL` goes 5 → 8, which automatically pulls them into the icon
resolution test and the gallery. Two A5 constraints carry:

- **Licensing must be checked per icon.** Lucide is dual-licensed: Feather-derived
  icons are MIT (Cole Bemis), the rest ISC. `NOTICE`'s hand-written
  `## Bundled assets` section — above the `cargo-about` marker so
  `scripts/notice-extract.sh` is unaffected — records whichever applies to each of
  the three.
- The `Dat0Assets` own-first shadowing gate (asserting dat0's filenames are
  disjoint from upstream's) must stay green; none of the three exists upstream
  today, and the gate is what catches it if a future rev adds one.

Four new i18n keys: **`rail.title`** (the rail container's accessible name,
"Activity bar") and **`rail.show_catalog`** / **`rail.show_connections`** /
**`rail.show_ai`** (§7.4 — the item labels must NOT reuse the panel titles, or the
capture tree gets duplicate names and `query_by_role` panics). A5's warning
applies when adding them: duplicate keys in `en.json` are silently overwritten
with no error, so confirm none of the four already exists.

## 9. Shell changes

| Change | Detail |
|---|---|
| New fields | `left_dock_state: (bool, bool, bool)`, `rail_cursor: usize`, `catalog_panel`/`connections_panel`/`ai_dock_panel` entity handles |
| New const | `LEFT_DOCK_WIDTH: f32 = 384.0` |
| New methods | `activate_left_panel`, `left_panel_visible`, `sync_left_dock`, `render_catalog_body`, `render_connections_body`, `render_ai_body`, plus `catalog_visible()` / `connections_visible()` / `ai_visible()` getters for the panels |
| Mount site | left dock built in the same `if self.dock_area.is_none()` block as the right dock; `set_left_dock` called once |
| `render` | three `.children(..)` blocks deleted; `.child(rail)` added; `sync_left_dock(window, cx)` called beside `sync_right_dock` |
| Menu handlers | three inline bool flips replaced by `activate_left_panel` calls |
| Test shims | three rewritten to route through `activate_left_panel` (§5.2) |

`src/grid/`, `src/session/`, and `dat0-i18n`'s existing keys are untouched — the
`git diff` on the first two being empty is part of the local gate, as in B5/B6.

## 10. Risks, and how each is discharged

| # | Risk | Discharge |
|---|---|---|
| R1 | `.tab_group()` reorders Tab so `catalog-tree` and the eight `ai-*` stops land in the wrong order, or become unreachable. **The master plan's declared hot spot.** | T0 probe (§12) before any implementation, with a `DockItem::split` fallback and an explicit STOP clause |
| R2 | The radio invariant is violated by some path, producing a horizontal tab bar beside the rail | One method owns every transition (§5.1); shims routed through it (§5.2); a test asserts at most one visible after every transition |
| R3 | Moving `catalog_fh` / `ai_handles` changes handle identity and silently breaks `catalog_nav` / `ai_nav` | Handles stay minted from the shell's `hero_focus` map (§6.1); the two suites are gates, not afterthoughts |
| R4 | The rail is new hero chrome — P11a's onboarding tour and hero band were designed without it | Boot the built binary and diff the log against a `main` build (B5's tour-regression method); `onboarding_gpui` + `onboarding` suites are gates |
| R5 | `a11y_spike`'s exact node count moves — the rail adds **+4** on every screen including the hero (1 container + 3 items), and an open panel adds **+1** more for a labelled title (§6.2) | Expected and deliberate. Measure in T0/P4, then bump the constant **with a comment naming the new nodes**; a `>=` would destroy the double-render proof (B3's rule) |
| R8 | A new accessible name collides with an existing one and `query_by_role` panics — the trap §7.4 walked into once already | Rail items named for the action, not the panel; test 4b asserts it per panel rather than trusting the review |
| R6 | Tooltips are unproven in this codebase | Smoked in T0; they are additive and can be dropped without touching the rest of the slice if they misbehave |
| R7 | A tab group plus a collapsed dock changes where focus goes when the open panel disappears under the user's cursor | Covered by an explicit test: collapse while focus is inside the panel, assert focus lands somewhere live (the `ai-key-forget` self-removing-button lesson) |

The bench is **not** a gate: B5 ruled `benches/grid_scroll.rs` a `render_cell`
watchdog that cannot see mounting, and wrote that into the bench's own module doc.
Verify the post-merge run at step level; read no meaning into the number.

Disk is **not** a per-slice worry: `c4b3aba` took macOS job-end to 17 Gi, ~6× the
2.9 Gi failure line. This slice may add test binaries freely.

## 11. Test plan

**New: `tests/left_dock.rs`**, mirroring `tests/right_dock.rs`'s harness
(`boot` → `open_shell_window` → `settle`):

1. dock is closed when all three panels are hidden;
2. activating each panel opens the dock and titles it correctly (×3);
3. activating the open panel collapses the dock;
4. **the radio invariant** — driving every activate/collapse transition between
   the four reachable states (none open, Catalog, Connections, AI), assert after
   each that at most one bool is true;
4b. **no unnamed duplicate** — with each panel open in turn, `query_by_role` for
   that panel's name resolves without panicking (§7.4's collision, asserted
   rather than assumed);
5. catalog body content reaches the capture through the dock;
6. focus survives a collapse driven from inside the panel (R7).

**New: rail coverage** (same file): Tab reaches `activity-rail`; ↑/↓ move the
cursor; Enter activates; Enter on the open panel collapses; the cursor and the
open panel stay consistent after a click.

**Existing suites that gate this slice** (all must stay green, and the first two
are the point of the slice): `catalog_nav`, `ai_nav`, `keyboard_nav`,
`a11y_content`, `sql_console_transient_nav`, plus `right_dock`,
`dock_chrome_spike`, `onboarding_gpui`, `menu_reachability`, `style_lint`,
`icon_assets`, and `a11y_spike` with a documented count bump.

**Non-vacuity**, per A6's standing rule: for each new assertion, perturb the thing
it claims to test and confirm it goes red. Remember to `touch` the source after
reverting a probe — a backwards-dated file makes cargo reuse the stale binary and
report a false red (A6).

## 12. T0 hard gate

Runs first, on a throwaway probe, and its findings are committed before T1.

| Probe | Question | STOP condition |
|---|---|---|
| P1 | With the left dock open and Catalog active, does Tab still reach `catalog-tree`, and do ↑/↓ still route to `catalog_nav_key`? | Cannot be made green ⇒ fall back to `DockItem::split` (§3) and re-plan T1 |
| P2 | With AI active, are all eight `ai-*` handles still reachable in the documented order? | Same fallback |
| P3 | Does exactly-one-visible really render a 30 px title row and **no** tab bar? | If a tab bar appears with one visible panel, the rail model needs re-design before any code lands |
| P4 | What is the `a11y_spike` node-count delta? | None — this is a measurement, recorded and used to set the new constant |
| P5 | Does `.tooltip(..)` with `gpui_component::Tooltip` compile and render? | Drop tooltips to icon-only a11y labels; nothing else changes |

Per B6, no chrome/double-render spike is needed: that question is settled and
`tests/dock_chrome_spike.rs` stands as its regression guard.

## 12b. T0 as-built (2026-08-03)

**Verdict: GO.** `DockItem::tabs` stands; the `DockItem::split` fallback is not
needed. Spike: `crates/dat0-app/tests/left_dock_spike.rs`, six probes, all green.

| Probe | Result |
|---|---|
| P1 multi-stop reachability | **PASS** — all three stops in a docked panel are Tab-reachable. B6 only ever proved this for ONE stop. |
| P1d escape (added mid-gate) | **PASS** — `host-stop → a-1 → a-2 → a-3 → host-stop`. Tab enters the group and returns to a stop outside the `DockArea`. No keyboard trap. Proven non-vacuous: removing the outside stop makes it red. |
| P2 document order | **PASS** — stops are visited in document order, so `catalog_nav` / `ai_nav` order assertions should hold. |
| P3 title row vs tab bar | **PASS** — one visible panel yields one title and hidden panels contribute none. Its **control** (two visible → two titles) proves the assertion is not vacuous: without it, P3 would pass under either branch. |
| P4 capture baseline | `a11y_spike` is **8** today and unchanged by the dock. The rail adds four `.a11y` sites, so Task 5 sets **12**. |
| P5 tooltip | **PASS**, with two corrections below. |

### The two findings that corrected the design

**1. `Tooltip` is not at gpui-component's crate root.** `lib.rs:66` is a bare
`pub mod tooltip;`, so the path is **`gpui_component::tooltip::Tooltip`**, and
`.tooltip()` lives on `StatefulInteractiveElement` — the element needs `.id()`
first **and** that trait imported. §7.4's plan compiled only after both fixes.

**2. A focus stop inside the CENTER panel does not register as a tab stop.** This
one first read as a keyboard trap and nearly failed the gate. Staging focus inside
the docked panel showed Tab cycling `a-1 → a-2 → a-3 → a-1`, never reaching the
center probe — which looks exactly like WCAG 2.1.2. It is not. A stop rendered
inside a `DockItem::panel` center is **captured by the a11y snapshot but absent
from the tab-stop order**, so there was nothing outside the group to escape to and
`next()` was wrapping to the global first (`tab_stop.rs:130`). The follow-up walk
staged on the center then showed `center → a-1`, which looks like traversal but is
the `tab_node_for_focus_id → None → next(None)` fallback (`tab_stop.rs:123-125`) —
an unknown focus id restarting from the beginning. Both readings were artifacts of
the probe, not behaviour of the dock.

**The lesson, and it generalises:** a Tab-walk probe is only meaningful if the
reference point outside the group is itself a registered tab stop. Introducing
`host-stop` — a stop outside the `DockArea` entirely, the analogue of the activity
rail — is what made the question answerable at all. **Two consecutive probes
"passed" while measuring nothing**, and the second one's output was the more
convincing of the two.

Production is unaffected by the center quirk: the grid's tab stop lives on the
shell's root element, outside the dock, which is why
`keyboard_nav::grid_tab_reach_then_arrow_moves_active_cell` has stayed green since
B5. It is recorded here because any future slice that puts a focus stop inside the
center panel will find it unreachable.

## 13. Local gate

Unchanged from B6, plus one binary:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p dat0-app` × {plain, `a11y-capture`, `a11y-capture,gallery`} —
  **115 binaries** expected (114 as of B6, +1 for `left_dock`)
- `cargo build -p dat0-app --bin dat0` **and boot it**, diffing the log against a
  `main` build (B5's tour regression was found only this way)
- `tests/style_lint.rs` ratchet unchanged at `[("window.rs", 1)]`
- `git diff` empty for `src/grid/` and `src/session/`

`cargo test --workspace` and `cargo bench` remain unrunnable on this machine
(macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift; reproduces on `main`).

## 14. Owed human glance

The pre-B7 combined pass (B4 palette, B5 diff-the-pixels + narrow window + file
drop, B6 title bars/divider/export) is being run by the owner **before** this
slice lands, so B7 starts from a clean visual baseline.

B7 itself will owe, in all three themes and high contrast most of all:

- rail width, icon size and spacing; the accent bar for the open panel and the
  focus ring for the cursor, and whether the two read as distinct;
- rail contrast against the shell background (WCAG ≥ 3:1 for the non-text accent);
- the 384 px dock against the old 256 px docks — Catalog tree indentation, the
  Connections panel's buttons, the AI panel's rows at the new width;
- the rail on the **first-run hero**, beside the onboarding band;
- tooltip legibility and placement;
- a narrow window, where the rail plus a 384 px dock plus an 848 px right dock
  can exceed the viewport.

## 15. As-built (2026-08-03)

Merged shape matches the design except where noted. Local gate: fmt clean,
`clippy --workspace --all-targets -D warnings` exit 0, **115 binaries × 3 feature
combos, 0 failures**, `style_lint` ratchet unchanged at `[("window.rs", 1)]`,
`src/grid` and `src/session` byte-identical to main, and the built binary boots
with a log identical to a `main` build apart from the config-dir path.

### 15.1 ⚠⚠ The one real design change: split, not tabs

**§4 said `DockItem::tabs` of three. As built it is `DockItem::split` of three
single-panel `DockItem::tab`s** — the fallback §3 retained, adopted for a reason
nobody predicted.

`DockItem::tabs` **cannot be constructed from inside `WorkspaceShell::render` at
all**. It calls `TabPanel::add_panel` once per panel, and every add after the
first runs `set_active_ix`, which does real work and ends up calling
`Panel::visible` → `shell.read(cx)`. The shell is already leased by its own
`render`, so it panics: *"cannot read WorkspaceShell while it is already being
updated"*. All ten `left_dock` tests failed identically, which is what made the
cause obvious.

A single-panel `DockItem::tab` never trips it, because `set_active_ix`
early-returns when the index is unchanged (`tab_panel.rs:208-211`) — **which is
exactly why B6's two-panel right dock was fine, and why B6 could not have
discovered this.**

The tab-group cost that motivated tabs is moot: the at-most-one invariant means
at most one child is ever visible, so at most one group is ever populated, and a
hidden child collapses and yields its space (`stack_panel.rs:427-431`). T0's
measurements still apply — they were taken against real dock chrome.

### 15.2 What matched the prediction exactly

- `a11y_spike` 8 → **12**, the rail's four `.a11y` sites. The relocated panel
  titles contribute none, because `a11y_label` records `click_id: None`.
- All five declared focus-migration gates stayed green with nine handles now
  inside dock chrome: `catalog_nav`, `ai_nav`, `keyboard_nav`, `a11y_content`,
  `sql_console_transient_nav`.
- Zero new colour literals; the ratchet never moved.

### 15.3 Smaller as-builts

- **`tests/left_dock.rs` needs an ambient tokio runtime where `right_dock.rs`
  needed none.** The Catalog arm of `activate_left_panel` reaches
  `refresh_catalog`'s `tokio::spawn`, which panics with "there is no reactor
  running" under a bare `TestAppContext`. `ai_nav`'s `enter_async_harness` was
  copied in. This is a consequence of centralising the side effects, and it is
  the right trade: no entry point can lose the refresh.
- **Two `keyboard_nav` hero tests were updated, not worked around.** The rail is
  now the first tab stop on every screen — it is a sibling of the `DockArea` and
  precedes it in document order, and the hero lives inside the dock since B5. A
  real product change, so the expected sequences gained `rail.title`.
- **A test was measurably vacuous and is now documented as such.**
  `never_two_panel_names_in_the_tree_at_once` stayed GREEN under T2's
  non-vacuity probe while four siblings went red, because only the catalog
  contributed a named node at that point. It gained teeth at T4 when the
  Connections and AI titles moved into `Panel::title` with labels, and the probe
  was re-run there to confirm. **Recording the weakness beat deleting or
  trusting the test.**
- `LEFT_DOCK_WIDTH` and the three `*_visible()` getters had to be introduced in
  the commit that first uses them: `-D warnings` makes an unused const or method
  a hard error, so a "declare now, use next commit" split does not build.

### 15.4 Owed human glance (unchanged from §14)

Nothing in the local gate can see pixels. §14's list stands, with one addition
from 15.1: with only one panel ever visible, the split's **resize splitter now
sits between the open panel and the grid** — worth a look that dragging it feels
right and that B9 will have something sane to persist.
