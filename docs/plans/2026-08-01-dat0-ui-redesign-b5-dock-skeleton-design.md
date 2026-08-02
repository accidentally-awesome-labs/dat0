# dat0 UI Redesign — Slice B5: DockArea skeleton (design)

Date: 2026-08-01
Branch: `feat/ui-redesign-b5-dock-skeleton` off main `f389dc0` (B4)
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B5
Pinned gpui-component rev: `0f0ab35` (must not be bumped)

B5 is the first of four slices (B5–B8) that move dat0's shell onto
`gpui_component::dock::DockArea`. It converts the **center only**, to a single
`GridPanel`, and is supposed to produce **no visible change at all** — which
makes it the slice where a silent regression is easiest to miss and hardest to
notice by eye. Every gate below is chosen with that in mind.

---

## 1. What exists today

`WorkspaceShell::render` (`src/window.rs:6483-7289`) builds one flat element
tree. The relevant part is the body row (`:7208-7269`):

```
div#workspace-shell (flex_col, shell focus handle, arrow key handler, modal trap)
├── banners / tab strip / pipeline bar / sql console panel
├── body row (flex_row, flex_1)
│    ├── catalog dock       .w_64().border_r_1()      when catalog_panel_visible
│    ├── connections dock   .w_64().border_r_1()      when connections_panel_visible
│    ├── ai dock            .w_64().border_r_1()      when ai_panel_visible
│    ├── div().flex_1().child(body)          ← the grid / hero / placeholder
│    ├── inspector dock     .w_72().border_l_1()      when inspector_panel_visible
│    └── charts dock        .w(560px).border_l_1()    when chart_panel_visible
├── status bar (B3 — sibling of the body row, spans under every dock)
└── popover / cell-editor / modal overlays, then Root's sheet + dialog layers
```

`body` is a three-arm match built at `:6650-6755`:

| arm | condition | content |
|---|---|---|
| real grid | `data_source` + `table_state`, source non-empty | `Table::new(state).stripe(true).bordered(true)` wrapped in a `div` carrying the selection-aware context menu |
| placeholder | data source landed, `TableState` not yet promoted | `div().child("Loading grid…")` |
| hero | no source, or an empty one | `EmptyState::new(recents_empty, first_run_done, recents_active).render(&hero, cx)` |

The hero arm needs `&mut self`: it mints the five fixed hero focus handles plus
one per sample entry through `hero_focus_handle` (`:6457`), and it can flip
`tour_auto_shown` when scheduling the one-shot onboarding tour (`:6717`).

---

## 2. The one structural edit

`.child(div().flex_1().child(body))` becomes `.child(div().flex_1().child(dock_area))`.

Nothing else in the shell tree moves. The fixed docks keep flanking the
DockArea — that hybrid is the point of the slice, since it proves the
conversion can proceed panel-by-panel instead of as one big-bang rewrite.

```
├── div().flex_1().child(dock_area)
│     └── DockArea { center: DockItem::Panel { view: GridPanel }, no docks yet }
│           └── render_items() → view.view()  → GridPanel::render
│                 └── shell.update(|ws, cx| ws.render_grid_body(window, cx))
```

### 2.1 Why `DockItem::Panel` and not `DockItem::Tabs`

Verified at the pinned rev, not assumed:

- `DockArea::render_items` (`dock/mod.rs:1055-1062`) renders `DockItem::Panel`
  as `view.clone().view().into_any_element()` — **the panel's raw `AnyView`,
  with no wrapper of any kind**.
- `DockItem::Tabs` renders a `TabPanel`, and `TabPanel::render`
  (`dock/tab_panel.rs:1177-1198`) *always* emits a title bar. Under
  `PanelStyle::Auto` with one visible panel (`:625`) that is not "no chrome" —
  it is a **30 px title row** carrying the panel title and the toolbar. The
  master plan's T0 spike (c), "single-tab center hides tab bar
  (`PanelStyle::Auto`)", is wrong on this point: Auto swaps the *tab bar* for a
  *title bar*, it does not remove chrome.
