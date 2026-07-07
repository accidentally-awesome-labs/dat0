# UAT Slice — Recents-list keyboard nav (design)

> **Date:** 2026-07-06 · **Branch:** `uat-recents-nav` off `main` (`d950b49`)
> First of the **deferred internal keyboard-nav slices** (the Slice-6 carve-out).
> Slices 1–7 merged; the capability-enabler cluster (Slice 6 focus oracle +
> `focus_stop`, Slice 7 crash-e2e) is complete. This slice reuses that harness to
> add keyboard nav to the **recents list** on the Home hero — the dynamic-list gap
> that was explicitly OUT of Slice 6 (`empty_state.rs` recents rows: `hero-recent-{i}`).

## Problem

Slice 6 made the Home hero's *fixed-id* controls keyboard-reachable — take-tour,
open-demo, the 3 sample cards, and both open-file buttons all carry `focus_stop`
(`empty_state.rs`). It explicitly left the **recents rows** unreachable: they are a
dynamic loop of plain `div().id("hero-recent-{i}").child(label).on_click(..)`
(`empty_state.rs::recents_column`) with **no `tab_index`, no `FocusHandle`, no
keyboard activation, no focus ring**. A keyboard-only user who has opened files
before (the common returning-user path) lands on Home and **cannot reach any of
their up-to-25 recent entries** without a mouse. This is a genuine product a11y
gap (P10b `docs/a11y.md` A1 "keyboard nav reaches every interactive element"),
not just missing test coverage.

The recents list is capped at 25 entries (`recents/mod.rs::MAX_ENTRIES`). Making
each row its own tab stop would create up to 25 tab stops to traverse before
anything below the list — Tab fatigue, and it would require a stable
`FocusHandle` **per dynamic row** (the hard part the backlog flagged). The
idiomatic model for a list of this size is a **listbox**: one tab stop, arrow
keys move a selection within it.

## Approach (decided in brainstorming)

**Arrow-within-list nav**, reusing the *grid's* `SelectionModel` shape rather than
the *hero's* per-element `focus_stop` shape:

- The recents list is **one** tab stop (`focus_stop` on the container).
- ↑/↓ move an **active-index** (`usize`) held on the persistent `WorkspaceShell`.
- Enter/Space opens the active row.
- A focus ring marks the active row (decoupled from gpui focus, like the grid's
  `is_active` cell ring).

This **sidesteps the per-dynamic-row `FocusHandle` problem entirely** — there is
exactly one handle (the container) — and is the canonical ARIA listbox UX. It
forges the list-nav pattern the Catalog slice will reuse next.

Rejected alternative — **Tab-through each row**: reuses `focus_stop` directly but
needs a dynamic-handle keying scheme (index or path) and creates up to 25 tab
stops. Worse UX; more machinery. Not taken.

## Production change (ships in release — genuine a11y fix)

Touches `empty_state.rs` (render) and `window.rs` (state + accessors).

### State (on the persistent `WorkspaceShell`, never the transient `EmptyState`)

- **`recents_active: usize`** — NEW field. Defaults `0`. Clamped `min(active,
  len-1)` at render so a shrinking recents list can never dangle the index.
- **Container `FocusHandle`** — **reuse** the existing
  `hero_focus: HashMap<&'static str, FocusHandle>` + `hero_focus_handle(id, cx)`
  get-or-insert, keyed by the fixed `"recents-list"` id. No new handle field; the
  handle is registered in `WorkspaceShell::render` alongside the other hero
  handles and reached via `HeroHandles::get("recents-list")`.

### Render (`recents_column`)

