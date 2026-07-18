# UAT — inline cell-editor behavioral coverage (design)

> **Date:** 2026-07-18 · **Branch:** `uat-cell-editor-nav` off `main` (`573b691`)
> Deferred keyboard-nav carve-out #4 (after Slice 6 keyboard-reachability,
> recents-nav, catalog-tree, and AI-config-nav). **Unlike the prior four
> slices this ships NO new production a11y** — the cell-editor's keyboard
> behavior is already fully wired. This slice closes the long-standing
> **coverage** gap on the inline-edit flow (UAT-owed since P4b/P4c T15,
> deferred as PD-013). Covers the **cell-editor** surface flagged in the
> kbd-nav backlog.

## Problem

The kbd-nav backlog lists the cell-editor as "mostly shipped already; gap =
coverage + wiring grid-Enter→edit." **On inspection against `573b691` the
grid-Enter→edit trigger is already wired** (`window.rs:6436`), so the slice
collapses to almost pure coverage.

What is already SHIPPED (production, verified):

- **`CellEditor` entity** (`src/grid/cell_editor.rs`) — type-aware inline
  editor: text `Input` for String/Numeric/Date/Timestamp, `Select` for Bool.
  Real `FocusHandle` + focus-on-mount. `Enter` →
  `CellEditorEvent::CommitAndMove(value, Down)`; `Blur` → `Commit`; bool
  `Select` confirm → `Commit`; Cancel button → `Cancel`. Pure `parse_text`
  rejects invalid input (un-parseable numeric, malformed date/timestamp).
- **`begin_cell_edit` / `commit_cell_edit` / `commit_cell_edit_and_advance`**
  (`src/grid/edit_ops.rs`) — mount editor typed off the active column, subscribe
  (stored in `cell_editor_sub`), route `Commit`→commit, `CommitAndMove`→advance
  one row down + re-open, `Cancel`→teardown. Commit drives the engine round-trip
  via `spawn_rebind` (the edit is a display-overlay compiled into the view SQL,
  `SELECT * REPLACE (CASE …)`, **not** a base-table mutation).
- **Grid key handler FULLY wired** (`window.rs:6422-6498`): `Enter`/`F2` →
  `begin_cell_edit`; `Escape`-with-editor → cancel (cursor stays); arrows →
  `SelectionModel`; clipboard/bulk keys.
- `Tab`→right is **PD-020, deliberately NOT shipped** (`Input` swallows Tab; a
  wrapper `on_key_down` would steal keystrokes). The editor mounts **blank**
  (`begin_cell_edit` uses `CellEditor::new`, not `with_seed`).

What is already TESTED:

- `tests/cell_editor_smoke.rs` — **construction-only** (no `Window`):
  subscription-storage regression guard + `focus_handle` accessor exists.
  Explicitly defers behavior to "manual UAT T15 / PD-013".
- `cell_editor.rs` in-module — 5 pure `parse_text` units.
- `tests/edit_lifecycle.rs` / `tests/edit_restore_e2e.rs` — **engine-level**
  `CellEdit`→ViewModel→engine round-trips. NOT the UI editor.
- `tests/keyboard_nav.rs` grid test — proves Tab→grid shell + arrow→active cell
  via `grid_active_cell_for_test()`. **Never touches the editor.**

**The gap:** no test drives the inline-edit flow end-to-end — grid `Enter`/`F2`
→ editor mounts → type → `Enter` → commit+advance+re-open → `Escape` cancel →
invalid-reject → bool path. This is a *data-mutation* surface. The untested part
is specifically the **UI→CellEdit bridge** (a typed value + `Enter` produces the
right edit against the right row/col) and the **keystroke wiring**; the
engine-level round-trip is already covered by `edit_restore_e2e`.

## Scope (locked in brainstorm — 3 user decisions)

1. **Coverage-only.** Zero / near-zero production behavior change (matches
   Slices 1–5). Only test-only `#[cfg(feature = "a11y-capture")]` accessors are
   added. No `.a11y` twin, no screen-reader name, no PD-020, no seed-from-cell.
2. **Assert depth = boundary + one round-trip.** Keystroke-drive the flows;
   assert mount / teardown / advance / invalid-reject / bool at the shell-state
   boundary. PLUS **one** representative numeric commit driven through the real
   engine round-trip and read back from the live data source — proving the
   UI→CellEdit→engine bridge once.
