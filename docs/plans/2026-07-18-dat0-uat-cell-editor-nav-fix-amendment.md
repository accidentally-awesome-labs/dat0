# Amendment — cell-editor slice pivots from coverage-only to fix + coverage

**Date:** 2026-07-18
**Amends:** `2026-07-18-dat0-uat-cell-editor-nav-design.md` + `-plan.md`
**Branch:** `uat-cell-editor-nav`

## Why this amendment exists

The plan's Task 1 (T0 hard gate) was a *coverage-only* spike that assumed the
shipped inline editor's "Enter walks down a column" advance worked, and merely
needed a test. Running the T0 spike **falsified that assumption** and surfaced
**two real production bugs** that jointly break Enter-to-commit-and-advance for a
real keyboard user. Per the plan's own STOP-clause governance ("do NOT fake or
weaken an assertion to make it green") the implementer reported BLOCKED rather
than asserting broken behavior as correct.

The user decided (2026-07-18): **fix both bugs first, then cover the fixed
behavior.** This converts the slice from "coverage-only / zero production
behavior change / release byte-identical" into "**two small production fixes +
behavioral coverage that asserts the fixed behavior**". The test accessors stay
`#[cfg(feature = "a11y-capture")]`; the two fixes ship in real (non-gated) code.

## Bug A — grid Enter re-mounts the editor and drops the commit subscription

**Site:** `window.rs:6436` (the grid key handler).

```rust
if (key_str == "enter" || key_str == "f2") && no_mods {
    ws.begin_cell_edit(window, cx);   // fires UNCONDITIONALLY
    return;
}
```

When an editor is already open and its inner `Input` handles Enter, gpui-component's
`Input::enter()` emits `InputEvent::PressEnter` **and then calls `cx.propagate()`**
(by design, so an enclosing dialog can also react). The raw `KeyDownEvent` therefore
bubbles to this shell handler, which calls `begin_cell_edit` **again** — replacing
`self.cell_editor` / `self.cell_editor_sub` and dropping the OLD `Subscription`
(the documented P4a T10b trap) **before** the just-queued `PressEnter` is delivered
to it. Result: `CommitAndMove` never routes; the keystroke commit is lost. This is
real (same `dispatch_key_event` path gpui uses for platform events), not a test
artifact.

**Fix:** guard the branch so the open editor owns Enter:

```rust
if (key_str == "enter" || key_str == "f2") && no_mods && ws.cell_editor.is_none() {
    ws.begin_cell_edit(window, cx);
    return;
}
```

With the guard, the shell no-ops on Enter while an editor is open; the queued
`PressEnter` reaches the still-alive subscription → `CommitAndMove` →
`commit_cell_edit_and_advance`.

## Bug B — the advance is reset to origin by the async rebind

**Sites:** `edit_ops.rs:164` (`commit_cell_edit_and_advance`) vs `window.rs:2350`
(`apply_view_change`) vs `window.rs:6062-6064` (render rebuild).

`commit_cell_edit_and_advance` commits (which fires `spawn_rebind`, an async engine
round-trip), then synchronously `sel.move_active(1, 0)` and re-opens the editor.
But when the round-trip lands, `apply_view_change` unconditionally does
`self.selection = None` ("defensively clear"), and the next render rebuilds a fresh
`SelectionModel::new(rows, cols)` **at the origin**. The moved model is discarded →
the cursor snaps back to `(0, 0)`. Deterministic (proven with a 100-iteration settle
loop), and it hits **every** cell-edit commit path, not just Enter-advance.

**Fix (targeted):** carry the intended post-commit active cell across the rebind.

- Add `pending_active_cell: Option<crate::grid::selection::CellCoord>` to
  `WorkspaceShell` (init `None` in the constructor).
- In `commit_cell_edit_and_advance`, after `sel.move_active(dr, dc)`, record
  `self.pending_active_cell = Some(sel.active())`.
- In the render rebuild (`window.rs:6062-6064`), when a fresh `SelectionModel` is
  built, honor a pending target via the existing **clamped** `move_active_to`:
  ```rust
  if rows > 0 && cols > 0 && self.selection.is_none() {
      let mut model = crate::grid::selection::SelectionModel::new(rows, cols);
      if let Some(target) = self.pending_active_cell.take() {
          model.move_active_to(target.row, target.col); // clamps to new dims
      }
      self.selection = Some(model);
  }
  ```
- Clear `pending_active_cell = None` in `set_data_source` (a brand-new source has no
  cursor to restore).

`move_active_to` (selection.rs:225) already clamps to the new dimensions, so the
"defensive clear guards against a future column-count change" rationale in
`apply_view_change` is preserved. Only the advance path sets `pending_active_cell`;
undo/redo/console rebinds leave it `None` and keep their current reset-to-origin
behavior (unchanged, untouched).

## Scope of behavior change

- Enter (or F2-then-Enter) in the inline editor now genuinely commits the edit AND
  walks the cursor down one row, re-opening the editor on the new cell — for real
  keyboard input, not just via the event-emit seam.
- No other cursor behavior changes (plain non-advance commit, undo/redo, console
  bind still reset to origin — out of scope for this slice; recorded as an
  observation below).

## Deliberately NOT fixed here (recorded observations)

- **Plain (non-advance) cell commit** and **undo/redo/SQL-console rebind** still
  reset the cursor to origin (same `apply_view_change` clear). Only the Enter-advance
  path opts into cursor preservation. A general "preserve cursor across every rebind"
  is a broader UX change touching untested surfaces — separate decision.
- **Escape-then-navigate stale focus:** after `Escape` tears down the editor, a stale
  `FocusId` can make subsequent keystrokes miss the shell handler until focus is
  re-established (mirrors a note in `tests/keyboard_nav.rs`). Not on the
  commit/advance path; tests route around it (bool test mounts fresh). Worth a manual
  UAT check.

## New owed human glance

- **Behavioral (not pixels):** in the running app, type down a numeric column with
  Enter — confirm the cursor visibly walks down and each value commits. Confirms
  Bug A + Bug B fixes at the real UI. (No new a11y ring/contrast glance — no `.a11y`
  node or new element added.)
