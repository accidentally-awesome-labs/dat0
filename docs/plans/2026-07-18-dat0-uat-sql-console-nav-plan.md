# SQL Console keyboard-navigation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the SQL Console's primary toolbar (Run/Cancel, run-in-pane, new-tab, history, save, saved, save-as-Table) and tab strip keyboard-reachable and operable, shipping real production a11y plus windowed behavioral coverage.

**Architecture:** Fixed toolbar buttons each get the shipped chip triad — `focus_stop(id, &fh, 0, on_activate).a11y(id, Button, label)` chained onto the existing `div().id(id)...on_click(...)`, `on_activate` emitting the same `SqlConsoleEvent` as the click. The dynamic tab strip becomes ONE container `focus_stop` (the recents/catalog tablist pattern) keyed off the existing `self.active`: a chained 2nd `.on_key_down` handles ←/→ (auto-switch active + emit `Persist`) and Delete/Backspace (close active via `close_tab`). Focus handles for the buttons live in a new `toolbar_focus: HashMap<&'static str, FocusHandle>`; the tab strip gets one `tabstrip_focus` field.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component (pinned git), the in-repo a11y kit (`focus_stop`/`a11y`/`focused_label`), `tempfile`, `serial_test` (all existing dev-deps — **zero new deps**).

## Global Constraints