3. **Flows = baseline + {invalid-input rejection, bool→Select-confirm}.** Blur
   commit and F2-alias were **dropped**.

## Approach — real-keystroke drive + state-accessor assert (T0-gated)

Reuse `keyboard_nav`'s proven windowed grid harness. Drive via
`simulate_keystrokes` (grid `Enter` → mount; commit `Enter`) and assert via
test-only state accessors. A **T0 hard gate** proves the keystroke→commit path
before any test is written, with a documented fallback rung (an
`InputState::set_value` accessor to set the typed characters) because prior
slices (Settings, crash-report) found character-typing into a gpui-component
`Input` unreliable headlessly.

Rejected alternatives:

- **Emit-seam (pure-seam):** invoke `begin_cell_edit` + emit `CellEditorEvent`
  directly, assert routing. Robust but skips *exactly* the untested bridge (grid
  key wiring + `Input`→`PressEnter`→emit). Kept only as the T0 STOP-clause
  fallback, not the primary approach.
- **Full round-trip for every flow:** re-proves the engine bridge repeatedly;
  heaviest async-pump surface, most fragile.

## Seams (test-only — ZERO release footprint)

All additions are `#[cfg(feature = "a11y-capture")]`, in the existing accessor
block in `src/window.rs` (near `grid_active_cell_for_test`, ~line 6872) unless
noted. Release binary byte-identical → **no owed human glance** (ties the best
Slice-4/5 outcome).

- `cell_editor_open_for_test(&self) -> bool` — reads `self.cell_editor.is_some()`.
- `cell_editor_for_test(&self) -> Option<Entity<CellEditor>>` — clones the handle
  so a test can reach the inner `InputState` / `SelectState` (fallback drive rung
  + the bool `Select`).
- `cell_display_for_test(&self, row: usize, col: usize) -> Option<String>` —
  reads the committed value back off the **live** data source
  (`self.data_source.as_ref()?.cell_display…`) — the rendered/overlay value, not
  the base table.
- reuse existing `grid_active_cell_for_test() -> CellCoord`.
- **Fallback rung only:** a `#[cfg(feature = "a11y-capture")]` accessor on
  `CellEditor` to set its inner `InputState` value (delegates to
  `InputState::set_value`, the Settings-slice idiom) — added ONLY if T0 shows raw
  keystroke typing does not land characters.

## Harness — `crates/dat0-app/tests/cell_editor_nav.rs`

New windowed integration binary (`a11y-capture` feature, `#[serial]` for the
`config_dir` seam). Helpers copied per-binary (established convention). Recipe
adapted from `keyboard_nav::grid_tab_reach_then_arrow_moves_active_cell`:

1. `set_config_dir(tempdir)` → `ensure_dispatcher` → `enter_async_harness`
   (guard held to end-of-test) → `init_components` → `build_empty_session_in`.
2. `open_shell_window(session)` → `run_until_parked` → drain + **close the
   first-run auto-show tour dialog** (the mandatory baseline dance).
3. **Seed a typed table via CTAS** on the session's engine
   (`session.lock().engine`, `create_table` path à la `edit_lifecycle`) — ONE
   table with a **numeric column + a bool column** (e.g. `n INT, flag BOOL`,
   3+ rows). CTAS gives exact column types (numeric round-trip + invalid-reject
   use the numeric col; bool flow uses the bool col) and auto-injects
   `__dat0_rowid`, avoiding CSV type-sniffing.
4. `GridDataSource::new(engine, table)` → `shell.set_data_source` → notify →
   **pump-to-page-0** (loop `run_until_parked` + `drain_dispatcher` until
   `ds.cell_render(0,0).is_some()`) so the lazily-built `SelectionModel` exists.
5. Establish grid focus (click the window center → `#workspace-shell`
   `click_to_focus`, or `window.focus_next()` onto the registered grid tab stop —
   both proven in `keyboard_nav`).

## Tests (6 GPUI behavioral, `#[gpui::test] #[serial]`)

| # | Flow | Drive | Assert |
|---|------|-------|--------|
| 1 | Entry trigger | grid focused → `simulate_keystrokes("enter")` | `cell_editor_open_for_test()==true` |
| 2 | Commit + advance | type value + `enter` | `grid_active_cell_for_test().row` == before+1 AND `cell_editor_open_for_test()==true` (re-opened) |
| 3 | Numeric round-trip (**the one deep one**) | numeric col → type `42` + `enter` → pump | `cell_display_for_test(0,0)=="42"` |
| 4 | `Escape` cancel | open editor → `escape` | `cell_editor_open_for_test()==false` AND `grid_active_cell_for_test()` unchanged AND value unchanged |
| 5 | Invalid-input rejection | numeric col → type `abc` + `enter` | **no** advance AND `cell_editor_open_for_test()==true` (commit suppressed, editor stays) |
| 6 | Bool → `Select`-confirm | bool col → `enter` mounts `Select` → confirm | commit fires (editor gone / `cell_display_for_test` flips) — **T0-gated**, see below |