```
container div
  .focus_stop("recents-list", hero.get("recents-list"), 0,   // tab_index 0 = natural order, like every hero stop
              on_activate = |ws, _ev, window, cx| {
                  if let Some(e) = active_recent(&entries, ws.recents_active) {
                      ws.open_recent_entry(e, cx);
                  }
              })                                   // Enter/Space  (focus_stop's own handler)
  .on_key_down(cx.listener(|ws, ev: &KeyDownEvent, _window, cx| {
      match ev.keystroke.key.as_str() {
          "down" => ws.recents_active = (ws.recents_active + 1).min(len - 1),
          "up"   => ws.recents_active = ws.recents_active.saturating_sub(1),
          _ => return,
      }
      cx.notify();
  }))                                              // ↑/↓  (chained 2nd listener)
  .a11y("recents-list", Button, t("hero.recent_label"))   // ONE oracle twin on the CONTAINER
                                                           // (SAME static id as focus_stop; Slice-6 rule)
  for (i, entry) in entries:
      row div .id("hero-recent-{i}")
              .when(i == ws.recents_active, |r| r.border_2().border_color(FOCUS_RING))  // active ring
              .on_click(|ws| ws.open_recent_entry(entry.clone(), cx))  // rows: NO per-row a11y (minimises R3)
```

**a11y twin placement.** Only the **container** carries `.a11y("recents-list",
Button, t("hero.recent_label"))` — the SAME `&'static str` id as its `focus_stop`
(Slice-6 rule: every `focus_stop` needs a matching `.a11y` twin with the same id
so the focus oracle can name it). The oracle returns the node's **label text**
(a11y/mod.rs `focused_label` joins the focused handle's id → the FRAME node with
that `click_id` → its `text`), so the Tab-reaches-list assertion compares against
the **label text** `t("hero.recent_label")`, not the id. Rows carry no a11y node
(only the ring + `on_click`), which keeps the new-node footprint at one
conditionally-rendered container node → minimal cross-binary drift (R3).

**Composition.** `focus_stop` supplies tab-stop metadata (on the FocusHandle),
`track_focus`, the ring, and the Enter/Space→activate handler. gpui's
`on_key_down` **pushes** to a listener Vec, so a chained second `on_key_down`
adds ↑/↓ without displacing focus_stop's handler — both fire. One container
handle; no per-row handles.

**Single-source-of-truth** (Slice-6 rule): a row's `on_click` and Enter both open
via `open_recent_entry` — mouse and keyboard cannot drift. Enter routes through
`active_recent(entries, recents_active)`; `on_click` passes the row's own entry
directly (equivalent).

**Ring.** On the active row only, decoupled from gpui focus (grid `is_active`
idiom). If `window` is reachable in `recents_column`, gate the ring on
`hero.get("recents-list").is_focused(window)` so it shows only while the list is
being navigated; otherwise render it unconditionally. **T0 resolves** which
(render-signature detail). This is the owed-glance pixel surface.

### Pure activation seam (`empty_state.rs` or a small `recents` helper)

```rust
/// The recent entry the active-index currently selects, or None if the list is
/// empty or the index is out of range. Pure — unit-tested without GPUI.
fn active_recent(entries: &[RecentEntry], active: usize) -> Option<RecentEntry>
```

Mirrors Slice-4's `resolve_relaunch_action` pure-seam pattern. Makes the
*selection* logic unit-testable and keeps the real file-open
(`open_recent_entry` → `open_workspace_at` / `open_package_at`, heavy free-fns
taking `cx`) out of the automated path.

## Test harness & coverage

New tests in `crates/dat0-app/tests/recents_nav.rs` (a11y-capture, full
`WorkspaceShell` mount via the `onboarding_gpui.rs` helpers, per Slice 3/5/6).

### Accessors / seeding

- **`recents_active(&self) -> usize`** (`#[cfg(feature = "a11y-capture")]`) on
  `WorkspaceShell` — mirrors grid `SelectionModel::active()`; the arrow-nav oracle.
- **Seeding needs NO new prod shim.** `recents_column` reads
  `config_dir()/recents.json`; `keyboard_nav.rs` already establishes the
  `set_config_dir(tempdir)` + `#[serial]` pattern (writes `DAT0_CONFIG_DIR`). The
  test writes a real `recents.json` before mounting via the public
  `Recents::with_path(cfg.join("recents.json")).push(RecentEntry::…)` API — paths
  need not exist (the row only *displays* `path().display()`; we never drive the
  open). So R2 resolves to "reuse the file's existing `DAT0_CONFIG_DIR`+`#[serial]`
  seam"; no `seed_recents_for_test` shim.

### Assertions

1. **Tab reaches the list.** With ≥1 recent seeded, `press_tab` (bounded loop, as
   keyboard_nav.rs:278 does) until `focused_label() == Some(t("hero.recent_label"))`
   — the container's a11y **label text**. Fails loudly if the list is never the
   focused stop.