- **Zero new dependencies.** `Cargo.toml` / `Cargo.lock` / `NOTICE` unchanged. D-015 stays open.
- **Ships real production a11y.** The `focus_stop` / `a11y` wiring is UNCONDITIONAL (not feature-gated), like the Slice-6 / recents / catalog / AI-config slices. Only the `_for_test` read accessors are `#[cfg(feature = "a11y-capture")]`.
- **No new `SqlConsoleEvent` variant** and no change to `WorkspaceShell::on_sql_console_event`. Every `on_activate` re-emits an existing event (or, for tabs, replicates the existing inline mutation).
- **a11y imports stay UNCONDITIONAL** — `sql_console.rs` already imports the a11y kit for the AI chips; reuse it, do NOT `#[cfg]`-gate it.
- **No new i18n keys** — every toolbar label key already exists in `crates/dat0-i18n/src/strings/en.json` (`sql.run`, `sql.cancel`, `sql.run_in_pane`, `sql.new_tab`, `sql.history`, `sql.save_query`, `sql.load_query`, `sql.save_as_table`).
- **Toolchain pinned 1.97.0.** `cargo fmt --all` before EVERY commit (the CI `fmt --check` gate is unforgiving of the plan's example wrapping). DCO: every commit uses `git commit -s`.
- **Test-only symbols go BEFORE any `#[cfg(test)] mod tests`** in a source file (clippy `items-after-test-module` under `-D warnings`).
- **Implementers run only the focused test** (`cargo test -p dat0-app --features a11y-capture --test sql_console_nav`); the controller runs the `cargo test --workspace --no-fail-fast` + `clippy --workspace --all-targets -D warnings` gate.
- **Harness helpers are COPIED VERBATIM per-binary** from `tests/ai_nav.rs` — established codebase convention (the module docs of `ai_nav.rs`/`catalog_nav.rs` state it), NOT a duplication defect.
- Commit-message trailer on every commit: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Create:** `crates/dat0-app/tests/sql_console_nav.rs` — the whole slice's tests + a per-binary copy of `ai_nav.rs`'s mount + console-open helpers.
- **Modify:** `crates/dat0-app/src/view/sql_console.rs` —
  - struct `SqlConsole` (fields at ~123-184): add `toolbar_focus` + `tabstrip_focus`.
  - `SqlConsole::new` (~290-309): init both new fields.
  - a helper `toolbar_fh` (in the main `impl SqlConsole`).
  - `render` (~577-1180): wire `sql-run` + the tab strip (Task 1); the other 6 buttons (Task 2).
  - a `#[cfg(feature = "a11y-capture")] impl SqlConsole` block (3 accessors) placed BEFORE the `#[cfg(test)] mod tests`.

No other production files change.

---

## Task 1: T0 HARD GATE — infra + wire `sql-run` + tab strip + 4-probe spike

**This is the load-bearing gate.** It proves — in one throwaway windowed test — the two drive patterns the whole slice rests on (per-button `focus_stop` reach+operate, and the tablist container reach+switch+close), BEFORE the breadth suites. If a STOP-clause fires, stop and report; do not build Tasks 2–3 on an unproven drive.

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs`
- Create: `crates/dat0-app/tests/sql_console_nav.rs`

**Interfaces produced (used by Tasks 2–3):**
- `SqlConsole::toolbar_fh(&mut self, id: &'static str, cx: &mut Context<Self>) -> FocusHandle`
- `SqlConsole::active_tab_for_test(&self) -> usize`
- `SqlConsole::tab_count_for_test(&self) -> usize`
- `SqlConsole::tabstrip_focused_for_test(&self, window: &Window) -> bool`
- Test helpers copied verbatim from `ai_nav.rs`: `set_config_dir`, `BUDGET`, `build_empty_session`, `open_shell_window`, `init_components`, `focus_shell_neutrally`, `AsyncHarness` + `enter_async_harness`, `open_console_with_log`, `tab_labels`, `tab_until`.

- [ ] **Step 1: Add the two struct fields**

In `sql_console.rs`, inside `pub struct SqlConsole { … }`, immediately after the `explain_focus` field (~line 183):

```rust
    /// Toolbar-button focus handles (SQL-Console-nav slice), keyed by the
    /// control's `&'static str` id. Get-or-insert via [`Self::toolbar_fh`] so each
    /// button is a stable Tab stop across re-renders. Kept separate from the
    /// `nl2sql_focus`/`explain_focus` named fields, which predate this map.
    pub(crate) toolbar_focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
    /// Stable focus handle for the tab-strip tablist container (SQL-Console-nav
    /// slice). ONE stop for the whole strip; ←/→ switch the active tab.
    pub(crate) tabstrip_focus: gpui::FocusHandle,
```

- [ ] **Step 2: Initialise both fields in `new`**

In `SqlConsole::new`, in the `Self { … }` literal after `explain_focus: cx.focus_handle(),` (~line 308):

```rust
            toolbar_focus: std::collections::HashMap::new(),
            tabstrip_focus: cx.focus_handle(),
```

- [ ] **Step 3: Add the `toolbar_fh` helper**

In the main `impl SqlConsole` block (e.g. right after `close_tab`):

```rust
    /// Get-or-insert the stable toolbar focus handle for `id` (SQL-Console-nav
    /// slice). Mirrors `WorkspaceShell::hero_focus_handle`; returns a CLONE so the
    /// caller can chain it into `focus_stop` without holding a borrow on the map.
    fn toolbar_fh(&mut self, id: &'static str, cx: &mut Context<Self>) -> gpui::FocusHandle {
        self.toolbar_focus
            .entry(id)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }
```

- [ ] **Step 4: Add the `#[cfg(feature = "a11y-capture")]` accessors**

In `sql_console.rs`, immediately BEFORE the `#[cfg(test)] mod tests {` line (find it; place the block above it so clippy `items-after-test-module` is satisfied):

```rust
#[cfg(feature = "a11y-capture")]
impl SqlConsole {
    /// The active tab index — lets a test assert an arrow switched tabs.
    pub fn active_tab_for_test(&self) -> usize {
        self.active
    }

    /// The open-tab count — lets a test assert Delete closed a tab.
    pub fn tab_count_for_test(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the tab-strip tablist container currently holds focus — the
    /// title-agnostic reach oracle for the tab strip (its accessible name is the
    /// active tab's title, which is dynamic, so the test detects reach by focus).
    pub fn tabstrip_focused_for_test(&self, window: &Window) -> bool {
        self.tabstrip_focus.is_focused(window)
    }
}
```

`Window` is already imported in `sql_console.rs` (used by `new`/`render`).

- [ ] **Step 5: Wire the `sql-run` primary button**

Hoist the handle at the top of `render` (right after `let active = self.active;`, ~line 586):

```rust
        let run_fh = self.toolbar_fh("sql-run", cx);
```

Then replace the `primary_btn` builder (~683-697) with the focus_stop'd version (keep the existing `run_label` computation just above it):

```rust
        let run_key = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            if this.running {
                cx.emit(SqlConsoleEvent::Cancel);
            } else {
                cx.emit(SqlConsoleEvent::Run {
                    target: ResultTarget::MainGrid,
                });
            }
        });
        let primary_btn = div()
            .id("sql-run")
            .px_3()
            .py_1()
            .cursor_pointer()
            .child(SharedString::from(run_label.clone()))
            .focus_stop("sql-run", &run_fh, 0, run_key)
            .a11y("sql-run", AccessRole::Button, run_label)
            .on_click(cx.listener(|this, _ev, _window, cx| {
                if this.running {
                    cx.emit(SqlConsoleEvent::Cancel);
                } else {
                    cx.emit(SqlConsoleEvent::Run {
                        target: ResultTarget::MainGrid,
                    });
                }
            }));
```

`run_label` was previously moved into `.child(...)`; it is now `.clone()`d into the child and moved into `.a11y(...)`. `focus_stop`, `a11y`, `AccessRole`, `SharedString`, `ResultTarget` are already in scope in this file (the AI chips use the a11y kit; the run button already uses the rest).

- [ ] **Step 6: Wire the tab strip as a tablist**

Hoist the handle at the top of `render` (next to `run_fh`):

```rust
        let tabstrip_fh = self.tabstrip_focus.clone();
        let tabstrip_name: String = self.tabs[self.active].meta.title.clone();
```

Then modify the `tab_strip` builder (~615-671): add `.id("sql-tabstrip")` at the top of the chain, and after the trailing `.child(add-button)` append the focus_stop + a11y + arrow/delete key handler. The children/close/add code inside is UNCHANGED — only the outer `div()` id and the three trailing chained calls are added:

```rust
        let tab_strip = div()
            .id("sql-tabstrip")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .children(/* UNCHANGED per-tab label + close closure */)
            .child(/* UNCHANGED "+" add-tab button */)
            // Enter/Space are a no-op: in the auto-activate model the tab under
            // the cursor is already the live tab.
            .focus_stop(
                "sql-tabstrip",
                &tabstrip_fh,
                0,
                cx.listener(|_this, _ev: &gpui::KeyDownEvent, _window, _cx| {}),
            )
            .a11y("sql-tabstrip", AccessRole::Button, tabstrip_name)
            // Second on_key_down for ←/→/Delete. gpui PUSHES key_down listeners, so
            // this coexists with focus_stop's Enter/Space listener (recents-nav R1).
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                let m = &ev.keystroke.modifiers;
                if m.shift || m.platform || m.control || m.alt {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "left" => {
                        if this.active > 0 {
                            this.active -= 1;
                            cx.emit(SqlConsoleEvent::Persist);
                            cx.notify();
                        }
                    }
                    "right" => {
                        if this.active + 1 < this.tabs.len() {
                            this.active += 1;
                            cx.emit(SqlConsoleEvent::Persist);
                            cx.notify();
                        }
                    }
                    "delete" | "backspace" => {
                        let a = this.active;
                        this.close_tab(a, cx); // no-op on the last tab; clamps active
                    }
                    _ => {}
                }
            }));
```

(Leave the per-tab `("sql-tab-label", i)` / `("sql-tab-close", i)` closures exactly as they are — mouse still works.)

- [ ] **Step 7: Build with the feature on**

Run: `cargo build -p dat0-app --features a11y-capture`
Expected: builds clean (no unused warnings; `focus_stop`/`a11y` resolve; the accessors compile).

- [ ] **Step 8: Create the test file with copied helpers**

Create `crates/dat0-app/tests/sql_console_nav.rs`. **Copy VERBATIM from `tests/ai_nav.rs`** (they compile as-is): the module `mod support;` line, the imports block (adjust to drop `AiPanel`/`Provider`, keep `SqlConsole`, `SqlConsoleEvent`, `WorkspaceShell`, `Session`, `A11ySnapshot`, `press_tab`), the `BUDGET` const, `set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`, `focus_shell_neutrally`, `struct AsyncHarness` + impl, `enter_async_harness`, `open_console_with_log`, `tab_labels`, `tab_until`. Header comment:

```rust
//! SQL Console keyboard-nav behavioral coverage (UAT carve-out #5).
//!
//! Windowed tests driving the SHIPPED SQL Console toolbar + tab strip through
//! the real keystroke path. Harness helpers are copied per-binary from
//! `tests/ai_nav.rs` (this crate's per-binary-copy precedent).
```

- [ ] **Step 9: Write the T0 gate spike**

Add to `sql_console_nav.rs`:

```rust
/// T0 HARD GATE — proves the two drive patterns the slice rests on:
///   Probe 1: `sql-run` is Tab-reachable across the shell→console boundary
///            (the oracle names it by its `sql.run` label).
///   Probe 2: Enter on the focused Run button emits `SqlConsoleEvent::Run`.
///   Probe 3: the tab strip is Tab-reachable and `left` switches the active tab
///            (auto-activate) and emits `Persist`.
///   Probe 4: `delete` on the focused tab strip closes a tab (count 2 → 1).
#[gpui::test]
#[serial]
fn t0_sql_console_nav_gate(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, log) = open_console_with_log(&shell, vcx);

    // Seed a 2nd tab so switch/close have something to act on. `new_tab` needs a
    // `&mut Window`; it makes the new tab active (active == 1 afterwards).
    vcx.update(|window, app| console.update(app, |c, cx| c.new_tab(window, cx)));
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(vcx.cx, |c, _| c.tab_count_for_test()),
        2,
        "seed: 2 tabs open"
    );

    let run = dat0_i18n::t("sql.run");

    // Probe 1: Run is Tab-reachable.
    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 60);
    assert!(
        seen.contains(&run),
        "STOP-1: sql-run must be Tab-reachable across the shell→console boundary; visited {seen:?}"
    );

    // Probe 2: Enter on Run emits Run.
    focus_shell_neutrally(vcx);
    tab_until(vcx, &run);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Run { .. })),
        "STOP-2: Enter on sql-run must emit Run; got {:?}",
        log.borrow()
    );

    // Probe 3: reach the tab strip, then `left` switches active (2nd tab → 1st)
    // and emits Persist.
    focus_shell_neutrally(vcx);
    let mut reached = false;
    for _ in 0..60 {
        press_tab(vcx);
        if console.read_with(vcx.cx, |c, _| {
            vcx.update(|window, _| c.tabstrip_focused_for_test(window))
        }) {
            reached = true;
            break;
        }
    }
    assert!(reached, "STOP-3: the tab strip must be Tab-reachable");
    let before = console.read_with(vcx.cx, |c, _| c.active_tab_for_test());
    assert_eq!(before, 1, "seed: new tab is active");
    log.borrow_mut().clear();
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(vcx.cx, |c, _| c.active_tab_for_test()),
        0,
        "STOP-3: left must switch the active tab 1 → 0"
    );
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Persist)),
        "STOP-3: switching a tab must emit Persist; got {:?}",
        log.borrow()
    );

    // Probe 4: `delete` on the focused tab strip closes a tab (2 → 1).
    vcx.simulate_keystrokes("delete");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(vcx.cx, |c, _| c.tab_count_for_test()),
        1,
        "STOP-4: delete on the focused tab strip must close the active tab"
    );
    drop(state);
}
```

> Note on the `tabstrip_focused_for_test` reach loop: `is_focused` needs a `&Window`. If the nested `vcx.update(...)` inside `read_with` does not type-check in this harness, hoist it: `let f = vcx.update(|window, app| console.read(app).tabstrip_focused_for_test(window));` and test `f` — same semantics, cleaner borrow. Use whichever compiles; the assertion is unchanged.

- [ ] **Step 10: Run the gate**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_nav t0_sql_console_nav_gate -- --nocapture`
Expected: **PASS.**

**STOP-clauses (report + halt if any fires — do NOT proceed to Task 2):**
- **STOP-1** (Run not reachable): confirm `open_console_with_log` opened the console and `focus_shell_neutrally` established focus (mirror `ai_nav`'s `console_ai_triggers_reachable`); check the `.a11y` twin renders (`A11ySnapshot::capture(vcx).has_label(&run)`).
- **STOP-2** (no Run emit): the `focus_stop` `on_activate` isn't firing — verify Enter reaches the focused button (the chip precedent proves the path); check the `run_key` listener is the one passed to `focus_stop`.
- **STOP-3** (tab strip unreachable / `left` doesn't switch): if unreachable, the container `focus_stop` may need the `.id("sql-tabstrip")` to register as a tab stop (added in Step 6) — verify it's present. If reachable but `left` does nothing, the chained 2nd `.on_key_down` is not coexisting with focus_stop's listener — report it (this is the recents-nav R1 property; if it regressed, the whole tablist approach needs review before Task 3).
- **STOP-4** (delete doesn't close): confirm the tab strip still holds focus after the `left` switch (it should — one container handle), and that `close_tab` ran (it no-ops only on the last tab; here 2 tabs are open).

- [ ] **Step 11: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/sql_console.rs crates/dat0-app/tests/sql_console_nav.rs
git commit -s -m "feat(sql-console): keyboard-nav T0 gate — wire sql-run + tab strip

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Remaining toolbar buttons + breadth / operate suite

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs` (wire 6 more buttons in `render`)
- Modify: `crates/dat0-app/tests/sql_console_nav.rs` (add 3 tests)

**Interfaces consumed:** `toolbar_fh` (Task 1) + the copied helpers.

- [ ] **Step 1: Wire the remaining 6 buttons**

Hoist their handles at the top of `render` (next to `run_fh`):

```rust
        let run_pane_fh = self.toolbar_fh("sql-run-pane", cx);
        let new_tab_fh = self.toolbar_fh("sql-tab-add", cx);
        let history_fh = self.toolbar_fh("sql-history", cx);
        let save_fh = self.toolbar_fh("sql-save", cx);
        let saved_fh = self.toolbar_fh("sql-saved", cx);
        let save_as_table_fh = self.toolbar_fh("sql-save-as-table", cx);
```

Then apply this triad to EACH existing button `div().id(<id>)...on_click(<emit>)` — chain `.focus_stop(<id>, &<fh>, 0, <key>).a11y(<id>, AccessRole::Button, dat0_i18n::t(<label-key>))` where `<key>` is a `cx.listener` that emits/does the SAME thing the existing `on_click` does. The exact per-button values:

| element (id) | handle | label key | on_activate (mirror the click) |
|---|---|---|---|
| `sql-run-pane` (▾, ~707) | `run_pane_fh` | `sql.run_in_pane` | `cx.emit(SqlConsoleEvent::Run { target: ResultTarget::Pane })` |
| `sql-tab-add` (+, ~663) | `new_tab_fh` | `sql.new_tab` | `this.new_tab(window, cx)` (use the `window` param of the key listener) |
| `sql-history` (🕘, ~955) | `history_fh` | `sql.history` | `cx.emit(SqlConsoleEvent::ShowHistory)` |
| `sql-save` (💾, ~969) | `save_fh` | `sql.save_query` | `cx.emit(SqlConsoleEvent::SaveQuery)` |
| `sql-saved` (📑, ~983) | `saved_fh` | `sql.load_query` | `cx.emit(SqlConsoleEvent::ShowSaved)` |
| `sql-save-as-table` (⤓, ~998) | `save_as_table_fh` | `sql.save_as_table` | `cx.emit(SqlConsoleEvent::SaveAsTable)` |

Concrete example for `sql-history` (all others follow identically; `sql-tab-add`'s key listener takes `window` because `new_tab` needs it, and `sql-run-pane` is inside the idle-only `run_caret` branch so it stays a tab stop only when idle — exactly the desired behavior):

```rust
        div()
            .id("sql-history")
            .px_2()
            .py_1()
            .cursor_pointer()
            .child(SharedString::from("🕘"))
            .focus_stop(
                "sql-history",
                &history_fh,
                0,
                cx.listener(|_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                    cx.emit(SqlConsoleEvent::ShowHistory);
                }),
            )
            .a11y("sql-history", AccessRole::Button, dat0_i18n::t("sql.history"))
            .on_click(cx.listener(|_this, _ev, _window, cx| {
                cx.emit(SqlConsoleEvent::ShowHistory);
            }))
```

For `sql-tab-add`, the key listener mirrors the click's inline `new_tab`:

```rust
            .focus_stop(
                "sql-tab-add",
                &new_tab_fh,
                0,
                cx.listener(|this, _ev: &gpui::KeyDownEvent, window, cx| {
                    this.new_tab(window, cx);
                }),
            )
            .a11y("sql-tab-add", AccessRole::Button, dat0_i18n::t("sql.new_tab"))
```

- [ ] **Step 2: Build with the feature on**

Run: `cargo build -p dat0-app --features a11y-capture`
Expected: clean.

- [ ] **Step 3: Write the breadth + operate tests**

Add to `sql_console_nav.rs`:

```rust
/// Every fixed toolbar button is Tab-reachable (labels appear as Tab walks the
/// console). Uses the label oracle — each button carries its localized `.a11y`
/// twin (glyph child, text label).
#[gpui::test]
#[serial]
fn toolbar_buttons_reachable(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, _log) = open_console_with_log(&shell, vcx);

    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 80);
    for key in [
        "sql.run",
        "sql.run_in_pane",
        "sql.new_tab",
        "sql.history",
        "sql.save_query",
        "sql.load_query",
        "sql.save_as_table",
    ] {
        let label = dat0_i18n::t(key);
        assert!(seen.contains(&label), "{key} ({label:?}) Tab-reachable; visited {seen:?}");
    }
    drop(state);
}

/// Enter on the focused Run button emits `Run { MainGrid }` while idle.
#[gpui::test]
#[serial]
fn enter_on_run_emits_run(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, log) = open_console_with_log(&shell, vcx);

    let run = dat0_i18n::t("sql.run");
    focus_shell_neutrally(vcx);
    tab_until(vcx, &run);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow().iter().any(|e| matches!(
            e,
            SqlConsoleEvent::Run { target: ResultTarget::MainGrid }
        )),
        "Enter on Run must emit Run{{MainGrid}}; got {:?}",
        log.borrow()
    );
    drop(state);
}

/// While running, the same control shows Cancel and Enter emits Cancel.
#[gpui::test]
#[serial]
fn enter_on_run_while_running_emits_cancel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, log) = open_console_with_log(&shell, vcx);

    // Force the running state so the primary button is Cancel.
    vcx.update(|_w, app| console.update(app, |c, cx| c.set_running(true, cx)));
    vcx.run_until_parked();

    let cancel = dat0_i18n::t("sql.cancel");
    focus_shell_neutrally(vcx);
    tab_until(vcx, &cancel);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow().iter().any(|e| matches!(e, SqlConsoleEvent::Cancel)),
        "Enter on the running Cancel button must emit Cancel; got {:?}",
        log.borrow()
    );
    drop(state);
}
```

> `set_running` is `SqlConsole`'s existing public setter for the running flag (used by the shell's run lifecycle). If its exact name/signature differs (e.g. it takes no `cx`, or is named `set_running_state`), adjust the one call site; the assertion is unchanged. If no public setter exists, add a trivial `#[cfg(feature = "a11y-capture")] pub fn set_running_for_test(&mut self, v: bool, cx: &mut Context<Self>) { self.running = v; cx.notify(); }` accessor (placed in the same gated block as the others).

- [ ] **Step 4: Run the suite**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_nav -- --nocapture`
Expected: **all PASS** (t0 gate + these 3).

- [ ] **Step 5: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/sql_console.rs crates/dat0-app/tests/sql_console_nav.rs
git commit -s -m "feat(sql-console): wire remaining toolbar buttons + breadth/operate suite

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Tab-strip behavioral suite + closed-console negative

**Files:**
- Modify: `crates/dat0-app/tests/sql_console_nav.rs` (add 4 tests)

**Interfaces consumed:** Task 1 accessors + copied helpers.

- [ ] **Step 1: A reach helper for the tab strip**

Add near the top of the test file (after the copied helpers):

```rust
/// Press Tab (up to `budget` times) until the tab-strip container holds focus.
/// Returns true if reached. Mirrors `tab_until` but keys off the focus accessor
/// (the tab strip's accessible name is the dynamic active-tab title).
fn tab_until_tabstrip(vcx: &mut VisualTestContext, console: &Entity<SqlConsole>, budget: usize) -> bool {
    for _ in 0..budget {
        press_tab(vcx);
        let f = vcx.update(|window, app| console.read(app).tabstrip_focused_for_test(window));
        if f {
            return true;
        }
    }
    false
}
```

- [ ] **Step 2: Write the four tests**

```rust
/// Seed 3 tabs; from the focused tab strip, ← / → move the active tab and clamp
/// at both ends.
#[gpui::test]
#[serial]
fn tabstrip_arrows_switch_and_clamp(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, _log) = open_console_with_log(&shell, vcx);

    // 1 → 3 tabs (each new_tab makes itself active; end active == 2).
    for _ in 0..2 {
        vcx.update(|window, app| console.update(app, |c, cx| c.new_tab(window, cx)));
        vcx.run_until_parked();
    }
    assert_eq!(console.read_with(vcx.cx, |c, _| c.tab_count_for_test()), 3);

    focus_shell_neutrally(vcx);
    assert!(tab_until_tabstrip(vcx, &console, 60), "tab strip reachable");
    assert_eq!(console.read_with(vcx.cx, |c, _| c.active_tab_for_test()), 2);

    // → at the right edge clamps (stays 2).
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert_eq!(console.read_with(vcx.cx, |c, _| c.active_tab_for_test()), 2, "right clamps at end");

    // ← walks back to 0 and clamps.
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(console.read_with(vcx.cx, |c, _| c.active_tab_for_test()), 1);
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(console.read_with(vcx.cx, |c, _| c.active_tab_for_test()), 0);
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert_eq!(console.read_with(vcx.cx, |c, _| c.active_tab_for_test()), 0, "left clamps at start");

    // → moves forward again (auto-activate).
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert_eq!(console.read_with(vcx.cx, |c, _| c.active_tab_for_test()), 1);
    drop(state);
}

/// Delete on the focused tab strip closes the active tab (count drops, active
/// clamps).
#[gpui::test]
#[serial]
fn tabstrip_delete_closes_active(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, _log) = open_console_with_log(&shell, vcx);

    vcx.update(|window, app| console.update(app, |c, cx| c.new_tab(window, cx)));
    vcx.run_until_parked();
    assert_eq!(console.read_with(vcx.cx, |c, _| c.tab_count_for_test()), 2);

    focus_shell_neutrally(vcx);
    assert!(tab_until_tabstrip(vcx, &console, 60), "tab strip reachable");
    vcx.simulate_keystrokes("delete");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(vcx.cx, |c, _| c.tab_count_for_test()),
        1,
        "delete closes the active tab"
    );
    drop(state);
}

