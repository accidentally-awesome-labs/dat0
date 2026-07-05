# UAT Slice 6 — Keyboard-nav / focus reachability (design)

> **Date:** 2026-07-05 · **Branch:** `uat-keyboard-nav` off `main` (`37aeeea`)
> Sixth slice of the manual-UAT-backlog retirement via the AccessKit + async GPUI
> harness (Slices 1 Settings, 2 Update/About, 3 Charts, 4 Crash/Report, 5 MotherDuck
> merged). First **capability-enabler** slice: it builds a reusable test capability
> (a focus oracle) AND ships a real production a11y fix. Covers **P10b §10 keyboard
> reachability** (`docs/plans/2026-06-23-dat0-p10b-uat.md` §10.1–10.6; `docs/a11y.md`
> A1/A2).

## Problem

P10b left the entire **keyboard-only reachability** half of the a11y gate owed to
human-UAT (`docs/a11y.md:18-19` A1 "keyboard nav reaches every interactive element" and
A2 "focus indicators" both **UAT-pending**; the granular table `docs/a11y.md:94-227` marks
essentially every focus-chain item pending). The P10b UAT checklist §10.1–10.7 asks a human
to confirm Tab lands on each interactive element and paints a visible ring.

Two problems compound:

1. **The harness has zero keyboard capability.** Every existing test drives the UI by
   mouse (`support/mod.rs:176-184` `debug_bounds`→`simulate_click`). There is no keyboard
   simulation and — critically — **no way to assert which element is focused**. gpui 0.2.2
   exposes `window.focused(cx) -> Option<FocusHandle>` (an *opaque* handle) but no reverse
   map to a label/id, and the a11y emitter hardcodes `TreeUpdate.focus = NodeId(0)`
   (`a11y/mod.rs:130`). Neither focus oracle can currently name the focused element.

2. **Several interactive surfaces are not keyboard-reachable at all.** The Home hero
   buttons (`empty_state.rs:166-334` — take-tour, open-demo, 3 sample cards, open-file,
   recents rows) and the Settings DIY toggles (`settings_ui/panel.rs:95-121` `toggle_row`,
   shared by 3 toggles) are raw `div().id().on_click()` with **no `tab_index`, no
   `FocusHandle`, no keyboard activation**. `grep tab_index / key_context` over `dat0-app`
   src is empty. The workspace shell has `.track_focus(&self.focus_handle)` (`window.rs:6422`)
   but **no `tab_index`**, so Tab does not even land on the grid region. So §10.1/§10.6 as
   written **fail today** — this is a genuine product a11y gap, not just missing tests.

## Key reframing (why this is a capability-enabler, not a content slice)

Slices 1–5 asserted rendered *content* by seaming `.a11y_label` onto elements that were
already inert in release. This slice is different on both axes:

- **New test capability (the oracle).** The reusable win is a *focus oracle*: after
  `simulate_keystrokes("tab")`, name the focused element by label. Built entirely from a
  test-only `FocusHandle↔label` side-map plus `window.focused()` — **no dependency on
  kittest's focus API** and no gpui fork. This unlocks the whole §10 cluster for this and
  future slices.

- **Real production fix (chosen scope).** User-confirmed 2026-07-05: **also fix production
  `tab_index`** so §10.1 actually passes, not just file the gap. So — unlike `.a11y`, which
  is an identity no-op in release — the new `focusable()` wiring **ships in the release
  binary**. It makes the hero buttons and DIY toggles genuinely keyboard-reachable and
  operable. This owes a human focus-**ring** visual glance and carries extra review weight.

The honest boundary: we automate **reachability** ("Tab lands on element X, Enter fires its
handler"). The **2px ring pixel appearance + WCAG contrast** (§11.4, A2) is Gap 1 (no pixels
under `TestPlatform`) and **stays human**.

## Scope

**IN**
- Harness focus oracle (`focused_label()`, `tab()`/`shift_tab()` combinators).
- Production `focusable()` helper (release-real: `tab_index` + `track_focus` +
  Enter/Space activation + focus-ring style).
- Apply to: **Home hero** buttons, **Settings `toggle_row`** (fix-once → 3 toggles),
  **grid shell** (Tab-reachability).
- Tests: hero Tab-cycle + Enter-activation; Settings sidebar/inputs/toggles reachable +
  toggle Enter/Space persist; grid Tab-reach + arrow-nav via `SelectionModel`.

**DEFER** (own follow-on slice — mixed mechanisms, keep this slice mergeable)
- Catalog-tree deep arrow-nav, AI prompt input (§10.7), SQL-editor internal tab-stops,
  cell-editor Tab progression (§10.6 tail).

