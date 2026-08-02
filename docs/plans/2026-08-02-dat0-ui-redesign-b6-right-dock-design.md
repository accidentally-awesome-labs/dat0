# UI Redesign — Slice B6: right dock (Inspector + Charts)

**Date:** 2026-08-02
**Branch:** `feat/ui-redesign-b6-right-dock` off main `c4b3aba`
**Master plan:** `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B6 (size M)
**Predecessor:** B5 dock skeleton (`cda2974`, PR #74) — design `2026-08-01-dat0-ui-redesign-b5-dock-skeleton-design.md`

---

## 1. What this slice does

Moves the Inspector and Charts panels out of the hand-rolled fixed docks in
`WorkspaceShell::render`'s body row and into a real `DockArea` right dock,
following B5's thin-panel template.

B5 mounted the grid center as `DockItem::Panel`, which renders a panel's raw
view with zero chrome. **B6 is the first slice where dock chrome is real**, and
that inverts B5's headline result: a left/right/bottom `Dock` whose item is a
`Split` is built out of `TabPanel`s, and a `TabPanel` always paints a title bar,
wraps its panel in `overflow_y_scroll`, wraps it again in
`.cached(StyleRefinement::default().absolute().size_full())`, and marks the
container `.tab_group()`. So this slice is visible, and it re-opens the
master plan's top risk (double-render vs the single-frame a11y capture) that B5
retired only for the no-wrapper path.

Out of scope: the catalog / connections / AI left docks (B7-B8), focus
migration into the panels (B7), dock-layout persistence (B9), `window.rs`
styling and the last style-lint literal (B10).

---

## 2. Verified upstream facts (pinned rev `0f0ab35`)

Every claim below was read out of the vendored checkout at
`~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/crates/ui/src/dock/`,
not assumed. Several were not known when the master plan was written.

1. **A `Split`'s children MUST be `TabPanel` or `StackPanel`.**
   `StackPanel::insert_panel` opens with `assert_panel_is_valid`
   (`stack_panel.rs:106-112`), which hard-asserts exactly that. So a
   two-panel right dock structurally *cannot* dodge `TabPanel` chrome by
   nesting bare `DockItem::panel`s. This is an assert, not a style choice.

2. **`Dock::render` *can* host a bare `DockItem::Panel`** — `dock.rs:392`
   renders `view.cached(cache_style)` with no `TabPanel` at all. That is the
   escape hatch we are deliberately *not* taking (see §3, alternative C).

3. **`set_locked(true)` already kills the close button.**
   `TabPanel::closable()` returns `false` when `!draggable()`
   (`tab_panel.rs:107`), and `draggable()` is `!is_locked && !is_last_panel`
   (`:423`). B5 already calls `set_locked(true)`. ⇒ no ✕ appears, and the ⋯
   menu's Close entry is suppressed (`:506`). Nothing can un-mount a panel
   behind the View menu's back.

4. **No dock toggle button will render anywhere.**
   `render_dock_toggle_button` requires
   `toggle_button_panels.right == Some(self_entity_id)` (`:539`), and that
   field is filled from `self.items.right_top_tab_panel(cx)` — the **center**
   item's rightmost `TabPanel`. Our center is `DockItem::Panel`, whose
   `right_top_tab_panel` returns `None` (`mod.rs:508`). ⇒ the View menu stays
   the only way to open/close the right dock. This is what makes §5's
   single-source-of-truth design airtight rather than merely tidy.

5. **The ⋯ (Ellipsis) menu button renders unconditionally**
   (`tab_panel.rs:483`), regardless of `zoomable()` / `closable()`. Returning
   `zoomable() -> None` only makes its "Zoom In" row disabled; it cannot
   remove the button. Accepted as unavoidable chrome at this rev.

6. **`Panel::title()`'s default is the string `"Dock.Unnamed"`**
   (`panel.rs:70`) and it is called every frame from the title bar. B5 left it
   at the default because nothing on the `DockItem::Panel` path reads it; B6
   must override it, and must keep it cheap.

7. **Toolbar buttons are forced non-focusable.** `render_toolbar` applies
   `.xsmall().ghost().tab_stop(false)` to every button returned by
   `toolbar_buttons()` (`tab_panel.rs:454`). Upstream treats title-bar
   controls as mouse-only chrome. This is the fact that drives §6.

8. **`inner_padding` only applies with >1 panel per tab panel**
   (`:844`: `self.panels.len() > 1 && self.inner_padding(cx)`). One panel per
   `TabPanel` ⇒ no extra `pt_2`, so we leave the default alone.

9. **Hidden panels collapse, they do not blank.** `StackPanel::render` builds
   `resizable_panel().child(..).visible(panel.visible(cx))` (`:427-431`), so a
   panel whose `visible()` is false yields its space to its sibling.

10. **`set_collapsible(false)` force-opens the dock** (`dock.rs:140-143`), so it
    is *not* a way to suppress chrome. Not needed anyway, per fact 4.

11. **Dead-code oddity, recorded so it is not mistaken for a bug later:**
    `split_with_sizes` loops `add_panel` over its items **twice**
    (`mod.rs:221-231`). It is harmless only because `insert_panel` dedups at
    `:219`. No workaround required; do not "fix" it by pre-deduping.

---

## 3. Alternatives considered

**A — Split of two tab panels (CHOSEN).** `DockItem::split(Horizontal,
[tabs(Inspector), tabs(Charts)])`. Preserves today's side-by-side layout, gives
a real resizable divider and independent visibility, and produces the structure
B9 will persist. Cost: two 30px title bars + two ⋯ buttons.

**B — One tab panel, two tabs.** Rejected: only one panel visible at a time,
which removes a capability users have today (reading the inspector while a
chart is up).

**C — One dat0-owned wrapper panel** hosted as `DockItem::panel` (legal per
fact 2, zero upstream chrome). Rejected: it keeps today's pixels but buys none
of the dock semantics — no per-panel identity for B9, no divider — while still
paying the `.cached()` risk. It would make B6 a rename, not an adoption.

---

## 4. Panels

Two new files, both strictly B5's thin template (`panels/grid_panel.rs`):

- `src/panels/inspector_panel.rs` — `InspectorPanel`
- `src/panels/charts_panel.rs` — `ChartsPanel`

Each holds exactly one field, `shell: WeakEntity<WorkspaceShell>`, and:

| Member | Value | Why |
|---|---|---|
| `PANEL_NAME` | `"InspectorPanel"` / `"ChartsPanel"` | B9's serialization key; frozen from here on, with the same rename-ratchet unit test `GridPanel` has |
| `Focusable::focus_handle` | the **shell's** root handle | B5's rule: a private handle is tracked by no element, so focusing it swallows focus |
| `Render` | `shell.update(cx, \|ws, cx\| ws.render_*_body(cx))` | measured-safe in B5's T0: a child entity may update its parent from inside its own render |
| `title()` | `t("inspector.title")` / `t("charts.title")` | static; called every frame |
| `visible()` | shell bool via the weak handle, `false` if dead | §5 |
| `zoomable()` | `None` | v1 dock scope is resize + collapse only |
| `toolbar_buttons()` | `ChartsPanel` only: PNG / SVG | §6 |

`inspector.title` (`"Inspector"`) already exists in `en.json`. `charts.title`
(`"Charts"`, plural, matching the View menu) is the one new key —
`chart.panel.title` is `"Chart"` singular and is already interpolated into the
body's chart-type button, so it is not reused here. A5's lesson applies: JSON
silently overwrites duplicate keys; `charts.title` was confirmed absent before
being added to this design.

### 4.1 Amendment — the inspector already draws its own title

Found while planning against the tree, not at design time.
`inspector::panel::render_inspector` opens with its own title row —
`div().a11y_label(AccessRole::Label, "Inspector").child("Inspector")`
(`inspector/panel.rs:37-40`) — which the new 30px title bar would duplicate on
screen.

⇒ **the inspector body extraction drops that title row**, and
`InspectorPanel::title()` returns `div().a11y_label(AccessRole::Label, t).child(t)`
rather than a bare `SharedString`. Three reasons this is the right shape:

- No test asserts the inspector title text today (verified by grep across
  `tests/` and `src/`), so nothing is repointed under duress.
- The accessible name survives in the capture tree — moving it, not deleting
  it — which keeps T0's exact node count **neutral**: one label leaves the
  body, one arrives in the title bar. A net-zero count is a much sharper
  signal than a count that moved for two reasons at once.
- The title bar is rendered *outside* the `.cached()` wrapper (only
  `active-panel` → `tab-content` is cached, `tab_panel.rs:851-861`), so an
  a11y node emitted from `title()` is not exposed to the caching risk that
  §8 tracks for the bodies.

Per A5's rule, `a11y_label()` **pushes** a node rather than setting an
attribute, so this must be the only label on that element — it is.

Both are registered in `panels::register_panels`, whose builders keep B5's
shell-less-degradation contract (`WeakEntity::new_invalid()` → empty render,
never `unimplemented!()`).

---

## 5. Visibility: one source of truth, and where it is synced

Today `inspector_panel_visible` / `chart_panel_visible` are `WorkspaceShell`
bools driving the fixed docks. They stay **the** truth; the dock derives from
them. The master plan's warning was against keeping *two* independent stores
alive, and this keeps one — it just inverts which side derives.

Consequences, all of them deliberate:

- Session persistence is untouched. `inspector_panel_visible` keeps its
  existing `persist_dock_ui` / restore path and `chart_panel_visible` keeps
  being deliberately not persisted (`window.rs:2522` restores it as `false`).
  **Session stays v10**, exactly as the master plan requires.
- The View-menu toggles are untouched.
- Every `#[cfg(feature = "a11y-capture")]` shim that writes a bool
  (`chart_bind_for_test`, `seed_lineage_target_for_test`, …) keeps working with
  no change, because the dock reads the bool rather than the other way round.