- The Tabs path also wraps the panel in `overflow_y_scroll()` +
  `.cached(StyleRefinement::default().absolute().size_full())`
  (`tab_panel.rs:830-862`) and marks the container `.tab_group()` (`:1192`).
  All three are hazards here: a nested scroll container around a **virtualized
  `Table`**, a cached child element against a **single-frame a11y capture**, and
  a tab group that reorders Tab traversal (B2: groups reorder, they do not
  contain).

`DockItem::Panel` avoids all of it. It is the only mount shape consistent with
"no visible change".

### 2.2 What that costs, and who pays it (B9)

`PanelState::to_item` (`dock/state.rs:222-232`) rebuilds a dumped
`PanelInfo::Panel` as `DockItem::tabs(vec![view])`. **A bare panel does not
round-trip** — restore it through `DockArea::load` and it comes back wearing
the very title bar this slice exists to avoid.

⇒ **B9 must not persist the center.** `dock_layout` covers the left/right/bottom
docks; the center is reconstructed as `DockItem::panel(GridPanel)` on every
boot. Recorded here because B9 is three slices away and this is exactly the
kind of constraint that gets rediscovered the hard way.

### 2.3 Lifecycle

`DockArea::new(id, version, window, cx)` (`dock/mod.rs:514`) needs a
`&mut Window`, which the shell only has inside `render`. So
`dock_area: Option<Entity<DockArea>>` is built lazily on the first render —
the same pattern `table_state` already uses (`window.rs:6594`), for the same
reason. Construction does:

```rust
let dock = cx.new(|cx| {
    let mut d = DockArea::new("dat0-workspace", Some(1), window, cx);
    d.set_locked(true, window, cx);
    d
});
dock.update(cx, |d, cx| {
    d.set_center(DockItem::panel(Arc::new(grid_panel.clone())), window, cx)
});
```

`set_locked(true)` (`dock/mod.rs:670`) disables tab dragging. With
`DockItem::Panel` there is no tab bar to drag, so this is belt-and-braces for
B6+ — v1 is resize + collapse only, never drag-rearrange.

---

## 3. GridPanel

New module `src/panels/grid_panel.rs` (+ `src/panels/mod.rs`). A dedicated
directory rather than `src/view/`: seven `Panel` implementors are coming across
B5–B8, and the trait impl is a different kind of thing from a free render fn.

The panel is **thin**. It holds exactly one field:

```rust
pub struct GridPanel {
    shell: WeakEntity<WorkspaceShell>,
}
```

- `Panel::panel_name() -> "GridPanel"` — **frozen from this slice onward**; it
  is B9's serialization key and the upstream trait doc says a panel name must
  never change once defined.
- `Focusable::focus_handle` returns **the shell's root focus handle**, not a
  handle of its own. That handle is the grid's tab stop and the host of the
  arrow-key handler, so any `window.focus(panel)` — from dock code now or a
  later slice — lands on the real grid. A private handle would be tracked by no
  element, and focusing it would silently swallow focus instead. Needs a
  `pub(crate)` accessor on `WorkspaceShell` (the field at `window.rs:2201` is
  private).
- `EventEmitter<PanelEvent>` — empty impl. v1 emits nothing; B9 wires
  `LayoutChanged`.
- Every other `Panel` method keeps its default. None of them is consulted on
  the `DockItem::Panel` path (no title bar reads `title()`, no toolbar reads
  `zoomable()`), but B6 starts reading them the moment a real dock appears.

`register_panel(cx, "GridPanel", …)` (`dock/panel.rs:356`) is called in
`run_app` **and** in each test binary's `init_components`. Nothing loads a
layout until B9, but registering now makes the name real and gives the suite
something to assert. Precedent: `register_modal_keys` has the same dual-site
rule, and a binding registered in prod but not in tests is silently absent
(B1/B2).

### 3.1 The shell side

