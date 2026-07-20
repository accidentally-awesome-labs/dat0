# SQL-console `Input` keyboard operability — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the SQL editor's keyboard trap (Escape → focus Run) and make the shared `NamePrompt` modal fully keyboard-operable (focus-on-open, Enter-submit, Escape-cancel, keyboard-reachable OK/Cancel), with windowed behavioral coverage — real shipped a11y across all 5 prompt call sites.

**Architecture:** Surface 1 — an ancestor `on_action::<input::Escape>` on the `SqlConsole` render root moves focus to the existing `sql-run` `FocusHandle` when (and only when) the active editor holds focus; gpui bubble order means an open autocomplete popup consumes Escape first, so the observable behavior is "close popup, else leave editor onto Run". Surface 2 — inside the shared `NamePrompt` component: focus the input on open, subscribe to its `InputEvent::PressEnter` → emit `Confirm`, add an `on_action::<input::Escape>` → emit `Cancel`, and give the OK/Cancel buttons the shipped `focus_stop`+`.a11y` triad. One change fixes all 5 call sites (NL→SQL, Save-query, Save-as-table, AI-key, MD-token).

**Tech Stack:** Rust, gpui 0.2.2, gpui-component (pinned git; `InputState`/`Input`/`InputEvent`/`Enter`/`Escape`), the in-repo a11y kit (`FocusStopExt`/`A11yExt`/`focused_label`), `tempfile`, `serial_test` (all existing dev-deps — **zero new deps**).

## Global Constraints

- **Zero new dependencies.** `Cargo.toml` / `Cargo.lock` / `NOTICE` unchanged. D-015 stays open.
- **Ships real production a11y.** The `on_action`/`focus_stop`/`.a11y`/subscription/focus-on-open wiring is UNCONDITIONAL (not feature-gated). Only the `_for_test` read/seam accessors are `#[cfg(feature = "a11y-capture")]`.
- **Zero new i18n keys.** The NamePrompt OK/Cancel buttons already display the literal strings `"Save"` / `"Cancel"` — reuse those verbatim as the `.a11y` labels. No `en.json` change.
- **No new event variants** — reuse `NamePromptEvent::{Confirm(String), Cancel}` and `SqlConsoleEvent`. No change to `on_name_prompt_event`'s routing.
- **a11y imports stay UNCONDITIONAL** — mirror `sql_console.rs:31` (`use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};`); do NOT `#[cfg]`-gate.
- **Toolchain pinned 1.97.0.** `cargo fmt --all` before EVERY commit. DCO: every commit `git commit -s`.
- **Test-only symbols go BEFORE any `#[cfg(test)] mod tests`** in a source file (clippy `items-after-test-module` under `-D warnings`).
- **Implementers run only the focused test** (`cargo test -p dat0-app --features a11y-capture --test input_nav`); the controller runs the `cargo test --workspace --no-fail-fast` + `clippy --workspace --all-targets -D warnings` gate.
- **Harness helpers are COPIED VERBATIM per-binary** from `tests/sql_console_nav.rs` — established codebase convention, NOT a duplication defect.
- Commit-message trailer on every commit: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Create:** `crates/dat0-app/tests/input_nav.rs` — the whole slice's tests + a per-binary copy of `sql_console_nav.rs`'s mount/console-open helpers.
- **Modify:** `crates/dat0-app/src/view/name_prompt.rs` — focus-on-open + `PressEnter`→Confirm subscription + `Escape`→Cancel + OK/Cancel `focus_stop`+`.a11y`; new fields; a `#[cfg(feature="a11y-capture")]` accessor block.
- **Modify:** `crates/dat0-app/src/view/sql_console.rs` — the editor `Escape`→focus-Run `on_action` on the render root; a `#[cfg(feature="a11y-capture")]` editor-focus accessor.
- **Modify:** `crates/dat0-app/src/window.rs` — a `#[cfg(feature="a11y-capture")]` accessor block on `WorkspaceShell` to open + inspect a `NamePrompt` from a test.

No other production files change.

---

## Task 1: T0 HARD GATE — all production wiring + 4-probe spike