/// Delete with a single tab open is a no-op (never an empty console).
#[gpui::test]
#[serial]
fn tabstrip_delete_last_tab_is_noop(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (console, _log) = open_console_with_log(&shell, vcx);

    assert_eq!(console.read_with(vcx.cx, |c, _| c.tab_count_for_test()), 1);
    focus_shell_neutrally(vcx);
    assert!(tab_until_tabstrip(vcx, &console, 60), "tab strip reachable");
    vcx.simulate_keystrokes("delete");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(vcx.cx, |c, _| c.tab_count_for_test()),
        1,
        "delete on the last tab must be a no-op"
    );
    drop(state);
}

/// With the console CLOSED, none of the new toolbar labels are Tab stops (the
/// console render doesn't paint, so the `.a11y` twins are absent).
#[gpui::test]
#[serial]
fn toolbar_not_tab_stops_when_console_closed(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (_shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // Do NOT open the console.

    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 60);
    let run = dat0_i18n::t("sql.run");
    assert!(
        !seen.contains(&run),
        "with the console closed, sql-run must not be a Tab stop; visited {seen:?}"
    );
    drop(state);
}
```

- [ ] **Step 3: Run the full binary**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_nav -- --nocapture`
Expected: **all PASS** (t0 gate + 3 Task-2 + 4 Task-3 = 8 tests).