`WorkspaceShell::render_grid_body(&mut self, window, cx) -> AnyElement` is
today's `body` match **cut out verbatim**, no edits. The shell keeps
`data_source`, `table_state`, `selection`, `recents_active`, the hero focus-handle
map, the root focus handle, the arrow-key handler, and the context-menu builder.

Grid mutation stays shell-coupled. The master plan's "`recents_active` + hero
handles move into the panel" is **deliberately deferred**: moving focus handles
in the same slice that introduces the dock indirection would leave two suspects
if `hero_tab_cycle_visits_every_button` or `keyboard_nav` goes red. B7 is the
declared focus-migration slice; it can take the hero with it.

### 3.2 Update-through, and its fallback

`GridPanel::render` calls back into the shell:

```rust
impl Render for GridPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_grid_body(window, cx))
    }
}
```

Whether that is legal is **T0-gate 1**. What is already proven is the weaker
claim: a descendant *reading* the shell during its own render is fine —
`GridTableDelegate` does `ws.upgrade()…read(cx)` inside `render_td`
(`grid/mod.rs:503-505`) and has since P3. What is unproven is *updating* it,
and B4 showed exactly how that fails: a registry closure dispatched from inside
a `Context<WorkspaceShell>` update panicked with "cannot read WorkspaceShell
while it is already being updated", and ~2/3 of palette commands would have
crashed on Enter.

If T0 gate 1 comes back red, the fallback is a **data snapshot, not an element
mailbox**: the shell computes the `&mut self`-requiring bits in its own render
(hero handle map, `recents_empty`, `first_run_done`, `recents_active`) and
pushes them into the panel with `panel.update(cx, …)`; the panel then builds
elements itself from that snapshot plus a weak read of `data_source` /
`table_state` / `selection`. Parking a built `AnyElement` in the entity was
considered and rejected — a frame-scoped element stored across frames paints
stale content the moment the panel re-renders without the shell.

---

## 4. T0 gates — all three run before any implementation work

Following the T0 hard-gate convention used since the kbd-nav slices: a
throwaway probe each, with an explicit STOP clause.

**Gate 1 — update-through re-entrancy.** Mount a trivial child entity whose
`render` calls `shell.update(…)`, force a frame, assert no panic.
STOP if red → switch to the §3.2 snapshot fallback and amend this document
before writing the plan.

**Gate 2 — single-frame capture under the dock.** Run the **entire** suite
against a cheap probe that mounts the grid body under a `DockArea`, and expect
`tests/a11y_spike.rs`'s exact node count (**8** on the hero) to hold. That
assertion is a frame-bracket double-render proof, not a content check — it
reacts to any added or duplicated capture site anywhere in the shell — so it is
the single most informative gate in this slice, and it is free.
STOP if it reads 16 (or any multiple) → the dock re-renders children per forced
frame, and the pre-designed generation-counter fallback at `a11y/mod.rs:24`
(bump on `begin_frame()`, keep only max-gen nodes) gets built first, as its own
task.

Note this is the *third* consecutive slice where the whole-suite-against-a-probe
approach is the gate: B3's mount gate fired on `a11y_spike.rs` and on nothing
else, and grepping for "what asserts on labels" would have missed it entirely.

**Gate 3 — chrome absence, asserted not assumed.** Probe that
`DockItem::Panel` adds no title bar, no tab stop and no wrapper element between
the shell and the `Table`. §2.1 is read from the pinned rev's source, but the
whole no-visible-change claim rests on it, so it gets an assertion rather than
a citation.
STOP if red → the slice's premise is void; return to the owner with the Tabs
trade-off before writing any code.

---

## 5. The bench ruling — B5 decides what B3 opened

B3 proved that `benches/grid_scroll.rs` **does not exercise the Table
delegate**. It loops `dat0_app::grid::renderers::render_cell` over a synthetic
1M-row Arrow batch (`benches/grid_scroll.rs:70-86`) and never builds a
`Window`, a `WorkspaceShell` or the `Table` widget — its own T13 module note
says why: `TableDelegate::render_td_cell` takes `&mut Window` and is unrunnable
headlessly, so real frame timing was deferred to the P10 perf runner (D-013).