2. **↓/↑ move the active-index (drives the ring).** Seed ≥2 recents. Focus the
   list, `simulate_keystrokes("down")` → `recents_active() == 1`; `"up"` → back to
   `0`; assert clamp at both ends (up at 0 stays 0; down at `len-1` stays `len-1`).
   `recents_active()` is the single observable — it drives the active-row ring.
3. **Activation wired.** Unit-test `active_recent(entries, i)`: correct entry for
   in-range `i`, `None` for empty list, `None` for out-of-range (`entries.get(i)`).
   The real file-open is **not** driven (heavy round-trip; stays human).

The ring **pixels** are not asserted (Gap 1, human) — the T0 spike proves the
`.when(active)` ring branch renders without panic (Slice-6 criterion-3 idiom),
and `recents_active()` proves the state that drives it.

### Cut lines (honest)

- Real file-open on Enter (`open_workspace_at` / `open_package_at`) — **stays
  human** (heavy round-trip, needs real files + dispatcher; consistent with "real
  external round-trips stay human").
- Focus-ring **pixels + WCAG ≥3:1 contrast** — **stays human** (Gap 1); joins the
  standing About / Charts / Settings / Slice-6 ring glances.
- Recents-**empty** case — no list is rendered (the sample picker shows instead,
  already `focus_stop`'d in Slice 6); nothing to assert.

## Seam / release cost

`focus_stop`, the arrow `on_key_down`, and the `recents_active` field all ship
**unconditionally** (genuine a11y fix, like Slice 6 — not a test-only no-op).
Only `record_focus_id` (inside `focus_stop`) and the accessors/shims are
`#[cfg(a11y-capture)]`. The per-row `.a11y` twin chains onto the existing row div
(no new wrapper). **Owed human glance = the active-row ring pixels** (already
expected for any ring-bearing surface).

## Risks / T0 spike (HARD GATE — must exercise EVERY asserted surface)

Slice-3/6 lesson: a T0 spike only proves the surfaces it touches. This T0 must
drive Tab→list, ↓→index-move, ring-render, and the accessor before any T1 code.

- **R1 — chained `on_key_down` under `TestPlatform`.** Does a second, bare
  `on_key_down` coexist with `focus_stop`'s Enter/Space handler, and does
  `simulate_keystrokes("down"/"up")` route to it? (The grid uses a keymap, not a
  bare `on_key_down`.) T0 must show ArrowDown actually mutates `recents_active`.
  *If it doesn't route:* fall back to a single unified `on_key_down` that handles
  ↑/↓/Enter/Space together (dropping `focus_stop`'s activate arm, keeping its
  tab-stop + ring), or route via the shell's existing key handler.
- **R2 — hermeticity of the `recents.json` read.** Resolved: reuse
  `keyboard_nav.rs`'s `set_config_dir(tempdir)`+`#[serial]` seam and write a real
  `recents.json` via `Recents::with_path(..).push(..)` before mount. T0 confirms
  the seeded rows render (`recents_empty == false`).
- **R3 — cross-binary frame-count drift** (Slice-6 lesson). Adding even the ONE
  container `.a11y` node can shift another binary's exact node-count assertion
  (only `a11y_spike.rs` asserts exact counts; and its scene has empty recents so
  the conditional node likely never renders there → probably zero drift). The
  controller `cargo test --workspace --no-fail-fast` gate is the backstop; the
  focused `--test recents_nav` cannot see it.
- **R4 — tab-index ordering.** The list stop uses `tab_index 0` (natural order,
  like every hero stop); T0 confirms it slots sanely into the `focused_label` Tab
  sequence and isn't skipped.

## Build cadence (SDD)

T0 spike **hard gate** (R1–R4 empirically resolved) → T1 production nav
(`recents_active` + `focus_stop` container + arrow handler + `active_recent` seam)
→ T2 tests (accessors/shim + the 4 assertions) → per-task spec+quality reviews →
final opus whole-branch review → green **both** platforms → squash → **watch the
post-merge main run** (the macOS grid-scroll bench is push-to-main-only → can
redden main silently). Zero new deps expected (D-015 stays open).