**STAYS HUMAN** (Gap 1 / owed glance)
- Focus-ring pixel appearance + WCAG contrast (§11.4, A2). We assert reachability, never
  ring pixels.

Already tab stops (no change): Settings inputs (gpui-component `Input`), theme/log-level/
Reset/MD/AI **Buttons** (gpui-component `Button`), dialog buttons — gpui-component registers
`tab_index` in its own render, so Tab already traverses them.

## Approach

### The focus oracle (test-only, `a11y-capture`)

The existing design already keeps content + geometry in lockstep by keying both on the
**same `&'static str` id** (`a11y/mod.rs:12-13`). The oracle extends that one principle to
focus — everything joins on the static id, so `focusable()` and `.a11y()` stay orthogonal
(pure focus wiring vs. pure content) and there is no order-dependent "attach to the last
node" fragility:

- Under `a11y-capture`, `focusable(id, &fh, ...)` records `fh.id() → id` into a **second
  thread-local side-map** (`FocusId → &'static str`). It does **not** push an a11y node;
  the `.a11y(id, …)` call on the same element still pushes the `Captured { role, text,
  click_id: Some(id) }` node (label + id).
- `A11ySnapshot::focused_label()` resolves: `window.focused(cx)` → `FocusId` → side-map →
  `&'static str id` → the `Captured` whose `click_id == id` → its `text`. Pure join on the
  static id; **no dependency on whether kittest 0.3.0 surfaces focus**.
- For hygiene / a future OS adapter, `take_tree_update()` also sets `TreeUpdate.focus =
  NodeId(index+1)` of that node (replacing the hardcoded `NodeId(0)`) — but assertions read
  the side-map, not this field.
- Consequence: every `focusable()` element must also carry a matching `.a11y(id, …)` with
  the same id (the label source). A `focusable()` with no `.a11y` twin is a bug the spike
  will surface (focused → `None` label).
- New `A11ySnapshot` API in `tests/support/mod.rs`:
  - `focused_label(&self) -> Option<String>` — the label of the focused node.
  - `tab(&mut self, cx)` / `shift_tab(&mut self, cx)` — `vcx.simulate_keystrokes("tab" /
    "shift-tab")` → settle bracket (`refresh` + `run_until_parked`) → recapture. (Tab
    routing is provided by the already-mounted `gpui_component::Root` binding
    `KeyBinding::new("tab", Tab, Some("Root"))` → `window.focus_next()`.)
  - optional `assert_focus(label)` sugar.

### The production `focusable()` helper (release-real)

A chainable element helper — parallel to `.a11y` but **NOT** an identity no-op in release —
that turns a raw interactive `div` into a real keyboard control:

```
let activate = cx.listener(..);              // ONE handler, shared below
div().id(id)
    .focusable(id, &focus_handle, n, activate.clone())  // NEW (release-real)
    .a11y(id, AccessRole::Button, label)                 // existing (label source for oracle)
    .child(..)
    .on_click(activate)                                  // same handler as Enter/Space
```

`focusable()` chains (exact gpui/gpui-component method names proven by the T0 spike):
`.tab_index(n)` (join the tab ring) + `.track_focus(&fh)` (correlate + enable focus query) +
`on_key_down` for **Enter/Space → invoke the same handler `on_click` uses** (the site builds
the handler once and passes it to both, so keyboard and mouse activation cannot drift) +
focus-state ring style. Under `a11y-capture` it additionally records `fh.id() → id` into the
oracle side-map; in a plain release build it emits the focus wiring only (no side-map).

### Stable FocusHandles (the #1 risk)

A `FocusHandle` must be **created once and retained on the persistent entity**, not minted
per-render. So:
- **`WorkspaceShell`** (persistent `Entity`) owns the hero + grid focus handles (a small
  `Vec<FocusHandle>` or an id-keyed map), passed down into the transient `EmptyState` at
  render time. `EmptyState` is rebuilt each render — it must **not** own the handles.
- **`SettingsPanel`** (persistent `Entity`, already holds the `InputState` entities) owns
  the 3 toggle focus handles.

### Tests — `crates/dat0-app/tests/keyboard_nav.rs` (a11y-capture, full mounts)

Mount helpers copied per-binary (precedent: `chart_uat_window.rs`, `motherduck_window.rs`).

1. **Home hero** (full shell, empty session): `tab()` N times, assert `focused_label()`
   walks the hero buttons in DOM order (§10.1 reachability); focus the demo/tour button,
   `simulate_keystrokes("enter")`, assert its handler fired (e.g. tour opens / demo import
   begins — reuse an existing observable from `onboarding_gpui.rs`) (operability).