The master plan puts bench gates on B5/B8/B10. All three touch DockArea/Table
**mounting**, which this bench structurally cannot see. The ruling for B5:

**Accept the bench as a `render_cell` watchdog, and say so in writing.** It
stays in CI unchanged and is still worth watching, because `render_cell` and
Arrow column access are genuinely on the per-cell hot path. It is *not*
evidence about mounting, and no B5 artifact will claim otherwise.

B5's actual evidence that the grid did not regress is structural:

1. `grid/mod.rs` is byte-untouched — the delegate, `render_td`, and every
   per-cell theme read are unchanged.
2. `renderers::render_cell` is untouched.
3. `DockItem::Panel` puts **zero elements** between the shell and the `Table`
   (§2.1, asserted by T0 gate 3) — no scroll container, no cached wrapper, no
   tab group. The added depth is two plain `div`s from `DockArea::render`.

Real per-frame timing remains D-013's perf runner, which already owns it and is
already tracked as a P10-exit gap.

**And the A5/A6 readings get retired.** "Bench held with `grid/mod.rs` in the
diff" (A5) and "+615 ns with `grid/mod.rs` in the diff, inside noise" (A6) were
both measuring something that could not contain the change. They are not
evidence of no regression; they are evidence of nothing. The slice notes say so
explicitly so the series' own history stops being cited as reassurance.

---

## 6. Tests and gates