**This is the load-bearing gate.** It ships both surfaces' real wiring and proves — in one kept windowed test — the three drive mechanisms the slice rests on, BEFORE the breadth suites. If a STOP-clause fires, stop and report; do not build Tasks 2–3 on an unproven drive.

**Files:**
- Modify: `name_prompt.rs`, `sql_console.rs`, `window.rs`
- Create: `crates/dat0-app/tests/input_nav.rs`

**Interfaces produced (used by Tasks 2–3):**
- `SqlConsole::editor_focus_handle_for_test(&self, cx: &gpui::App) -> gpui::FocusHandle`
- `SqlConsole::editor_focused_for_test(&self, window: &gpui::Window, cx: &gpui::App) -> bool`
- `WorkspaceShell::open_name_prompt_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>)`
- `WorkspaceShell::name_prompt_open_for_test(&self) -> bool`
- `WorkspaceShell::name_prompt_entity_for_test(&self) -> Option<Entity<crate::view::name_prompt::NamePrompt>>`
- `NamePrompt::input_focused_for_test(&self, window: &Window, cx: &gpui::App) -> bool`
- `NamePrompt::seed_value_for_test(&self, value: &str, window: &mut Window, cx: &mut gpui::App)`
- Test helpers copied verbatim from `sql_console_nav.rs` (listed in Step 6).

- [ ] **Step 1: `NamePrompt` — new fields + imports + `new` wiring**

In `name_prompt.rs`, replace the imports block (lines 3–5) with:

```rust
use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, ParentElement, SharedString, Styled, Subscription,
    Window, div,
};
use gpui_component::input::{Escape, Input, InputEvent, InputState};
```

Replace the struct (lines 13–16) with:

```rust
pub struct NamePrompt {
    title: SharedString,
    input: Entity<InputState>,
    /// Focus stops for the OK/Cancel buttons (SQL-input-nav slice) — so the
    /// modal is fully keyboard-operable, not just click-operable.
    ok_focus: FocusHandle,
    cancel_focus: FocusHandle,
    /// Keeps the `input`→`PressEnter` subscription alive for the prompt's life
    /// (Enter in the field submits, mirroring the OK button).
    _enter_sub: Subscription,
}
```

Replace the body of `new` (lines 28–38 — the `let initial …` through the `Self { … }`) with:

```rust
        let initial = initial.into();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("name")
                .default_value(initial)
        });
        // Enter in the single-line field submits. `enter()` emits `PressEnter`
        // and `cx.propagate()`s; nothing consumed it before (the prompt was
        // mouse-only). Subscribing here fixes Enter-submit for ALL 5 call sites.
        let enter_sub = cx.subscribe(&input, |this, _input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                let v = this.value(cx);
                cx.emit(NamePromptEvent::Confirm(v));
            }
        });
        // Focus the field on open so a keyboard user can type immediately and
        // Tab/Escape work. `new` holds `&mut Window`; the pending focus lands on
        // the input when it next renders with `.track_focus`.
        window.focus(&input.read(cx).focus_handle(cx));
        Self {
            title: title.into(),
            input,
            ok_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            _enter_sub: enter_sub,
        }
```

- [ ] **Step 2: `NamePrompt::render` — Escape→Cancel + button `focus_stop`/`.a11y`**