2. **Settings** (`SettingsPanel` mount, Slice-1 pattern): `tab()` reaches the 9-section
   sidebar + Profile name/email inputs + 3 DIY toggles (`focused_label()` per stop, §10.2–
   10.5); focus a toggle, Enter/Space, assert the settings.toml persist (reuse Slice-1
   `DAT0_CONFIG_DIR`/`#[serial]` seam only if the toggle writes `config_dir()`); Reset
   button reachable → Tab to dialog Cancel.
3. **Grid** (full shell + seeded data, Slice-3/5 seed pattern): `tab()` reaches the grid
   shell (`focused_label()` or shell focus assert); arrow keystrokes move
   `SelectionModel.active` — asserted via `SelectionModel` state, **its real oracle**, since
   the grid ring is `is_active`-keyed (`grid/mod.rs:562-569`), decoupled from gpui focus.

## Seams (production edits)

- `src/a11y/mod.rs` — the oracle `FocusId → &'static str` thread-local side-map + its
  `reset`; `take_tree_update` sets `TreeUpdate.focus` from it; the `focusable()` helper
  (release-real branch records focus wiring + capture branch also fills the side-map). Note
  the release helper is **not** a no-op — add it to the release `stub` module as a *working*
  implementation (real `tab_index`/`track_focus`/key wiring), not an identity pass-through.
- `src/empty_state.rs` — hero buttons chain `.focusable(...)`; accept focus handles from the
  shell (signature change on the hero render path).
- `src/window.rs` — `WorkspaceShell` owns/creates the hero + grid `FocusHandle`s; grid shell
  gains `.tab_index(...)`; pass handles into `EmptyState`.
- `src/settings_ui/panel.rs` — `toggle_row` chains `.focusable(...)`; `SettingsPanel` owns
  the 3 toggle handles; Enter/Space activation wired.
- `tests/support/mod.rs` — `A11ySnapshot::{focused_label, tab, shift_tab, assert_focus}`.
- `tests/keyboard_nav.rs` — new binary.

## Risks / T0 spike gate

Per the Slice-3 lesson (*spike every asserted surface*), the T0 spike is a **hard gate** and
must prove ALL of the following before any breadth build; if any fails → STOP and re-scope:

1. A `focusable()` `div` is Tab-reachable via gpui-component `Root`'s Tab binding under
   `TestPlatform` (needs `gpui_component::init`, as the harness already does).
2. Enter/Space on a focused `focusable()` div fires the activation handler.
3. A focus-state ring paints on focus (render wiring compiles + applies; pixels stay human).
4. `window.focused(cx)` returns a `FocusHandle` whose `id()` equals the stored handle's.
5. `focused_label()` reads the focused element's label back through the side-map.
6. A `FocusHandle` created once on the persistent entity (`WorkspaceShell`/`SettingsPanel`)
   survives re-render and keeps stable tab order (the transient-`EmptyState` trap).

Secondary risks: (a) hero DOM order vs. visual/tab order must match the asserted sequence;
(b) `focus_next` walks `rendered_frame.tab_stops`, so the settle bracket
(`refresh`+`run_until_parked`) is mandatory before each focus query; (c) the grid's Tab-reach
vs. arrow-nav are two different mechanisms — do not conflate (grid uses `SelectionModel`).

## Deps / CI / footprint

- **Zero new deps** — gpui + gpui-component already present; oracle uses our own map.
  Cargo.lock / NOTICE unchanged; **D-015 stays open** (still no OS AccessKit adapter).
- **Release footprint is NON-zero this slice** (the `focusable()` production wiring) — the
  first slice to ship real release code. Owed: one **human focus-ring visual glance** across
  hero + Settings toggles in each theme (joins the standing About/Charts/Settings glances).
- New test binary auto-runs under `cargo test --workspace` via the self-dev-dep feature
  unification (`a11y-capture` on), same as Slices 1–5.
- CI: standard gate (fmt / clippy -D warnings / workspace tests / i18n / dep-guards); the
  push-to-main macOS grid-scroll bench is unaffected (WATCH the post-merge main run per
  standing practice).

## What this deliberately does NOT do

- No focus-ring pixel/contrast assertion (Gap 1 → human).
- No Catalog/AI/SQL-editor/cell-editor internal nav (deferred slice).
- No OS AccessKit adapter (D-015 unchanged).
- No gpui / gpui-component fork or version bump (pinned 0.2.2 / 0.5.1).