**Zero new test binaries.** macOS CI disk is one-way (job-end `after-live-ai`
4.6 Gi at B4; the #65 hotfix line is 2.9 Gi), and B3 showed a whole slice can
land its coverage in existing suites. Coverage lands as:

| suite | what it proves under the dock |
|---|---|
| `tests/a11y_spike.rs` | exact node count still 8 → one render per forced frame through the dock indirection (T0 gate 2, kept permanently) |
| `tests/keyboard_nav.rs` | Tab still reaches the grid; arrows still move the selection; the shell root is still the key host |
| `tests/a11y_content.rs` | the grid's content nodes still reach the capture tree; hero still renders through the panel |
| wherever cheapest | `GridPanel::panel_name()` is `"GridPanel"` and is registered — a name-stability ratchet for B9 |

Keyboard tests drive real keystrokes via `simulate_keystrokes`, never
`dispatch_action` — a green test driven by `dispatch_action` can hide a dead
production key path (the carve-out #7 Escape-ladder lesson).

Non-vacuity: each new assertion is proven red by perturbing the thing it
asserts, before being accepted green.

Local gate (unchanged): `cargo fmt --check`; `cargo clippy --workspace
--all-targets -D warnings`; `cargo test -p dat0-app` × {plain, `a11y-capture`,
`a11y-capture,gallery`} = **112** binaries; `tests/style_lint.rs` ratchet
UNCHANGED at `[("window.rs", 1)]`; `cargo build -p dat0-app --bin dat0`.
`cargo test --workspace` and `cargo bench` remain unrunnable on this machine
(macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift — reproduces on `main`, not a
branch defect); bench numbers come from the post-merge CI artifact via
`gh run download`, verified at STEP level.

---

## 7. Risks and invariants

| risk | mitigation |
|---|---|
| Dock re-renders children → duplicate a11y capture nodes | T0 gate 2, run against the whole suite; generation-counter fallback pre-designed at `a11y/mod.rs:24` |
| `shell.update()` from panel render panics | T0 gate 1; snapshot fallback in §3.2 |
| A future gpui-component rev changes `render_items` and re-introduces chrome | T0 gate 3 becomes a permanent assertion, not a one-off probe |
| Focus lands on an untracked panel handle and disappears | `Focusable` returns the shell's root handle (§3) |
| B9 restores the center and the title bar reappears | §2.2 — center is never persisted |
| Silent visual regression in a slice with no expected visual change | the owed human glance is a **diff-the-pixels** check, not a feel check: grid, hero and the four fixed docks must look byte-identical to `f389dc0` |

Invariants held: no `grid/mod.rs` change; no schema change (session stays v10
until B9); no new colour literals (the ratchet stays at 1); Escape ladder
untouched (key contexts survive re-parenting, verified at the pinned rev, and
the dock binds no `escape`); no i18n keys (nothing new is user-visible).

---

## 8. Non-goals

- Left / right / bottom dock adoption — B6, B7, B8.
- Layout persistence and session v11 — B9.
- Drag-rearrange — never in v1; `set_locked(true)`.
- Hero and focus-handle migration into the panel — B7.
- A real frame-level bench — D-013's perf runner.
- Any visible change whatsoever. If the owner can see B5, B5 is wrong.

---

## 9. As-built: T0 gate findings (measured 2026-08-01)

Probe: `crates/dat0-app/tests/b5_t0_probe.rs`, throwaway, deleted before this
commit. Two of the three gates ran as designed; the third is re-sequenced for
the reason recorded below.

### Gate 1 — update-through re-entrancy: **GREEN**

A child entity created inside its parent's `render` calls
`parent.update(cx, |p, _| { p.renders += 1; … })` from inside its own `render`.
Measured over two forced frames (`add_window_view` → `run_until_parked` →
`window.refresh()` → `run_until_parked`): the counter reached **2**, the child
body painted (`debug_bounds` resolved), and no lease panic occurred.

⇒ **The primary mechanism holds.** `GridPanel::render` may call
`shell.update(cx, |ws, cx| ws.render_grid_body(cx))` directly. The §3.2 data-
snapshot fallback is NOT needed and is not built.

The reason this was in doubt is worth keeping: B4 hit
"cannot read WorkspaceShell while it is already being updated" when a registry
closure re-entered the shell from inside a `Context<WorkspaceShell>` update, and
the fix was `App::defer`. The difference is *when*: gpui has finished the
parent's `render` lease by the time a child view's element is laid out, so the
descendant's update is not re-entrant. A closure dispatched *during* the parent's
own update still is. Both facts are now measured rather than assumed.

### Gate 3 — chrome absence under `DockItem::Panel`: **GREEN**

A `DockArea` with `set_locked(true)` and `set_center(DockItem::panel(probe))`,
hosted in a full-window `div`:

```
host = Bounds { origin: (0px, 0px), size: 1920px × 1080px }
body = Bounds { origin: (0px, 0px), size: 1920px × 1080px }
```

Same origin, same height — the panel body is not pushed down by a title bar and
loses no vertical space to one. Asserted, not eyeballed: the probe fails if
`body.origin.y != host.origin.y` or `body.size.height != host.size.height`.

⇒ §2.1 holds as read from the source. `DockItem::Panel` is the zero-chrome mount.

### Gate 2 — single-frame a11y capture: **re-sequenced, not skipped**

The gate as designed wanted a cheap probe mounting the grid body under a dock
*before* T1/T2. There is no such probe: any faithful version needs the panel to
render the real grid body, which IS the T1 extraction plus the T2 mount. A stub
panel rendering an empty `div` would blank the grid and turn a dozen unrelated
suites red, drowning the one signal the gate exists to read.

⇒ Gate 2 runs as **T2 Step 10** instead — the full `--features a11y-capture`
suite against the real mount, with `tests/a11y_spike.rs`'s exact node count (8)
as the double-render proof. The STOP clause is unchanged and still armed: if the
count comes back a multiple of 8, the generation counter at `a11y/mod.rs:24`
(bump on `begin_frame()`, keep only max-gen nodes) gets built as its own task
before anything else proceeds. Gates 1 and 3 being green makes the ordering safe:
neither T1 (a verbatim code move) nor T2 (one element swap) is wasted work if the
capture needs a generation counter on top.
