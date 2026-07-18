# Inline cell-editor behavioral coverage — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add end-to-end behavioral coverage for the already-shipped inline cell-editor (grid `Enter`/`F2` → edit → commit/advance → cancel → invalid-reject → bool), using the windowed AccessKit grid harness — closing the UAT-owed gap (PD-013 / P4b–P4c T15) with **zero production behavior change**.

**Architecture:** New windowed integration binary `tests/cell_editor_nav.rs` (feature `a11y-capture`, `#[serial]`). Reuse `keyboard_nav.rs`'s proven grid harness (real session + `WorkspaceShell` window + `MainThreadDispatcher` + async runtime). Seed a CTAS-typed table (numeric `n` + bool `flag`) on the session engine, bind a `GridDataSource`, drive via `simulate_keystrokes` + a test-only `InputState::set_value` accessor for the typed characters, and assert via new test-only `#[cfg(feature = "a11y-capture")]` accessors. No `.a11y` nodes added → release byte-identical → no owed human glance.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component (pinned git), DuckDB engine, `tempfile`, `serial_test` (all existing dev-deps — **zero new deps**).

## Global Constraints

- **Zero new dependencies.** `Cargo.toml` / `Cargo.lock` / `NOTICE` unchanged. D-015 stays open.
- **Zero production behavior change.** Only `#[cfg(feature = "a11y-capture")]` test accessors may be added to `src/`. No `.a11y` / `.a11y_label`, no new production `div`/element, no keymap change. The release binary must be byte-identical.
- **Toolchain pinned 1.97.0.** `cargo fmt --all` before EVERY commit (the CI `fmt --check` gate is unforgiving of the plan's example wrapping). DCO: every commit uses `git commit -s`.
- **Test-only symbols go BEFORE any `#[cfg(test)] mod tests`** in a source file (clippy `items-after-test-module` under `-D warnings`).
- **Implementers run only the focused test** (`cargo test -p dat0-app --test cell_editor_nav`); the controller runs the `cargo test --workspace --no-fail-fast` + `clippy --workspace --all-targets -D warnings` gate.
- Commit-message trailer on every commit: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Create:** `crates/dat0-app/tests/cell_editor_nav.rs` — the whole slice's tests + a per-binary copy of `keyboard_nav.rs`'s grid harness helpers + one new `seed_typed_grid` helper.
- **Modify:** `crates/dat0-app/src/window.rs` — add 3 `#[cfg(feature = "a11y-capture")]` accessors to the existing accessor `impl` block (starts line 6782, next to `grid_active_cell_for_test` ~6872).
- **Modify:** `crates/dat0-app/src/grid/cell_editor.rs` — add a `#[cfg(feature = "a11y-capture")] impl CellEditor` block (2 accessors) placed BEFORE the `#[cfg(test)] mod tests` at line 519.

No other production files change.

---

## Task 1: T0 HARD GATE — accessors + drive-ladder spike

**This is the load-bearing gate.** It proves — in one throwaway windowed test — the four drive mechanisms the whole slice rests on, BEFORE any real test is written. If a STOP-clause fires, stop and report; do not build Tasks 2–3 on an unproven drive.

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (accessor block ~6872)
- Modify: `crates/dat0-app/src/grid/cell_editor.rs` (before line 519)
- Create: `crates/dat0-app/tests/cell_editor_nav.rs`

**Interfaces produced (used by Tasks 2–3):**
- `WorkspaceShell::cell_editor_open_for_test(&self) -> bool`
- `WorkspaceShell::cell_editor_for_test(&self) -> Option<Entity<CellEditor>>`
- `WorkspaceShell::cell_display_for_test(&self, row: usize, col: usize) -> Option<String>`
- `CellEditor::column_type_for_test(&self) -> ColumnType`
- `CellEditor::set_text_value_for_test(&mut self, value: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>)`
- Test helper `seed_typed_grid(cx, harness, session, shell) -> Arc<GridDataSource>`

- [ ] **Step 1: Add the `WorkspaceShell` accessors**

In `src/window.rs`, inside the existing `#[cfg(feature = "a11y-capture")] impl WorkspaceShell { … }` block (right after `grid_active_cell_for_test`, ~line 6880), add:

```rust
    /// Cell-editor coverage slice: is the inline cell editor currently mounted?
    pub fn cell_editor_open_for_test(&self) -> bool {
        self.cell_editor.is_some()
    }

    /// The live inline cell-editor entity (to reach its inner `InputState` /
    /// column type from a test). `None` when no editor is mounted.
    pub fn cell_editor_for_test(&self) -> Option<Entity<crate::grid::cell_editor::CellEditor>> {
        self.cell_editor.clone()
    }

    /// Read a rendered cell's display string off the LIVE data source (which, after
    /// a commit, is the rebound overlay view), by screen `(row, visible-col)`.
    /// `None` when no data source is mounted or the cell isn't resident.
    pub fn cell_display_for_test(&self, row: usize, col: usize) -> Option<String> {
        self.data_source.as_ref()?.cell_display(row, col)
    }
```

`Entity` is already imported in `window.rs`. `cell_editor` is `pub(crate)` (line 2010) — reachable from this same-module impl.

- [ ] **Step 2: Add the `CellEditor` accessors**

In `src/grid/cell_editor.rs`, immediately BEFORE `#[cfg(test)]\nmod tests {` (line 519), add:

```rust
#[cfg(feature = "a11y-capture")]
impl CellEditor {
    /// The column type this editor was built for. Lets a test assert the Bool
    /// column mounted the `Select` path (not a text `Input`). `ColumnType` is `Copy`.
    pub fn column_type_for_test(&self) -> ColumnType {
        self.column_type
    }

    /// Set the inner text input's value directly — the reliable headless drive for
    /// the typed characters (raw per-char keystrokes into a gpui-component `Input`
    /// are unreliable; the Settings-slice finding, which also used
    /// `InputState::set_value`). No-op when the widget is the Bool `Select` or
    /// hasn't rendered its `InputState` yet.
    pub fn set_text_value_for_test(
        &mut self,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(text) = self.widgets.as_ref().and_then(|w| w.text.clone()) {
            text.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }
}
```

`SharedString`, `Window`, `Context` are already imported in `cell_editor.rs`. `InputState::set_value(value: impl Into<SharedString>, window, cx)` is confirmed at `gpui-component .../input/state.rs:599`.

- [ ] **Step 3: Verify the crate still builds with the feature on**

Run: `cargo build -p dat0-app --features a11y-capture`
Expected: builds clean (accessors compile; no unused warnings under a plain build).

- [ ] **Step 4: Create the test file with the harness + `seed_typed_grid`**

Create `crates/dat0-app/tests/cell_editor_nav.rs`. **Copy VERBATIM from `tests/keyboard_nav.rs`** the following items (they compile as-is; do not modify): the module doc-comment block may be replaced, but copy the helper bodies of `set_config_dir`, the `BUDGET` const, `struct AsyncHarness` + its `impl`, `enter_async_harness`, `build_empty_session_in`, `open_shell_window`, `init_components`, `dialog_open`, the `MAIN_LOOP` static, `ensure_dispatcher`, and `drain_dispatcher`. Do NOT copy `focus_shell_neutrally` or `build_empty_session` (unused here). Use these imports (a trimmed subset of `keyboard_nav.rs`'s):

```rust
//! Cell-editor inline-edit behavioral coverage (UAT — PD-013 / P4b–P4c T15).
//!
//! Windowed tests that drive the SHIPPED inline cell editor through the real
//! grid keystroke path. Harness helpers are copied per-binary from
//! `tests/keyboard_nav.rs` (this crate's per-binary-copy precedent).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::grid::GridDataSource;
use dat0_app::grid::selection::CellCoord;
use dat0_app::main_bridge::{MainLoop, MainThreadDispatcher};
use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;
use dat0_engine::QueryEngine as _;
```

Then add the new seed helper:

```rust
/// CTAS a small TYPED table on the session engine (numeric `n` + bool `flag`,
/// 3 rows), bind it as a `GridDataSource`, mount it as the active grid, and pump
/// until page 0 is resident (PD-018) so the lazily-built `SelectionModel` exists.
/// CTAS (not CSV) guarantees `n`→Numeric and `flag`→Bool column types. Returns
/// the mounted data source (keep it bound for the test — `cell_render` residence
/// checks read it).
fn seed_typed_grid(
    cx: &mut VisualTestContext,
    harness: &AsyncHarness,
    session: &Arc<Mutex<Session>>,
    shell: &Entity<WorkspaceShell>,
) -> Arc<GridDataSource> {
    const SQL: &str = "SELECT * FROM (VALUES (1, true), (2, false), (3, true)) v(n, flag)";
    let engine = session.lock().engine.clone();
    harness.block_on(async {
        engine
            .create_table("cells", SQL, dat0_engine::DerivedOrigin::Sql(SQL.into()))
            .await
            .expect("create_table cells");
    });
    let ds = harness
        .block_on(async { GridDataSource::new(Arc::clone(&engine), "cells".to_string()).await })
        .expect("GridDataSource::new");
    let ds = Arc::new(ds);

    let ds_mount = Arc::clone(&ds);
    shell.update(cx, |view, cx| {
        view.set_data_source(ds_mount);
        cx.notify();
    });

    // Pump until page 0 is resident so `render` builds the SelectionModel.
    let mut ready = false;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        if ds.cell_render(0, 0).is_some() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready, "page 0 must load into the grid LRU before interacting");
    ds
}

/// Open a fresh window over an empty session, flush + close the first-run
/// auto-show tour dialog (mandatory baseline), then seed the typed grid and
/// focus the grid shell (no click — `focus_next` leaves the active cell at the
/// origin). Returns `(shell, vcx, ds, state_dir)`; keep `ds` + `state_dir` alive.
fn mount_grid_ready<'a>(
    cx: &'a mut TestAppContext,
    harness: &AsyncHarness,
) -> (
    Entity<WorkspaceShell>,
    &'a mut VisualTestContext,
    Arc<GridDataSource>,
    tempfile::TempDir,
) {
    let state = tempfile::tempdir().unwrap();
    let session = build_empty_session_in(harness, state.path());
    let (shell, vcx) = open_shell_window(cx, Arc::clone(&session));
    vcx.run_until_parked();
    drain_dispatcher(vcx);
    vcx.run_until_parked();
    if dialog_open(vcx) {
        vcx.update(|window, app| window.close_dialog(app));
        vcx.run_until_parked();
    }
    let ds = seed_typed_grid(vcx, harness, &session, &shell);
    // Focus the grid shell WITHOUT clicking a cell (keeps active at (0,0)).
    vcx.update(|window, _app| window.focus_next());
    vcx.run_until_parked();
    (shell, vcx, ds, state)
}
```

`create_table` resolves via the `QueryEngine` trait import. `close_dialog` / `has_active_dialog` come from `gpui_component::WindowExt` (already imported). The per-test setup (`set_config_dir`, `ensure_dispatcher`, `init_components`, `enter_async_harness`) stays in each test body (it needs the raw `cx` + tempdir lifetimes), exactly as in `keyboard_nav.rs`.

- [ ] **Step 5: Write the T0 gate spike test**

Add to `cell_editor_nav.rs`:

```rust
/// T0 HARD GATE — proves the four drive mechanisms the slice rests on:
///   1. grid `Enter` (keystroke on the focused shell) mounts the editor.
///   2. a mounted editor's value + `Enter` (keystroke) commits AND advances the
///      active cell by EXACTLY one row (the key ambiguity: gpui-component `Input`
///      binds `enter` as a context action — this asserts it does NOT also double-
///      fire the shell's `begin_cell_edit`).
///   3. the committed numeric value round-trips and reads back off the live source.
///   4. a Bool column mounts the `Select` path (not a text `Input`).
#[gpui::test]
#[serial]
fn t0_drive_ladder(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);

    // (1) grid Enter → editor mounts.
    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()),
        CellCoord { row: 0, col: 0 },
        "sanity: fresh SelectionModel starts at origin"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "STOP-1: grid Enter must mount the cell editor (begin_cell_edit wiring)"
    );

    // (2) set value + Enter → commit + advance by exactly one row, editor re-open.
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("42", window, ecx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();
    let after = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());
    assert_eq!(
        after,
        CellCoord { row: 1, col: 0 },
        "STOP-2: Enter must commit AND advance exactly one row (a value of {:?} \
         means it re-mounted / double-fired / did not advance)",
        after
    );
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "STOP-2b: the editor must re-open on the advanced cell"
    );

    // (3) round-trip: the committed 42 reads back at (0,0).
    let mut got = None;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        got = shell.update(cx, |ws, _| ws.cell_display_for_test(0, 0));
        if got.as_deref() == Some("42") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        got.as_deref(),
        Some("42"),
        "STOP-3: the committed numeric value must round-trip through the engine"
    );

    // (4) Bool column mounts the Select path. Cancel the current editor, move to
    // col 1 (`flag`), open a fresh editor, assert its column type is Bool.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.simulate_keystrokes("right"); // active → (1, 1) is fine; any bool cell
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let bool_editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("bool editor mounted");
    let ct = bool_editor.read_with(cx, |ed, _| ed.column_type_for_test());
    assert_eq!(
        ct,
        dat0_app::view::filter_popover::ColumnType::Bool,
        "STOP-4: the bool column must mount the Select path"
    );
}
```

- [ ] **Step 6: Run the gate**

Run: `cargo test -p dat0-app --features a11y-capture --test cell_editor_nav t0_drive_ladder -- --nocapture`
Expected: **PASS.**

**STOP-clauses (report + halt if any fires — do NOT proceed to Task 2):**
- **STOP-1** (grid Enter doesn't mount): investigate shell focus / that `focus_next` reached the shell (mirror `keyboard_nav`'s grid test; a click into the window center as a fallback baseline).
- **STOP-2** (no advance / double-advance / re-mount): the commit-Enter double-fires or is swallowed. Fallback drive: keep grid-Enter-mount by keystroke, but drive the COMMIT by emitting on the editor entity — `cx.update(|_, app| editor.update(app, |_, ecx| ecx.emit(dat0_app::grid::cell_editor::CellEditorEvent::CommitAndMove(dat0_engine::Scalar::Int(42), dat0_app::grid::cell_editor::EditorAdvance::Down))))` — and document that the `Input`→`PressEnter` leg is proven only structurally (honest partial). Re-verify advance-by-one.
- **STOP-3** (no round-trip): confirm the async harness guard is held and `drain_dispatcher` runs; extend the pump budget. If the overlay display formats differently, assert `Some("42")` exactly (Int → `i.to_string()`, confirmed `render.rs:347`).
- **STOP-4** (bool not Select): `column_type_for_source` didn't yield `Bool` for `flag` — the CTAS `BOOLEAN` type is expected; if not, adjust the CTAS SQL to `CAST(... AS BOOLEAN)`.

- [ ] **Step 7: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/src/grid/cell_editor.rs crates/dat0-app/tests/cell_editor_nav.rs
git commit -s -m "test(cell-editor): T0 hard gate — drive ladder + accessors

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Core edit-flow suite (mount · advance · escape-cancel · invalid-reject)

**Files:**
- Modify: `crates/dat0-app/tests/cell_editor_nav.rs` (add 4 named tests)

**Interfaces consumed:** all Task 1 accessors + `mount_grid_ready`.

- [ ] **Step 1: Write the four tests**

Add to `cell_editor_nav.rs`:

```rust
/// Baseline entry trigger: with the grid focused, `Enter` mounts the editor.
#[gpui::test]
#[serial]
fn grid_enter_mounts_editor(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    assert!(
        !shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "baseline: no editor before Enter"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "grid Enter must mount the inline cell editor"
    );
}

/// `Enter` in the editor commits and advances the active cell one row down, then
/// re-opens the editor on the new cell (spreadsheet walk-down).
#[gpui::test]
#[serial]
fn enter_commits_and_advances_down(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    let before = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());

    cx.simulate_keystrokes("enter"); // mount
    cx.run_until_parked();
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("7", window, ecx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter"); // commit + advance
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();

    let after = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());
    assert_eq!(after.row, before.row + 1, "Enter must advance one row down");
    assert_eq!(after.col, before.col, "advance must stay in the same column");
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "the editor must re-open on the advanced cell"
    );
}

/// `Escape` cancels the edit: the editor disappears and the cursor stays put.
#[gpui::test]
#[serial]
fn escape_cancels_and_keeps_cursor(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    let before = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());

    cx.simulate_keystrokes("enter"); // mount
    cx.run_until_parked();
    assert!(shell.update(cx, |ws, _| ws.cell_editor_open_for_test()));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        !shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "Escape must dismiss the editor"
    );
    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()),
        before,
        "Escape must leave the cursor on the cell being edited"
    );
}

/// Invalid input is rejected: typing non-numeric into a numeric cell + `Enter`
/// suppresses the commit — no advance, and the editor STAYS open so the user can
/// fix it.
#[gpui::test]
#[serial]
fn invalid_numeric_is_rejected_editor_stays(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);
    let before = shell.update(cx, |ws, _| ws.grid_active_cell_for_test());

    cx.simulate_keystrokes("enter"); // mount over numeric col 0
    cx.run_until_parked();
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("abc", window, ecx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter"); // commit attempt — must be suppressed
    cx.run_until_parked();
    drain_dispatcher(cx);
    cx.run_until_parked();

    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()),
        before,
        "invalid input must NOT advance the active cell"
    );
    assert!(
        shell.update(cx, |ws, _| ws.cell_editor_open_for_test()),
        "invalid input must leave the editor open to correct"
    );
}
```

- [ ] **Step 2: Run the four tests**

Run: `cargo test -p dat0-app --features a11y-capture --test cell_editor_nav -- --nocapture`
Expected: **all PASS** (t0 gate + these four).

Note: `invalid_numeric_is_rejected_editor_stays` assumes the commit-Enter uses the same keystroke drive proven in T0. If T0 landed on the emit-boundary fallback (STOP-2), drive the invalid case by emitting nothing (the editor's own parse rejects) — instead assert via the pure `CellEditor::parse_text(ColumnType::Numeric, "abc") == None` unit already in `cell_editor.rs`, and keep the behavioral test only for the keystroke path. Document the choice in a comment.

- [ ] **Step 3: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/cell_editor_nav.rs
git commit -s -m "test(cell-editor): core edit-flow suite (mount/advance/escape/invalid)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Numeric round-trip + bool Select path

**Files:**
- Modify: `crates/dat0-app/tests/cell_editor_nav.rs` (add 2 named tests)

**Interfaces consumed:** all Task 1 accessors + `mount_grid_ready`.

- [ ] **Step 1: Write the round-trip test**

```rust
/// The one deep proof: a typed numeric value driven through the real engine
/// round-trip reads back off the live (rebound overlay) data source. Proves the
/// UI→CellEdit→engine bridge once.
#[gpui::test]
#[serial]
fn numeric_commit_round_trips_to_data_source(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);

    // Sanity: (0,0) starts at the seeded 1.
    assert_eq!(
        shell.update(cx, |ws, _| ws.cell_display_for_test(0, 0)).as_deref(),
        Some("1"),
        "seed sanity: (0,0) is 1 before the edit"
    );

    cx.simulate_keystrokes("enter"); // mount over (0,0)
    cx.run_until_parked();
    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("editor mounted");
    cx.update(|window, app| {
        editor.update(app, |ed, ecx| ed.set_text_value_for_test("42", window, ecx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter"); // commit + advance
    cx.run_until_parked();

    let mut got = None;
    for _ in 0..100 {
        cx.run_until_parked();
        drain_dispatcher(cx);
        cx.run_until_parked();
        got = shell.update(cx, |ws, _| ws.cell_display_for_test(0, 0));
        if got.as_deref() == Some("42") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        got.as_deref(),
        Some("42"),
        "the edited numeric value must persist through the engine round-trip \
         and read back off the live overlay source"
    );
}
```

- [ ] **Step 2: Write the bool test (primary = Select mount; stretch = confirm)**

```rust
/// A Bool column mounts the `Select` widget path (not a text `Input`). This is
/// the meaningful, robust proof of the second widget path. Driving the actual
/// `SelectEvent::Confirm` headlessly is a known gpui-component limitation
/// (`set_selected_value` does not emit `Confirm`) — the commit itself stays
/// covered by the `parse_bool_text_path` unit in `cell_editor.rs`, so the mount
/// proof is where the new coverage lives.
#[gpui::test]
#[serial]
fn bool_column_mounts_select_path(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    ensure_dispatcher();
    let harness = enter_async_harness(cx);
    let _guard = harness.enter();
    init_components(cx);
    drain_dispatcher(cx);

    let (shell, cx, _ds, _state) = mount_grid_ready(cx, &harness);

    // Move the active cell to the bool column (`flag`, screen col 1), then open.
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(
        shell.update(cx, |ws, _| ws.grid_active_cell_for_test()).col,
        1,
        "Right must move the active cell onto the bool column"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let editor = shell
        .update(cx, |ws, _| ws.cell_editor_for_test())
        .expect("bool editor mounted");
    let ct = editor.read_with(cx, |ed, _| ed.column_type_for_test());
    assert_eq!(
        ct,
        dat0_app::view::filter_popover::ColumnType::Bool,
        "a bool column must mount the Select (Bool) editor path, not a text Input"
    );
}
```

- [ ] **Step 3: Run the two tests**

Run: `cargo test -p dat0-app --features a11y-capture --test cell_editor_nav -- --nocapture`
Expected: **all PASS** (6 tests + t0 gate = 7 total).

**Honest-cut clause (bool):** if T0's STOP-4 revealed the bool column does not mount `Select` in this harness, drop `bool_column_mounts_select_path` and note it — the parse logic stays unit-covered. Do not fake it.

- [ ] **Step 4: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/cell_editor_nav.rs
git commit -s -m "test(cell-editor): numeric round-trip + bool Select-path mount

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Controller gate + final review

**Files:** none (verification only).

- [ ] **Step 1: Workspace test gate (catches cross-binary drift)**

Run: `cargo test -p dat0-app --features a11y-capture --workspace --no-fail-fast`
Expected: PASS. In particular `a11y_spike` must be UNCHANGED — this slice adds no `.a11y` nodes, so no frame-count assertion should shift. If any binary's assertion moved, investigate before proceeding (an accidental production element crept in).

- [ ] **Step 2: Clippy + fmt gate (pinned 1.97.0)**

Run: `cargo clippy -p dat0-app --features a11y-capture --all-targets -- -D warnings`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`
Expected: all clean. (`items-after-test-module` would catch a mis-placed `CellEditor` accessor block.)

- [ ] **Step 3: Prove ZERO release footprint**

Run: `cargo build -p dat0-app` (feature OFF — the default)
Run: `cargo build -p dat0-app --release`
Expected: both build clean with no reference to the `_for_test` accessors (they are `#[cfg(feature = "a11y-capture")]`, absent from a default/release build). Confirm `git diff --stat main` touches only `tests/cell_editor_nav.rs` (new) + the two `src/` files, and that every `src/` addition is under `#[cfg(feature = "a11y-capture")]`.

- [ ] **Step 4: Confirm no dependency / manifest drift**

Run: `git diff main -- Cargo.toml Cargo.lock NOTICE crates/dat0-app/Cargo.toml`
Expected: EMPTY. Zero new deps (Global Constraint).

- [ ] **Step 5: Final whole-branch review (opus) + push/PR**

Dispatch a fresh-context whole-branch review (opus) checking: zero production behavior change (all `src/` additions cfg-gated), no `.a11y`/new-element, the drive genuinely exercises the keystroke path (not only the emit-seam), each STOP-clause fallback honestly documented, teeth non-vacuous (round-trip reads a value that differs from the seed; invalid-reject would fail if the commit weren't suppressed). Address Critical/Important; fold Minors. Then push `uat-cell-editor-nav` and open the PR. **Watch the post-merge main run** — the macOS grid-scroll bench is push-to-main-only and can redden main silently.

---

## Self-Review

**Spec coverage** (design §Scope + §Tests):
- Coverage-only, zero prod change → Global Constraints + Task 4 Step 3. ✓
- Boundary assertions (mount/advance/teardown/invalid/bool) → Task 2 + Task 3 Step 2. ✓
- One numeric round-trip → Task 3 Step 1. ✓
- Flows = baseline + invalid-reject + bool (Blur/F2 dropped) → Tasks 2–3; no Blur/F2 test present. ✓
- T0 hard gate proving the drive ladder first → Task 1. ✓
- Test-only `#[cfg(a11y-capture)]` seams, no `.a11y` → Steps 1–2, Task 4 Step 1. ✓
- CTAS-typed table (numeric + bool) → `seed_typed_grid`. ✓
- Non-goals (PD-020, seed-from-cell, Blur, F2, a11y-name, read-only/popover) → none added. ✓

**Placeholder scan:** no TBD/TODO; every code step shows full code; STOP-clauses give concrete fallbacks, not "handle errors". ✓

**Type consistency:** `cell_editor_open_for_test`/`cell_editor_for_test`/`cell_display_for_test`/`column_type_for_test`/`set_text_value_for_test`/`grid_active_cell_for_test`/`seed_typed_grid`/`mount_grid_ready` — names identical across Tasks 1–3. `CellCoord { row, col }`, `ColumnType::Bool`, `Scalar::Int` used consistently. `set_value(value, window, cx)` matches the confirmed gpui-component signature. ✓