Tests 2 and 3 may share a body; kept distinct in the table for clarity. Test 1
proves the grid-key→`begin_cell_edit` wiring; 2 proves the
`Input`→`PressEnter`→`CommitAndMove`→advance+reopen bridge; 5 proves the
`parse_text` guard is wired end-to-end (not just the unit).

## Risks / T0 spike gate (HARD GATE — prove drive BEFORE building)

Spike, in one throwaway test, in order — each is a documented STOP-clause:

1. **Grid `Enter` → editor mounts.** `simulate_keystrokes("enter")` on the
   focused grid shell flips `cell_editor_open_for_test()` to `true`. Low risk
   (mirrors `keyboard_nav`'s grid `Down`). STOP → investigate focus/dispatch.
2. **Mounted editor value + `Enter` → advance.** After mount + `run_until_parked`
   (inner `InputState` focused on mount), a value + `enter` produces a
   `CommitAndMove` that advances the active cell.
   - Rung (a): raw `simulate_keystrokes("4","2","enter")` types + commits.
   - Rung (b) fallback: keystroke `enter` for the commit + `InputState::set_value`
     (via the `cell_editor_for_test` handle) for the characters.
   - Rung (c) last resort: emit-seam for the commit assertions only (documented
     honest cut — keeps mount / Escape / invalid, which don't need typed chars).
3. **Bool `Select` drive (test 6).** Spike the second paint path separately
   (`SelectEvent::Confirm` headless). **Honest-cut clause:** if `Select` won't
   drive, drop test 6 — its parse/commit logic is already unit-covered
   (`parse_bool_text_path_accepts_common_forms` + `BoolItem`), so the cut is
   cheap and documented.
4. **Round-trip read-back timing (test 3).** After commit, `spawn_rebind`
   rebuilds the view; pump the async harness + drain until the new page is
   resident before reading `cell_display_for_test`. Reuse the page-0 pump loop.

Lesson carried from prior slices: **a T0 spike only proves the surfaces it
exercises** — spike the numeric text path AND the bool `Select` path, not just
one.

## Deps / CI / footprint

- **Zero new deps** (D-015 stays open; `tempfile`/`serial_test` already dev-deps).
- **+1 gpui integration-test binary.** Linux disk exhaustion is **RESOLVED**
  (PR #54 DWARF fix — Linux `target/` 110 G→18 G), so the historical margin
  concern is gone; macOS CI still runs thin → **watch the post-merge main run**
  (macOS grid-scroll bench is push-to-main-only → can redden main silently).
- **Cross-binary gate:** this slice adds no `.a11y` nodes, so it should NOT shift
  other binaries' frame-count assertions (`a11y_spike`). The controller still
  runs `cargo test --workspace --no-fail-fast` to confirm.
- Exec recipe (proven on recents / catalog / AI-config): fable implementers +
  sonnet task reviewers + opus for the T0 gate and the final whole-branch review;
  T0 HARD GATE first; controller runs the `cargo test --workspace` + clippy
  `-D warnings` gate while implementers run only the focused test; `cargo fmt
  --all` before every commit; DCO `git commit -s`. Toolchain pinned 1.97.0.

## What this deliberately does NOT do

- **No PD-020 Tab→right advance** — stays deferred (`Input` focus contention).
- **No seed-from-cell** (`with_seed` stays unused by `begin_cell_edit`).
- **No Blur→commit-in-place test** — dropped (Q3); headless focus-out is a drive
  risk and the emit path is structurally simple.
- **No F2-alias test** — dropped (Q3).
- **No `.a11y` / accessible-name / screen-reader announce** on the editor overlay
  — coverage-only.
- **No read-only-gate or popover-open-guard tests** — already unit-covered
  (`edit_ops` `gate_tests`, `read_only_gate.rs`).
- **No re-proving the engine round-trip per type** — `edit_restore_e2e` covers
  that; this slice proves the UI bridge once (test 3).