Replace the whole `render` body (lines 48–85) with (the `on_activate` closures mirror each button's existing `on_click` exactly):

```rust
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ok_fh = self.ok_focus.clone();
        let cancel_fh = self.cancel_focus.clone();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            // Escape cancels. A single-line `escape()` is a no-op that
            // `cx.propagate()`s, so this ancestor `on_action` catches it.
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(NamePromptEvent::Cancel);
            }))
            .child(self.title.clone())
            .child(Input::new(&self.input))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("name-prompt-ok")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .child(SharedString::from("Save"))
                            .focus_stop(
                                "name-prompt-ok",
                                &ok_fh,
                                0,
                                cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
                                    let v = this.value(cx);
                                    cx.emit(NamePromptEvent::Confirm(v));
                                }),
                            )
                            .a11y("name-prompt-ok", AccessRole::Button, "Save")
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                let v = this.value(cx);
                                cx.emit(NamePromptEvent::Confirm(v));
                            })),
                    )
                    .child(
                        div()
                            .id("name-prompt-cancel")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .child(SharedString::from("Cancel"))
                            .focus_stop(
                                "name-prompt-cancel",
                                &cancel_fh,
                                0,
                                cx.listener(|_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                                    cx.emit(NamePromptEvent::Cancel);
                                }),
                            )
                            .a11y("name-prompt-cancel", AccessRole::Button, "Cancel")
                            .on_click(cx.listener(|_this, _ev, _window, cx| {
                                cx.emit(NamePromptEvent::Cancel);
                            })),
                    ),
            )
    }
```

- [ ] **Step 3: `NamePrompt` — test accessors**

In `name_prompt.rs`, immediately BEFORE the `impl EventEmitter<NamePromptEvent> for NamePrompt {}` line, add:

```rust
#[cfg(feature = "a11y-capture")]
impl NamePrompt {
    /// Whether the prompt's text field currently holds focus (proves
    /// focus-on-open).
    pub fn input_focused_for_test(&self, window: &Window, cx: &gpui::App) -> bool {
        self.input.read(cx).focus_handle(cx).is_focused(window)
    }

    /// Seed the field's text without keystrokes (so a test can assert the
    /// submitted value round-trips through `Confirm`).
    pub fn seed_value_for_test(&self, value: &str, window: &mut Window, cx: &mut gpui::App) {
        self.input
            .update(cx, |s, cx| s.set_value(value, window, cx));
    }
}
```

- [ ] **Step 4: `SqlConsole` — editor Escape→focus-Run on the render root**

In `sql_console.rs`, at the END of the root `div()` chain returned by `render` — immediately after `.children(history_overlay)` (currently the last call, ~line 1345) — append:

```rust
            // Escape leaves the editor onto Run (fixes the code-editor keyboard
            // trap: Tab/Shift-Tab indent in multi-line mode). Guarded on the
            // active editor holding focus, so (a) when the autocomplete popup is
            // open the editor consumes Escape first and this never fires, and
            // (b) Escape while a toolbar button is focused is left alone. `run_fh`
            // is the same handle the Run button's `focus_stop` uses, so focus
            // lands on Run and Tab/Shift-Tab resume.
            .on_action(cx.listener(
                |this, _ev: &gpui_component::input::Escape, window, cx| {
                    if this.tabs[this.active]
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                    {
                        let run_fh = this.toolbar_fh("sql-run", cx);
                        window.focus(&run_fh);
                        cx.notify();
                    }
                },
            ))
```

- [ ] **Step 5: `SqlConsole` — editor-focus test accessors**

In `sql_console.rs`, inside the existing `#[cfg(feature = "a11y-capture")] impl SqlConsole { … }` block (the one holding `active_tab_for_test` etc., ~line 1349), add:

```rust
    /// The active tab's editor `FocusHandle` — lets a test focus the editor
    /// directly (it is a native tab-stop but not part of the `focus_stop` kit).
    pub fn editor_focus_handle_for_test(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.tabs[self.active].input.read(cx).focus_handle(cx)
    }

    /// Whether the active editor holds focus (the trap-exit oracle).
    pub fn editor_focused_for_test(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        self.tabs[self.active]
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }
```

- [ ] **Step 6: `WorkspaceShell` — test accessors to open/inspect a NamePrompt**

In `window.rs`, find the existing `#[cfg(feature = "a11y-capture")] impl WorkspaceShell { … }` block (the shims like `seed_catalog_tree_for_test` live there, ~line 6591). Add:

```rust
    /// Open a `NamePrompt` from a test using a side-effect-free intent
    /// (`SaveQuery` with no stashed SQL → `Confirm` is a clean no-op dismiss),
    /// so the generic prompt keyboard behavior can be driven without AI/engine.
    pub fn open_name_prompt_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_prompt_sql = None;
        self.open_name_prompt_with("Test", "", NamePromptIntent::SaveQuery, window, cx);
    }

    /// Whether the name-prompt overlay is currently mounted.
    pub fn name_prompt_open_for_test(&self) -> bool {
        self.name_prompt.is_some()
    }

    /// The live prompt entity — lets a test subscribe to its `NamePromptEvent`
    /// and read/seed its input.
    pub fn name_prompt_entity_for_test(
        &self,
    ) -> Option<gpui::Entity<crate::view::name_prompt::NamePrompt>> {
        self.name_prompt.clone()
    }
```

> `NamePromptIntent` is already in scope in `window.rs` (used by `open_name_prompt_with`). If the field is named other than `name_prompt`/`name_prompt_sql`, use the real field names (grep `self.name_prompt`); the accessor bodies are otherwise unchanged.

- [ ] **Step 7: Build with the feature on**

Run: `cargo build -p dat0-app --features a11y-capture`
Expected: builds clean. If `.on_action` on the plain root `div()` does not resolve, that is a compile error, not the runtime STOP — fix by confirming the `Escape`/`InputEvent` import paths; the runtime "does the ancestor handler fire" question is Step 10's Probe 1.

- [ ] **Step 8: Create the test file with copied helpers**

Create `crates/dat0-app/tests/input_nav.rs`. **Copy VERBATIM from `tests/sql_console_nav.rs`**: the `mod support;` line, the imports block (keep `SqlConsole`, `SqlConsoleEvent`, `WorkspaceShell`, `Session`, `A11ySnapshot`, `press_tab`, `ResultTarget`; add `use dat0_app::view::name_prompt::{NamePrompt, NamePromptEvent};`), the `BUDGET` const, `set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`, `focus_shell_neutrally`, `AsyncHarness` + `enter_async_harness`, `open_console_with_log`, `tab_labels`, `tab_until`. Header:

```rust
//! SQL-console `Input` keyboard-operability coverage (UAT carve-out #6).
//!
//! Windowed tests driving the SHIPPED SQL editor trap-exit (Escape → Run) and
//! the shared `NamePrompt` modal (focus-on-open, Enter-submit, Escape-cancel,
//! keyboard-reachable OK/Cancel) through the real dispatch path. Harness helpers
//! are copied per-binary from `tests/sql_console_nav.rs` (per-binary-copy precedent).
//! Inputs are driven with `cx.dispatch_action(...)`, NEVER `simulate_keystrokes`
//! (the cell-editor slice proved a stray "\n" panics a single-line Input).
```

- [ ] **Step 9: Write the T0 gate spike**

Add to `input_nav.rs`:

```rust
/// T0 HARD GATE — proves the three drive mechanisms the slice rests on:
///   Probe 1: focus the editor, `dispatch_action(Escape)` → focus lands on Run
///            (the ancestor Escape→Run handler fires; editor no longer focused).
///   Probe 2: with the editor focused, the run shortcut still runs — dispatch the
///            `SqlRun` menu action and assert the console reflects a run request.
///   Probe 3: open a NamePrompt → its field is focused; seed a value;
///            `dispatch_action(Enter)` → `Confirm(value)` emitted + overlay dismissed.
///   Probe 4: re-open → `dispatch_action(Escape)` → `Cancel` emitted + dismissed;
///            OK/Cancel are Tab-reachable (labels "Save"/"Cancel").
#[gpui::test]
#[serial]
fn t0_input_nav_gate(cx: &mut TestAppContext) {
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

    // ── Probe 1: editor trap-exit (Escape → Run) ────────────────────────────
    let editor_fh = vcx.update(|_w, app| console.read(app).editor_focus_handle_for_test(app));
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "precondition: editor focused"
    );
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        !vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "STOP-1: Escape must move focus OUT of the editor"
    );
    let run = dat0_i18n::t("sql.run");
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(run.clone()),
        "STOP-1: Escape must land focus on Run; got {:?}",
        A11ySnapshot::capture(vcx).focused_label()
    );

    // ── Probe 2: run shortcut still works from the editor ────────────────────
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    log.borrow_mut().clear();
    vcx.dispatch_action(dat0_app::menu_macos::SqlRun);
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Run { .. })),
        "STOP-2: the run action must produce a Run while the editor is focused; got {:?}",
        log.borrow()
    );

    // ── Probe 3: NamePrompt opens focused, Enter submits ─────────────────────
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.open_name_prompt_for_test(window, cx)));
    vcx.run_until_parked();
    let prompt = vcx
        .update(|_w, app| shell.read(app).name_prompt_entity_for_test())
        .expect("STOP-3: prompt must be open");
    assert!(
        vcx.update(|window, app| prompt.read(app).input_focused_for_test(window, app)),
        "STOP-3: the prompt field must be focused on open"
    );
    let plog: std::rc::Rc<std::cell::RefCell<Vec<NamePromptEvent>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let plog2 = plog.clone();
    let psub = vcx.cx.update(|app| {
        app.subscribe(&prompt, move |_p, ev: &NamePromptEvent, _app| {
            plog2.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(psub);
    vcx.update(|window, app| prompt.read(app).seed_value_for_test("hello", window, app));
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Enter { secondary: false });
    vcx.run_until_parked();
    assert!(
        plog.borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Confirm(v) if v == "hello")),
        "STOP-3: Enter must emit Confirm(value); got {:?}",
        plog.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "STOP-3: Confirm must dismiss the overlay"
    );

    // ── Probe 4: re-open, Escape cancels; OK/Cancel Tab-reachable ────────────
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.open_name_prompt_for_test(window, cx)));
    vcx.run_until_parked();
    let prompt2 = vcx
        .update(|_w, app| shell.read(app).name_prompt_entity_for_test())
        .expect("prompt re-open");
    let plog3 = plog.clone();
    let psub2 = vcx.cx.update(|app| {
        app.subscribe(&prompt2, move |_p, ev: &NamePromptEvent, _app| {
            plog3.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(psub2);
    let seen = tab_labels(vcx, 30);
    assert!(
        seen.contains(&"Save".to_string()) && seen.contains(&"Cancel".to_string()),
        "STOP-4: OK/Cancel must be Tab stops; visited {seen:?}"
    );
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        plog.borrow().iter().any(|e| matches!(e, NamePromptEvent::Cancel)),
        "STOP-4: Escape must emit Cancel; got {:?}",
        plog.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "STOP-4: Cancel must dismiss the overlay"
    );
    drop(state);
}
```

- [ ] **Step 10: Run the gate**

Run: `cargo test -p dat0-app --features a11y-capture --test input_nav t0_input_nav_gate -- --nocapture`
Expected: **PASS.**

**STOP-clauses (report + halt if any fires — do NOT proceed to Task 2):**
- **STOP-1** (Escape doesn't exit / doesn't land on Run): the ancestor `on_action::<Escape>` on the `SqlConsole` root isn't on the dispatch path or isn't firing. First fallback: add `.id("sql-console-root")` to the root `div()` (a stateful div registers a dispatch node); second: move the `on_action` onto the editor's wrapper `div().flex_1().child(editor)` at `sql_console.rs:1243`. If focus exits but `focused_label()` isn't `sql.run`, confirm `toolbar_fh("sql-run", cx)` returns the SAME handle the Run button's `focus_stop` uses (it does — same map key). Report if the mechanism is fundamentally unavailable.
- **STOP-2** (run action produces no Run — or `dat0_app::menu_macos::SqlRun` won't compile from the test because `menu_macos`/`SqlRun` isn't `pub`): (a) visibility — if the action type isn't reachable from the integration crate, prefer driving the real platform shortcut with `vcx.simulate_keystrokes("cmd-enter")` on macOS / `"ctrl-enter"` elsewhere (the editor is multi-line, so the single-line "\n" panic does not apply, and `SqlRun` should win the keymap tie and consume it before the editor) and assert the same `Run` observable; only if that is also unavailable, make `menu_macos` + `SqlRun` `pub` (a one-line visibility widening — note it as a deliberate, reviewed production change). (b) observable — if the run drives without an observable `SqlConsoleEvent::Run`, inspect the `SqlRun` handler on `WorkspaceShell` (grep `SqlRun` in `window.rs`; ~6631) and assert the real observable instead (e.g. a `running` flag via a new gated `is_running_for_test()`); the point is "the run shortcut works while the editor is focused", not the specific event. Report whichever adjustment was needed.
- **STOP-3 / STOP-4** (prompt not focused on open / Enter or Escape doesn't route / not dismissed): if not focused on open, move the `window.focus(&…)` from `NamePrompt::new` into a focus-on-first-render guard (add a `focused_once: bool` field; focus in `render` when `!focused_once`). If Enter/Escape don't route, verify the `PressEnter` subscription is retained (`_enter_sub`) and that the Escape `on_action` sits on an ancestor of the focused input (add `.id("name-prompt-root")` to the prompt root as the fallback). Report which fallback was needed.

- [ ] **Step 11: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/name_prompt.rs crates/dat0-app/src/view/sql_console.rs crates/dat0-app/src/window.rs crates/dat0-app/tests/input_nav.rs
git commit -s -m "feat(sql-console): Input keyboard-nav T0 gate — editor Escape→Run + NamePrompt operable

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: SQL editor behavioral suite

**Files:**
- Modify: `crates/dat0-app/tests/input_nav.rs` (add 3 tests)

**Interfaces consumed:** Task 1 accessors + copied helpers.

- [ ] **Step 1: Write the editor tests**

Add to `input_nav.rs`:

```rust
/// Escape from the focused editor lands focus on Run, then Tab/Shift-Tab resume
/// normal navigation (proves the trap is genuinely broken open).
#[gpui::test]
#[serial]
fn editor_escape_exits_to_run_then_tab_resumes(cx: &mut TestAppContext) {
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

    let editor_fh = vcx.update(|_w, app| console.read(app).editor_focus_handle_for_test(app));
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.run")),
        "Escape lands on Run"
    );
    // Focus can now leave Run by Tab (no longer trapped): the shift-tab neighbor
    // is a real toolbar stop, proving nav resumed.
    press_tab(vcx);
    assert!(
        A11ySnapshot::capture(vcx).focused_label().is_some(),
        "Tab from Run reaches another stop (nav resumed, not trapped)"
    );
    drop(state);
}

/// Escape does nothing observable when the editor is NOT focused (the guard):
/// focus a toolbar button, Escape, and the focus label is unchanged (Run is not
/// force-grabbed).
#[gpui::test]
#[serial]
fn editor_escape_guarded_to_editor_focus(cx: &mut TestAppContext) {
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
    tab_until(vcx, &dat0_i18n::t("sql.history"));
    let before = A11ySnapshot::capture(vcx).focused_label();
    assert_eq!(before, Some(dat0_i18n::t("sql.history")));
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        before,
        "Escape while a non-editor stop is focused must not hijack focus to Run"
    );
    drop(state);
}

/// The run shortcut (SqlRun action) produces a Run while the editor is focused.
#[gpui::test]
#[serial]
fn run_shortcut_works_from_editor(cx: &mut TestAppContext) {
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

    let editor_fh = vcx.update(|_w, app| console.read(app).editor_focus_handle_for_test(app));
    vcx.update(|window, _| window.focus(&editor_fh));
    vcx.run_until_parked();
    log.borrow_mut().clear();
    vcx.dispatch_action(dat0_app::menu_macos::SqlRun);
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::Run { .. })),
        "run action from the editor produces a Run; got {:?}",
        log.borrow()
    );
    drop(state);
}
```

> If Task 1's STOP-2 adjustment changed the run observable, mirror the same assertion here (`is_running_for_test()` etc.).

- [ ] **Step 2: Run the suite**

Run: `cargo test -p dat0-app --features a11y-capture --test input_nav -- --nocapture`
Expected: **all PASS** (t0 gate + these 3).

- [ ] **Step 3: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/input_nav.rs
git commit -s -m "test(sql-console): SQL editor trap-exit + run-shortcut behavioral suite

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: NamePrompt behavioral suite

**Files:**
- Modify: `crates/dat0-app/tests/input_nav.rs` (add 3 tests)

**Interfaces consumed:** Task 1 accessors + copied helpers.

- [ ] **Step 1: Write the prompt tests**

Add to `input_nav.rs`. A small local helper opens a prompt and returns (entity, event-log):

```rust
/// Open a NamePrompt and subscribe to its events. Returns (prompt, log).
fn open_prompt_with_log(
    shell: &Entity<WorkspaceShell>,
    vcx: &mut VisualTestContext,
) -> (Entity<NamePrompt>, std::rc::Rc<std::cell::RefCell<Vec<NamePromptEvent>>>) {
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.open_name_prompt_for_test(window, cx)));
    vcx.run_until_parked();
    let prompt = vcx
        .update(|_w, app| shell.read(app).name_prompt_entity_for_test())
        .expect("prompt open");
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log2 = log.clone();
    let sub = vcx.cx.update(|app| {
        app.subscribe(&prompt, move |_p, ev: &NamePromptEvent, _app| {
            log2.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(sub);
    (prompt, log)
}

/// The prompt field is focused on open (no click needed).
#[gpui::test]
#[serial]
fn prompt_focused_on_open(cx: &mut TestAppContext) {
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

    let (prompt, _plog) = open_prompt_with_log(&shell, vcx);
    assert!(
        vcx.update(|window, app| prompt.read(app).input_focused_for_test(window, app)),
        "the prompt field is focused on open"
    );
    drop(state);
}

/// Enter submits the typed value; the overlay dismisses.
#[gpui::test]
#[serial]
fn prompt_enter_confirms_value(cx: &mut TestAppContext) {
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

    let (prompt, plog) = open_prompt_with_log(&shell, vcx);
    vcx.update(|window, app| prompt.read(app).seed_value_for_test("my_query", window, app));
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Enter { secondary: false });
    vcx.run_until_parked();
    assert!(
        plog.borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Confirm(v) if v == "my_query")),
        "Enter emits Confirm(value); got {:?}",
        plog.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "Confirm dismisses the overlay"
    );
    drop(state);
}

/// Escape cancels and dismisses; OK/Cancel are keyboard-reachable + operable
/// (Enter on the focused Cancel button emits Cancel).
#[gpui::test]
#[serial]
fn prompt_escape_cancels_and_buttons_operable(cx: &mut TestAppContext) {
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

    // Escape → Cancel.
    let (_p1, plog1) = open_prompt_with_log(&shell, vcx);
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        plog1.borrow().iter().any(|e| matches!(e, NamePromptEvent::Cancel)),
        "Escape emits Cancel; got {:?}",
        plog1.borrow()
    );
    assert!(!vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()));

    // Buttons reachable + operable: Tab to Cancel, Enter → Cancel.
    let (_p2, plog2) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, &"Cancel".to_string());
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        plog2.borrow().iter().any(|e| matches!(e, NamePromptEvent::Cancel)),
        "Enter on the focused Cancel button emits Cancel; got {:?}",
        plog2.borrow()
    );
    drop(state);
}
```

> `simulate_keystrokes("enter")` is safe HERE — it drives Enter on the focused Cancel BUTTON (a `focus_stop`, not an `Input`), so there is no single-line-Input newline to panic on. Do NOT use it to drive the prompt's text field; that path uses `dispatch_action` (above).

- [ ] **Step 2: Run the full binary**

Run: `cargo test -p dat0-app --features a11y-capture --test input_nav -- --nocapture`
Expected: **all PASS** (t0 gate + 3 Task-2 + 3 Task-3 = 7 tests).

- [ ] **Step 3: `cargo fmt --all` then commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/input_nav.rs
git commit -s -m "test(sql-console): NamePrompt focus/Enter/Escape/buttons behavioral suite

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Controller gate + final review

**Files:** none (verification only).

- [ ] **Step 1: Workspace test gate (catches cross-binary drift)**

Run: `cargo test -p dat0-app --features a11y-capture --workspace --no-fail-fast`
Expected: PASS. In particular `a11y_spike` must be UNCHANGED — its scene keeps the SQL Console CLOSED and opens no NamePrompt, so the new (unconditional) `.a11y` nodes never paint there. If any binary's assertion moved, investigate before proceeding. Note: `motherduck_window` / `ai_nav` / `sql_console_nav` also mount the shell — confirm they still pass (the NamePrompt changes only add behavior; no existing prompt-open path is exercised by them, but the workspace run is the check).

- [ ] **Step 2: Clippy + fmt gate (pinned 1.97.0)**

Run: `cargo clippy -p dat0-app --features a11y-capture --all-targets -- -D warnings`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`
Expected: all clean.

- [ ] **Step 3: Prove the release build compiles (real a11y, no test leak)**

Run: `cargo build -p dat0-app` (feature OFF — default)
Run: `cargo build -p dat0-app --release`
Expected: both clean. The `on_action`/`focus_stop`/`.a11y`/subscription/focus-on-open wiring compiles with the feature off (real code); the `_for_test` accessors are absent from a default/release build.

- [ ] **Step 4: Confirm no dependency / manifest / i18n drift**

Run: `git diff main -- Cargo.toml Cargo.lock NOTICE crates/dat0-app/Cargo.toml crates/dat0-i18n/src/strings/en.json`
Expected: EMPTY. Zero new deps; zero new i18n keys (Global Constraints).

- [ ] **Step 5: Final whole-branch review (opus) + push/PR**

Dispatch a fresh-context whole-branch review (opus) checking: the editor `on_action` is correctly guarded on editor-focus (no focus hijack from other stops; popup-open Escape untouched) and reuses the `sql-run` handle (no drift); the `NamePrompt` `PressEnter` subscription and `Escape` `on_action` sit on ancestors of the focused input and emit the SAME events the OK/Cancel clicks emit (no drift); focus-on-open is real; the OK/Cancel `focus_stop`+`.a11y` carry the same-id twin with the literal `"Save"`/`"Cancel"` labels; tests are non-vacuous (Escape-exit reads a focus value that actually changed; the guard test would fail if the handler hijacked focus; Confirm/Cancel are distinguished via the captured `NamePromptEvent`, not just dismissal); the change fixes all 5 call sites without altering `on_name_prompt_event` routing; zero deps / zero new i18n keys; production wiring unconditional, only `_for_test` gated. Address Critical/Important; fold Minors. Then push `uat-input-nav` and open the PR. **Watch the post-merge main run** — the macOS grid-scroll bench is push-to-main-only and can redden main silently.

---

## Self-Review

**Spec coverage** (design §Surface 1 / §Surface 2 / §Testing):
- Editor Escape→Run exit (ancestor `on_action`, guarded, reuses `sql-run` handle) → Task 1 Step 4. ✓
- Run-from-editor already works (test-only) → Task 1 Probe 2 + Task 2. ✓
- NamePrompt focus-on-open → Task 1 Step 1 (`window.focus` in `new`). ✓
- NamePrompt Enter→Confirm (PressEnter subscription) → Task 1 Step 1. ✓
- NamePrompt Escape→Cancel (`on_action`) → Task 1 Step 2. ✓
- NamePrompt OK/Cancel `focus_stop`+`.a11y` → Task 1 Step 2. ✓
- Fixes all 5 call sites (change is in the shared component; routing unchanged) → Task 1 + Task 4 Step 5. ✓
- T0 hard gate (4 probes, STOP-clauses) → Task 1. ✓
- Behavioral suites (editor trap-exit + guard + run; prompt focus/Enter/Escape/buttons) → Tasks 2–3. ✓
- Drive via `dispatch_action`, never `simulate_keystrokes` into an Input → Steps + the Task-3 note. ✓
- Real production a11y (unconditional), only `_for_test` gated → Steps + Task 4 Step 3. ✓
- Zero deps, zero new i18n keys, `a11y_spike` zero-drift → Global Constraints + Task 4 Steps 1/4. ✓
- Owed human glance (Escape feel + prompt flow + ring contrast) → carried in the design; no code. ✓

**Placeholder scan:** no TBD/TODO; every code step shows full code or a verbatim-copy instruction; STOP-clauses give concrete fallbacks (`.id(...)` on the root; focus-on-first-render; alternate run observable). ✓

**Type consistency:** `editor_focus_handle_for_test`/`editor_focused_for_test`/`open_name_prompt_for_test`/`name_prompt_open_for_test`/`name_prompt_entity_for_test`/`input_focused_for_test`/`seed_value_for_test`/`open_prompt_with_log` — names identical across Tasks 1–3. `NamePromptEvent::{Confirm(String), Cancel}`, `SqlConsoleEvent::Run`, `gpui_component::input::{Enter{secondary}, Escape, InputEvent::PressEnter}`, `dat0_app::menu_macos::SqlRun` used consistently. `focus_stop(id,&fh,0,key)` + `.a11y(id, AccessRole::Button, label)` match the kit signature. ✓