**Where the sync happens.** `Dock::set_open` and `Dock::set_size` both need
`&mut Window`, and the toggles (`toggle_chart_panel(&mut self, cx)` and the
inspector's menu listener) do not have one. Rather than thread a `Window`
through every call site, the shell reconciles at the top of `render` — the same
place B5 builds the dock, and the only place a `Window` is guaranteed:

```
fn sync_right_dock(&mut self, window, cx) {
    let want = (self.inspector_panel_visible, self.chart_panel_visible);
    if want == self.right_dock_state { return; }   // one tuple compare per frame
    self.right_dock_state = want;
    // size tracks the visible set, then open/close
}
```

This is what makes the test shims work for free: they write the bool, the next
frame reconciles. `Dock::set_open` defers its `set_collapsed` through
`cx.defer_in` (`dock.rs:262`), which is fine — it lands on the following frame.

**Width tracks the visible set:** inspector alone 288, charts alone 560, both
848 — today's widths exactly. A user's manual resize survives until they toggle
a panel, at which point it is recomputed. Remembering a resize across toggles
needs the `dock_layout` blob, which is B9.

---

## 6. Chart export moves to the title bar

`render_chart_toolbar`'s two export buttons move into
`ChartsPanel::toolbar_buttons()` as short-labelled ghost buttons, `"PNG"` and
`"SVG"`. The chart-type cycle, the per-axis cycles and the **Save** button stay
in the body: Save carries a real `disabled` state whose affordance reads
correctly at body size, and the axis cycles carry long interpolated labels
(`"X: order_date"`) that a 30px `text_ellipsis` bar would truncate to noise.

Because of fact 7 the two buttons lose their tab stops, and chart export has no
menu item and no palette entry today (`src/actions/registry.rs` has zero chart
descriptors). Shipping that as-is would make chart export mouse-only in a
redesign whose entire A-series was about accessibility. So the move is paired
with two new registry descriptors:

| id | title | group |
|---|---|---|
| `chart.export.png` | `Export Chart as PNG` | `ActionGroup::File` |
| `chart.export.svg` | `Export Chart as SVG` | `ActionGroup::File` |

Registered in `actions::builtin::register_all` alongside the other view
actions, each dispatching through `focused_workspace(app)` →
`ws.export_chart(png, cx)` (which needs its visibility widened from `fn` to
`pub(crate)`). `ActionGroup` does not affect palette ranking — `visible_items`
sorts on score then title — so no new group variant is needed.

They are **not** added to `command_palette::HIDDEN`. That list is for actions
that are dead by construction (a `tracing` breadcrumb for a body, or an
argument a fuzzy search box cannot supply). These two work whenever a chart is
rendered and no-op otherwise, which is exactly `view.copy`'s situation — also
registered, also visible, also a no-op without a selection.

---

## 7. Shell changes

- Two extractions, mirroring B5's `render_grid_body`:
  `render_inspector_body(&mut self, cx) -> AnyElement` from the `.w_72()`
  block (`window.rs:7312-7321`) and `render_charts_body(&mut self, cx)` from
  the `.w(px(560.))` block (`:7323-7336`), each **minus** its sizing/border
  wrapper, which the dock now owns. Verbatim except for the two changes this
  design already justifies: the inspector's own title row moves into
  `InspectorPanel::title()` (§4.1) and the two chart export buttons move into
  `ChartsPanel::toolbar_buttons()` (§6).
- Both `.children(..)` blocks are deleted from the body row.
- New fields: `inspector_panel` / `charts_panel` (`Option<Entity<..>>`, built
  in the same lazy block as the dock) and `right_dock_state: (bool, bool)`.
- `export_chart` becomes `pub(crate)`.

---

## 8. Risks, and how each is discharged

| Risk | Discharge |
|---|---|
| **`.cached()` vs the single-frame a11y capture.** The master plan's top risk for B5-B8, retired by B5 only for the no-wrapper path. Note the likely failure mode here is nodes going **missing**, not duplicating — and the pre-designed generation counter (`a11y/mod.rs:24`) fixes duplicates, not omissions. | **T0 gate.** Re-run `a11y_spike` with the right dock mounted and open and report its exact node count (8 as of B5) either way. Fix in-slice: counter if duplicates, cache invalidation on the capture frame if omissions. |
| **`.tab_group()` reorders Tab traversal** (B1: gpui tab groups reorder, never contain). Neither panel has a single `focus_stop` or `tab_index` today, and no test references their element ids — so nothing existing can catch a regression. | T0 measures Tab order before/after; a new nav test asserts the chart toolbar's body buttons stay reachable. No new focus stops — that is B7's declared slice, and adding them here would leave two suspects behind any red result. |
| `title()` / `visible()` run every frame | Both are a weak upgrade plus a field read; `title()` returns a static i18n string, never a `format!`. |
| Chart `RenderImage` (main-thread, `!Send`) now inside a cached element | Covered by the existing chart integration tests, which must pass untouched. |
| Nested scroll around the panels | Not a risk here: neither `inspector/panel.rs` nor `charts/panel.rs` contains any `overflow_*_scroll` today, so the `TabPanel` wrapper is a pure gain — inspector content that used to clip now scrolls. (This *was* a real hazard for B5's virtualized `Table`, which is why the center stayed a bare panel.) |
| Narrow windows | Pre-existing: 288+560 was already fixed-width. The dock adds `overflow_hidden`, which B5 already recorded as the better behaviour. |

**Not a gate:** `benches/grid_scroll.rs`. B5 settled that it is a `render_cell`
watchdog that never builds a `Window`, a `WorkspaceShell` or the `Table`
widget, and wrote that into the bench's own module doc. Verify the post-merge
run at **step** level (a green job can mask a skipped bench), but read no
meaning into the number.

**Not a worry:** CI disk. `c4b3aba` took macOS job-end from 4.1 Gi to 17 Gi.
B6 may add test binaries freely.

---

## 9. Test plan

- **T0 gate (blocking, before any production code):** node-count and Tab-order
  measurements above, committed as findings.
- **`tests/right_dock.rs`** (new binary): dock mounts with a right dock; both
  panels render their real content through the a11y capture; visibility follows
  the bools in both directions; width tracks the visible set; the charts title
  bar carries PNG/SVG; both `PANEL_NAME`s frozen.
- **Palette:** the two new ids appear in `visible_items` and are absent from
  `HIDDEN`.
- **Regression oracle:** `chart_uat_window.rs`, `chart_panel_wiring.rs`,
  `a11y_content.rs` and the nav suite pass **unmodified**. If one needs
  editing, that is a finding to report, not a diff to write quietly.
- Non-vacuity proven in both directions for every new assertion, per the
  standing rule.

**Local gate:** `cargo fmt --check`; `cargo clippy --workspace --all-targets -D
warnings`; `-p dat0-app` under {plain, `a11y-capture`, `a11y-capture,gallery`};
`style_lint` ratchet unchanged at `[("window.rs", 1)]`; `cargo build -p
dat0-app --bin dat0` **and boot it** with a fresh `DAT0_CONFIG_DIR`, diffing
the log against a `main` build — that is how B5's first-run tour regression was
found, and this slice touches the shell.

---

## 10. Owed human glance

B6 is visibly different: two 30px title bars with ⋯ menus, a resizable divider,
export buttons relocated into the charts title bar, and scrolling where the
inspector used to clip. Needs a look in **all three themes**, high contrast
most of all — a 30px title bar and a ghost ⋯ button are exactly the surfaces
that have historically ignored HC.

Carried in from earlier slices, still owed and not blocking: B5's
diff-the-pixels pass plus its narrow-window and file-drop specifics, and B4's
palette glance.