- [ ] **Step 4: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/sql_console_nav.rs
git commit -s -m "test(sql-console): tab-strip behavioral suite + closed-console negative

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Controller gate + final review

**Files:** none (verification only).

- [ ] **Step 1: Workspace test gate (catches cross-binary drift)**

Run: `cargo test -p dat0-app --features a11y-capture --workspace --no-fail-fast`
Expected: PASS. In particular `a11y_spike` must be UNCHANGED — its scene keeps the SQL Console CLOSED, so the new (unconditional) `.a11y` nodes never paint there (same as the AI-config slice). If any binary's assertion moved, investigate before proceeding.

- [ ] **Step 2: Clippy + fmt gate (pinned 1.97.0)**

Run: `cargo clippy -p dat0-app --features a11y-capture --all-targets -- -D warnings`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`
Expected: all clean. (`items-after-test-module` catches a mis-placed accessor block.)

- [ ] **Step 3: Prove the release build compiles (real a11y, no test leak)**

Run: `cargo build -p dat0-app` (feature OFF — default)
Run: `cargo build -p dat0-app --release`
Expected: both clean. The `focus_stop`/`a11y` wiring compiles with the feature off (real code); the `_for_test` accessors are absent from a default/release build (they are `#[cfg(feature = "a11y-capture")]`).

- [ ] **Step 4: Confirm no dependency / manifest drift**

Run: `git diff main -- Cargo.toml Cargo.lock NOTICE crates/dat0-app/Cargo.toml crates/dat0-i18n/src/strings/en.json`
Expected: EMPTY. Zero new deps; zero new i18n keys (Global Constraints).

- [ ] **Step 5: Final whole-branch review (opus) + push/PR**

Dispatch a fresh-context whole-branch review (opus) checking: the wiring is minimal and mirrors the shipped chip precedent; every `focus_stop` carries a same-id `.a11y` twin; each `on_activate` re-emits the SAME event as its `on_click` (no drift); the tab-strip chained `on_key_down` coexists with focus_stop's listener and `self.active` stays the single source of truth (no orphaned focus on close); tests are non-vacuous (reach would fail if a stop were missing; switch/close read a value that actually changed; the closed-console negative would fail if a label leaked); zero production behavior change beyond the new keyboard reachability; zero deps / zero new i18n keys. Address Critical/Important; fold Minors. Then push `uat-sql-console-nav` and open the PR. **Watch the post-merge main run** — the macOS grid-scroll bench is push-to-main-only and can redden main silently.

---

## Self-Review

**Spec coverage** (design §Architecture + §Testing):
- 7 fixed toolbar buttons wired (per-button focus_stop + a11y) → Task 1 (`sql-run`) + Task 2 (6 more). ✓
- Tab strip = one container focus_stop + `self.active` + ←/→ auto-switch + Delete-close → Task 1 Step 6. ✓
- Editor/results widgets + overlay controls NOT touched → no task adds them. ✓
- T0 hard gate (4 probes, STOP-clauses) → Task 1. ✓
- Behavioral suite (breadth, Run/Cancel operate, tab switch/clamp/close/no-op, closed-negative) → Tasks 2–3. ✓
- Real production a11y (unconditional wiring), only `_for_test` gated → Steps throughout + Task 4 Step 3. ✓
- Zero deps, zero new i18n keys → Global Constraints + Task 4 Step 4. ✓
- `a11y_spike` zero-drift expectation → Task 4 Step 1. ✓
- Owed human glance (ring contrast) → carried in the design; no code. ✓

**Placeholder scan:** no TBD/TODO; every code step shows full code or a verbatim-copy instruction; STOP-clauses give concrete fallbacks. The two "if the exact name differs, adjust" notes (`set_running`, the `tabstrip_focused_for_test` borrow) name the precise call site + the unchanged assertion — not open-ended hand-waving. ✓

**Type consistency:** `toolbar_fh` / `active_tab_for_test` / `tab_count_for_test` / `tabstrip_focused_for_test` / `tab_until_tabstrip` — names identical across Tasks 1–3. `SqlConsoleEvent::{Run{target: ResultTarget::MainGrid|Pane}, Cancel, Persist, ShowHistory, SaveQuery, ShowSaved, SaveAsTable}` used consistently with the enum at `sql_console.rs:204-237`. `focus_stop(id, &fh, 0, key)` matches the a11y-kit signature. Label keys match `en.json` verbatim. ✓
