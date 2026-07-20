# SQL-console transient-bars keyboard-nav — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the four transient affordance bars of the SQL console (NL→SQL strip, Explain panel, error strip, history overlay) fully keyboard-operable, with focus moving into a bar on appear, re-homing across the streaming→finished button swap, an Escape cancel ladder, and focus returning to the active editor on close.

**Architecture:** A `pending_focus: Option<&'static str>` field on `SqlConsole` stashes a focus target that `render` drains via `window.focus` (mirrors the existing `pending_load`/`queue_load` idiom, because the state-setters run in `Context<Self>` with no `&mut Window`). Stable focus handles come from the existing get-or-insert `toolbar_fh` map. One consolidated `on_action::<Escape>` ladder at the console root replaces the current editor-only Escape handler. The history overlay reuses the repo's recents-list listbox pattern (one container `focus_stop` + a chained `on_key_down` for arrows + an active-index ring).

**Tech Stack:** Rust, gpui 0.2.2, gpui-component (`input::Escape` action, `InputState`), the repo's a11y kit (`FocusStopExt::focus_stop`, `A11yExt::a11y`, `focused_label`, `AccessRole`), `dat0-i18n`.

## Global Constraints

- Toolchain pinned **1.97.0**; run `cargo fmt --all` before every commit.
- Every commit: `git commit -s` + trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- **Zero new dependencies** — `Cargo.toml` / `Cargo.lock` / `NOTICE` unchanged. D-015 stays open.
- **Exactly two new i18n keys**: `sql.error.dismiss` = `"Dismiss error"`, `sql.history.close` = `"Close history"` (added in `crates/dat0-i18n/src/strings/en.json`). All other labels reuse existing keys.
- **No new `SqlConsoleEvent` variants** — reuse `StopAiStream` / `CloseExplain`; Insert / Discard / error-dismiss / history-close / row-pick stay inline handlers.
- No session-schema change; no change to SSE/streaming logic or bar content.
- Production wiring (`focus_stop`/`.a11y`, `pending_focus` drain, consolidated Escape, history listbox) is **unconditional shipped code**; only the state-injection seams and read oracles are `#[cfg(feature = "a11y-capture")]`.
- Controller gate per task: `cargo test -p dat0-app --no-fail-fast` + `cargo clippy --workspace --all-targets -- -D warnings`. Implementers may run only the focused test while iterating.
- Drive discipline: activate `focus_stop` divs with `simulate_keystrokes` (safe — divs, not single-line Inputs; the editor is multi-line). Dispatch Escape with `vcx.dispatch_action` / `cx.dispatch_action` of `gpui_component::input::Escape`. Never drive a single-line Input with `simulate_keystrokes` (the `"\n"`-panic).

## File map

- **Modify** `crates/dat0-app/src/view/sql_console.rs` — new `pending_focus` + `history_active` fields; `new()` init; `EDITOR_FOCUS` const; stash focus in `begin_nl_preview`/`finish_nl_preview`/`begin_explain`/`finish_explain`/`show_history`; `render` focus-drain + 8 handle hoists; `focus_stop`/`.a11y` on all 7 buttons; history listbox container; consolidated Escape ladder; `#[cfg(feature="a11y-capture")]` seams + oracles.
- **Modify** `crates/dat0-app/src/view/query_library.rs` — `render_history_list` gains an `active: usize` param and paints the active-row ring.
- **Modify** `crates/dat0-i18n/src/strings/en.json` — 2 new keys.
- **Create** `crates/dat0-app/tests/sql_console_transient_nav.rs` — the behavioral suite (harness helpers per-binary-copied from `tests/sql_console_nav.rs`).

## Reference: existing anchors (main `9804135`)

- Fields block ends ~`sql_console.rs:171` (`nl_preview`), `:173` (`explain`), `:162` (`history_overlay`), `:169` (`pending_load`).
- `new()` init list `:298`–`:319` (`pending_load: None,` at `:311`).
- Setters: `begin_nl_preview` `:497`, `finish_nl_preview` `:507`, `begin_explain` `:518`, `finish_explain` `:528`, `show_history` `:477`, `load_into_new_tab` `:453`.
- `toolbar_fh` `:568`. `render` opens `:598`; `pending_load` drained `:603`; toolbar handle hoists `:607`–`:613`.
- Error strip render `:909`–`:935` (`sql-err-dismiss` `:925`). History overlay `:956`–`:1002` (`sql-history-close` `:986`; `render_history_list` call `:998`). NL→SQL strip `:1246`–`:1306` (`nl2sql-stop` `:1260`, `nl2sql-insert` `:1278`, `nl2sql-discard` `:1293`). Explain panel `:1308`–`:1344` (`explain-stop` `:1319`, `explain-close` `:1332`). Existing Escape handler `:1353`–`:1366`. Test-seam block `:1370`–`:1409`.
- Shell: `StopAiStream` handler `window.rs:3938` calls `finish_nl_preview(None)` + `finish_explain(None)`; `CloseExplain` `:3952` calls `clear_explain`.
- Recents listbox pattern to mirror: `empty_state.rs:408`–`:450`.
- `a11y::FOCUS_RING = 0x3b82f6` (`a11y/mod.rs:30`). Imports already present in `sql_console.rs:31` (`A11yExt as _, AccessRole, FocusStopExt as _`).

---

### Task 1: Mechanism core + NL→SQL strip + T0 hard gate

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs`
- Test: `crates/dat0-app/tests/sql_console_transient_nav.rs` (create)

**Interfaces:**
- Produces: `pending_focus: Option<&'static str>` + `history_active: usize` fields; `const EDITOR_FOCUS: &str`; the `render` focus-drain; the 8 hoisted handles; the full consolidated Escape ladder (all 5 rungs — later tasks rely on it existing); operable `nl2sql-stop`/`nl2sql-insert`/`nl2sql-discard`; test seams `begin_nl_preview_for_test`, `push_nl_delta_for_test`, `finish_nl_preview_for_test`, `nl_preview_open_for_test`, plus reused `editor_focused_for_test`.

- [ ] **Step 1: Add the two fields.** In `sql_console.rs`, immediately after the `pending_load` field declaration (`:169`), add:

```rust
    /// Focus target queued for the next render (transient-bars nav, carve-out #7).
    /// Focus is a `&mut Window` op, but the setters that decide it (`begin_*`/
    /// `finish_*`/`show_history` + the Escape ladder) run in `Context<Self>` with
    /// no window in scope. Stash the target's `&'static str` id (a button id
    /// resolved via `toolbar_fh`, or [`EDITOR_FOCUS`] for the active editor) and
    /// let [`render`](Self::render) drain it — mirrors `pending_load`.
    pub(crate) pending_focus: Option<&'static str>,
    /// Active row index for the query-history overlay listbox (carve-out #7).
    /// Reset to 0 in [`show_history`](Self::show_history); clamped to the entry
    /// count at render. Mirrors `WorkspaceShell.recents_active`.
    pub(crate) history_active: usize,
```

- [ ] **Step 2: Add the sentinel const.** Just above `pub struct SqlConsole` (`:123`), add:

```rust
/// `pending_focus` sentinel meaning "the active tab's editor". Not a real
/// `toolbar_fh` id — `render` resolves it to the editor's `FocusHandle`.
const EDITOR_FOCUS: &str = "__editor__";
```

- [ ] **Step 3: Init the fields in `new()`.** In the struct literal, after `pending_load: None,` (`:311`), add:

```rust
            pending_focus: None,
            history_active: 0,
```

- [ ] **Step 4: Stash focus in the NL setters.** Replace `begin_nl_preview` and `finish_nl_preview` (`:497`–`:512`) with:

```rust
    pub(crate) fn begin_nl_preview(&mut self, prompt: String, cx: &mut Context<Self>) {
        self.nl_preview = Some(NlPreview::new(prompt));
        self.pending_focus = Some("nl2sql-stop"); // streaming → focus Stop
        cx.notify();
    }
    pub(crate) fn push_nl_delta(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.nl_preview {
            p.push(text);
            cx.notify();
        }
    }
    pub(crate) fn finish_nl_preview(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.nl_preview {
            p.finish(error);
            self.pending_focus = Some("nl2sql-insert"); // re-home across Stop→Insert swap
            cx.notify();
        }
    }
```

- [ ] **Step 5: Drain `pending_focus` in `render`.** Immediately after the `pending_load` drain block (`:603`–`:605`, the `if let Some(sql) = self.pending_load.take()`), add:

```rust
        // Drain any focus target queued by a transient-bar setter/handler
        // (carve-out #7). Done after the `pending_load` load so a freshly-opened
        // tab is already active when `EDITOR_FOCUS` resolves.
        if let Some(id) = self.pending_focus.take() {
            let fh = if id == EDITOR_FOCUS {
                self.tabs[self.active].input.read(cx).focus_handle(cx)
            } else {
                self.toolbar_fh(id, cx)
            };
            window.focus(&fh);
        }
```

- [ ] **Step 6: Hoist the 8 transient-bar handles.** After the `save_as_table_fh` hoist (`:613`), add:

```rust
        let nl_stop_fh = self.toolbar_fh("nl2sql-stop", cx);
        let nl_insert_fh = self.toolbar_fh("nl2sql-insert", cx);
        let nl_discard_fh = self.toolbar_fh("nl2sql-discard", cx);
        let explain_stop_fh = self.toolbar_fh("explain-stop", cx);
        let explain_close_fh = self.toolbar_fh("explain-close", cx);
        let err_dismiss_fh = self.toolbar_fh("sql-err-dismiss", cx);
        let history_close_fh = self.toolbar_fh("sql-history-close", cx);
        let history_list_fh = self.toolbar_fh("sql-history-list", cx);
```

(`nl_*` are used this task; the others are consumed by Tasks 2–4 but hoist them all now so later tasks touch only their own render block. `clippy` would warn on unused — suppress by prefixing the five not-yet-used ones with `_` for now: `_explain_stop_fh`, `_explain_close_fh`, `_err_dismiss_fh`, `_history_close_fh`, `_history_list_fh`; Tasks 2–4 drop the underscore as they wire each.)

- [ ] **Step 7: Make the NL→SQL buttons operable.** Replace the `if p.streaming { … } else { … }` block inside the NL strip closure (`:1257`–`:1304`) with:

```rust
                if p.streaming {
                    let key = cx.listener(|_c, _ev: &gpui::KeyDownEvent, _w, cx| {
                        cx.emit(SqlConsoleEvent::StopAiStream);
                    });
                    strip = strip.child(
                        div()
                            .px_2()
                            .py_1()
                            .border_1()
                            .cursor_pointer()
                            .child(SharedString::from(dat0_i18n::t("sql.ai.stop")))
                            .focus_stop("nl2sql-stop", &nl_stop_fh, 0, key)
                            .a11y("nl2sql-stop", AccessRole::Button, dat0_i18n::t("sql.ai.stop"))
                            .on_click(cx.listener(|_c, _ev, _w, cx| {
                                cx.emit(SqlConsoleEvent::StopAiStream);
                            })),
                    );
                } else {
                    let insert_key = cx.listener(|c, _ev: &gpui::KeyDownEvent, window, cx| {
                        if let Some(p) = c.nl_preview.take() {
                            c.load_into_new_tab(p.sql, window, cx);
                        }
                        c.pending_focus = Some(EDITOR_FOCUS);
                        cx.notify();
                    });
                    let discard_key = cx.listener(|c, _ev: &gpui::KeyDownEvent, _w, cx| {
                        c.nl_preview = None;
                        c.pending_focus = Some(EDITOR_FOCUS);
                        cx.notify();
                    });
                    strip = strip.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .cursor_pointer()
                                    .child(SharedString::from(dat0_i18n::t("sql.nl2sql.insert")))
                                    .focus_stop("nl2sql-insert", &nl_insert_fh, 0, insert_key)
                                    .a11y(
                                        "nl2sql-insert",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.nl2sql.insert"),
                                    )
                                    .on_click(cx.listener(|c, _ev, window, cx| {
                                        if let Some(p) = c.nl_preview.take() {
                                            c.load_into_new_tab(p.sql, window, cx);
                                        }
                                        c.pending_focus = Some(EDITOR_FOCUS);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .cursor_pointer()
                                    .child(SharedString::from(dat0_i18n::t("sql.nl2sql.discard")))
                                    .focus_stop("nl2sql-discard", &nl_discard_fh, 1, discard_key)
                                    .a11y(
                                        "nl2sql-discard",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.nl2sql.discard"),
                                    )
                                    .on_click(cx.listener(|c, _ev, _w, cx| {
                                        c.nl_preview = None;
                                        c.pending_focus = Some(EDITOR_FOCUS);
                                        cx.notify();
                                    })),
                            ),
                    );
                }
```

- [ ] **Step 8: Replace the Escape handler with the full consolidated ladder.** Replace the entire `.on_action(cx.listener(|this, _ev: &gpui_component::input::Escape, window, cx| { … }))` block (`:1353`–`:1366`) with:

```rust
            // Consolidated Escape ladder (transient-bars nav, carve-out #7).
            // First matching rung wins; gpui bubbles the action to this one
            // ancestor handler. Rung 5 preserves the carve-out #6 editor
            // trap-exit (Escape leaves the code editor onto Run).
            .on_action(
                cx.listener(|this, _ev: &gpui_component::input::Escape, window, cx| {
                    // 1. History overlay open → close, return to editor.
                    if this.history_overlay.is_some() {
                        this.history_overlay = None;
                        this.pending_focus = Some(EDITOR_FOCUS);
                        cx.notify();
                        return;
                    }
                    // 2. NL→SQL strip → stop if streaming, else discard.
                    if let Some(streaming) = this.nl_preview.as_ref().map(|p| p.streaming) {
                        if streaming {
                            cx.emit(SqlConsoleEvent::StopAiStream);
                        } else {
                            this.nl_preview = None;
                            this.pending_focus = Some(EDITOR_FOCUS);
                            cx.notify();
                        }
                        return;
                    }
                    // 3. Explain panel → stop if streaming, else close.
                    if let Some(streaming) = this.explain.as_ref().map(|e| e.streaming) {
                        if streaming {
                            cx.emit(SqlConsoleEvent::StopAiStream);
                        } else {
                            this.pending_focus = Some(EDITOR_FOCUS);
                            cx.emit(SqlConsoleEvent::CloseExplain);
                        }
                        return;
                    }
                    // 4. Error strip → dismiss, keep current focus.
                    if matches!(this.region, ResultRegion::Error(_)) {
                        this.region = ResultRegion::Empty;
                        cx.notify();
                        return;
                    }
                    // 5. Editor focused → leave onto Run (carve-out #6 trap-exit).
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
                }),
            )
```

- [ ] **Step 9: Add the Task-1 test seams.** In the `#[cfg(feature = "a11y-capture")] impl SqlConsole` block (after `:1408`, before its closing `}`), add:

```rust
    /// Inject a streaming NL→SQL preview (bypasses the real SSE flow).
    pub fn begin_nl_preview_for_test(&mut self, prompt: String, cx: &mut Context<Self>) {
        self.begin_nl_preview(prompt, cx);
    }
    /// Append a generated-SQL delta to the injected preview.
    pub fn push_nl_delta_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        self.push_nl_delta(text, cx);
    }
    /// Finish the injected preview (flips streaming → Insert/Discard).
    pub fn finish_nl_preview_for_test(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.finish_nl_preview(error, cx);
    }
    /// Whether the NL→SQL strip is currently open.
    pub fn nl_preview_open_for_test(&self) -> bool {
        self.nl_preview.is_some()
    }
```

- [ ] **Step 10: Create the test file with the harness + T0 gate.** Create `crates/dat0-app/tests/sql_console_transient_nav.rs`. Copy the harness scaffold from `tests/sql_console_nav.rs` lines 1–200 verbatim (the `mod support;`, imports, `BUDGET`, `set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`, `AsyncHarness`/`enter_async_harness`, `focus_shell_neutrally`, `tab_until`, `tab_labels`, `open_console_with_log`), then append this gate (a helper to open a console + inject a finished preview is inlined per-test):

```rust
/// T0 HARD GATE — de-risks the three empirical unknowns before any breadth:
///   Probe 1: injecting a streaming NL preview moves focus to `nl2sql-stop`
///            (focus-on-appear + the harness can observe the transient strip).
///   Probe 2: finishing the preview RE-HOMES focus to `nl2sql-insert` across the
///            Stop→Insert button swap (focus is not dropped to nowhere).
///   Probe 3: Escape while the finished strip is open discards it and returns
///            focus to the editor (the Escape ladder's NL rung routes).
#[gpui::test]
#[serial]
fn t0_transient_bars_gate(cx: &mut TestAppContext) {
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

    let stop = dat0_i18n::t("sql.ai.stop");
    let insert = dat0_i18n::t("sql.nl2sql.insert");

    // Probe 1: streaming preview → focus lands on Stop.
    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.begin_nl_preview_for_test("top users".into(), cx))
    });
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(stop.clone()),
        "STOP-1: focus must move to nl2sql-stop when the streaming strip appears"
    );

    // Probe 2: finish → focus re-homes to Insert across the button swap.
    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.finish_nl_preview_for_test(None, cx))
    });
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(insert.clone()),
        "STOP-2: focus must re-home to nl2sql-insert when the stream finishes"
    );

    // Probe 3: Escape discards the finished strip and returns to the editor.
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.nl_preview_open_for_test()),
        "STOP-3: Escape must discard the finished NL strip"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "STOP-3: focus must return to the editor after discard"
    );
}
```

Note: `A11ySnapshot` is the `support` helper already imported by the copied scaffold; `vcx.dispatch_action(gpui_component::input::Escape)` dispatches to the focused element's ancestors. Add `use dat0_app::view::sql_console::{SqlConsole, SqlConsoleEvent};` (already in the copied imports) and ensure `gpui_component` is available (it is a dev-dep of `dat0-app`; `tests/input_nav.rs` uses `gpui_component::input::{Enter, Escape}` — copy that `use` if the linker complains).

- [ ] **Step 11: Run the T0 gate — expect it to compile-fail first, then pass.**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav t0_transient_bars_gate -- --nocapture`
Expected on first run before Steps 1–9 are in place: compile error (missing seams) — that is why Steps 1–9 precede this. With Steps 1–9 applied: **PASS** all three probes. **STOP and escalate if any probe fails** — a failure means the `pending_focus` re-home or the Escape ladder does not behave as designed, and the rest of the plan rests on it.

- [ ] **Step 12: Add the NL behavioral tests.** Append to the same file:

```rust
#[gpui::test]
#[serial]
fn nl2sql_stop_emits_stopaistream(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.begin_nl_preview_for_test("q".into(), cx))
    });
    vcx.run_until_parked(); // focus is on Stop (T0 Probe 1)
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::StopAiStream)),
        "Enter on the focused Stop must emit StopAiStream; got {:?}",
        log.borrow()
    );
}

#[gpui::test]
#[serial]
fn nl2sql_insert_opens_tab_and_returns_focus(cx: &mut TestAppContext) {
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

    let before = console.read_with(&vcx.cx, |c, _| c.tab_count_for_test());
    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.begin_nl_preview_for_test("q".into(), cx);
            c.push_nl_delta_for_test("SELECT 1", cx);
            c.finish_nl_preview_for_test(None, cx);
        })
    });
    vcx.run_until_parked(); // focus is on Insert (T0 Probe 2)
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()),
        before + 1,
        "Insert must open a new tab with the generated SQL"
    );
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.nl_preview_open_for_test()),
        "Insert must consume the preview"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "Insert must return focus to the (new tab's) editor"
    );
}

#[gpui::test]
#[serial]
fn nl2sql_discard_returns_focus_to_editor(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.begin_nl_preview_for_test("q".into(), cx);
            c.finish_nl_preview_for_test(None, cx);
        })
    });
    vcx.run_until_parked(); // focus on Insert; Discard is the next tab stop
    support::press_tab(vcx); // Insert (index 0) → Discard (index 1)
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.nl2sql.discard")),
        "Tab must reach Discard after Insert"
    );
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.nl_preview_open_for_test()),
        "Discard must close the strip"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "Discard must return focus to the editor"
    );
}
```

- [ ] **Step 13: Run the full Task-1 suite.**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav`
Expected: 4 tests PASS (`t0_transient_bars_gate`, `nl2sql_stop_emits_stopaistream`, `nl2sql_insert_opens_tab_and_returns_focus`, `nl2sql_discard_returns_focus_to_editor`).

- [ ] **Step 14: Controller gate.**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p dat0-app --no-fail-fast`
Expected: clean (clippy: no unused-variable warnings — the five not-yet-wired handles are `_`-prefixed). Confirm `tests/input_nav.rs` still passes (the editor Escape→Run trap-exit is preserved as rung 5).

- [ ] **Step 15: Commit.**

```bash
git add crates/dat0-app/src/view/sql_console.rs crates/dat0-app/tests/sql_console_transient_nav.rs
git commit -s -m "feat(sql-console): transient-bars nav core + NL→SQL strip + T0 gate

pending_focus render-drain, consolidated Escape ladder, focus-managed
NL→SQL Stop/Insert/Discard. T0 gate proves focus-on-appear, the Stop→Insert
re-home, and the Escape NL rung.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Explain panel

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs`
- Test: `crates/dat0-app/tests/sql_console_transient_nav.rs`

**Interfaces:**
- Consumes: `EDITOR_FOCUS`, `pending_focus`, hoisted `explain_stop_fh`/`explain_close_fh` (drop their `_` prefix), the Escape ladder rung 3 (already present from Task 1).
- Produces: operable `explain-stop`/`explain-close`; seams `begin_explain_for_test`, `finish_explain_for_test`, `explain_open_for_test`.

- [ ] **Step 1: Stash focus in the Explain setters.** Replace `begin_explain` (`:518`–`:521`) and `finish_explain` (`:528`–`:533`) with:

```rust
    pub(crate) fn begin_explain(&mut self, sql: String, cx: &mut Context<Self>) {
        self.explain = Some(ExplainView::new(sql));
        self.pending_focus = Some("explain-stop"); // streaming → focus Stop
        cx.notify();
    }
```

```rust
    pub(crate) fn finish_explain(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        if let Some(e) = &mut self.explain {
            e.finish(error);
            self.pending_focus = Some("explain-close"); // re-home across Stop→Close
            cx.notify();
        }
    }
```

- [ ] **Step 2: Un-underscore the two handles.** In `render`, change `_explain_stop_fh`/`_explain_close_fh` (Task 1 Step 6) to `explain_stop_fh`/`explain_close_fh`.

- [ ] **Step 3: Make the Explain buttons operable.** Replace the `if e.streaming { … } else { … }` block inside the Explain panel closure (`:1316`–`:1342`) with:

```rust
                if e.streaming {
                    let key = cx.listener(|_c, _ev: &gpui::KeyDownEvent, _w, cx| {
                        cx.emit(SqlConsoleEvent::StopAiStream);
                    });
                    panel = panel.child(
                        div()
                            .px_2()
                            .py_1()
                            .border_1()
                            .cursor_pointer()
                            .child(SharedString::from(dat0_i18n::t("sql.ai.stop")))
                            .focus_stop("explain-stop", &explain_stop_fh, 0, key)
                            .a11y("explain-stop", AccessRole::Button, dat0_i18n::t("sql.ai.stop"))
                            .on_click(cx.listener(|_c, _ev, _w, cx| {
                                cx.emit(SqlConsoleEvent::StopAiStream);
                            })),
                    );
                } else {
                    let key = cx.listener(|c, _ev: &gpui::KeyDownEvent, _w, cx| {
                        c.pending_focus = Some(EDITOR_FOCUS);
                        cx.emit(SqlConsoleEvent::CloseExplain);
                    });
                    panel = panel.child(
                        div()
                            .px_2()
                            .py_1()
                            .border_1()
                            .cursor_pointer()
                            .child(SharedString::from(dat0_i18n::t("sql.explain.close")))
                            .focus_stop("explain-close", &explain_close_fh, 0, key)
                            .a11y(
                                "explain-close",
                                AccessRole::Button,
                                dat0_i18n::t("sql.explain.close"),
                            )
                            .on_click(cx.listener(|c, _ev, _w, cx| {
                                c.pending_focus = Some(EDITOR_FOCUS);
                                cx.emit(SqlConsoleEvent::CloseExplain);
                            })),
                    );
                }
```

- [ ] **Step 4: Add the Explain seams.** In the `#[cfg(feature = "a11y-capture")]` block, add:

```rust
    /// Inject a streaming Explain panel (bypasses the real SSE flow).
    pub fn begin_explain_for_test(&mut self, sql: String, cx: &mut Context<Self>) {
        self.begin_explain(sql, cx);
    }
    /// Finish the injected Explain (flips streaming → Close).
    pub fn finish_explain_for_test(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.finish_explain(error, cx);
    }
    /// Whether the Explain panel is currently open.
    pub fn explain_open_for_test(&self) -> bool {
        self.explain.is_some()
    }
```

- [ ] **Step 5: Add the Explain tests.** Append to the test file:

```rust
#[gpui::test]
#[serial]
fn explain_focuses_stop_then_rehomes_close(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.begin_explain_for_test("SELECT 1".into(), cx))
    });
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.ai.stop")),
        "streaming Explain must focus Stop"
    );
    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.finish_explain_for_test(None, cx))
    });
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.explain.close")),
        "finished Explain must re-home focus to Close"
    );
}

#[gpui::test]
#[serial]
fn explain_close_emits_and_returns_focus(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.begin_explain_for_test("SELECT 1".into(), cx);
            c.finish_explain_for_test(None, cx);
        })
    });
    vcx.run_until_parked(); // focus on Close
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::CloseExplain)),
        "Enter on Close must emit CloseExplain; got {:?}",
        log.borrow()
    );
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.explain_open_for_test()),
        "the shell's CloseExplain handler must clear the panel"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "Close must return focus to the editor"
    );
}

#[gpui::test]
#[serial]
fn explain_escape_streaming_stops_finished_closes(cx: &mut TestAppContext) {
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

    // Streaming: Escape emits StopAiStream.
    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.begin_explain_for_test("SELECT 1".into(), cx))
    });
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::StopAiStream)),
        "Escape while Explain streams must emit StopAiStream"
    );
}
```

- [ ] **Step 6: Run + gate.**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav`
Expected: 7 tests PASS. Then: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p dat0-app --no-fail-fast`.

- [ ] **Step 7: Commit.**

```bash
git add crates/dat0-app/src/view/sql_console.rs crates/dat0-app/tests/sql_console_transient_nav.rs
git commit -s -m "feat(sql-console): keyboard-operable Explain panel (Stop/Close)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Error strip

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs`, `crates/dat0-i18n/src/strings/en.json`
- Test: `crates/dat0-app/tests/sql_console_transient_nav.rs`

**Interfaces:**
- Consumes: hoisted `err_dismiss_fh` (drop `_`), Escape ladder rung 4 + rung 5 (present from Task 1).
- Produces: operable `sql-err-dismiss` (no auto-focus); seam `set_error_region_for_test`; i18n `sql.error.dismiss`.

- [ ] **Step 1: Add the i18n key.** In `crates/dat0-i18n/src/strings/en.json`, after the `"sql.cancelled": "Cancelled",` line (line 72), add:

```json
  "sql.error.dismiss": "Dismiss error",
```

- [ ] **Step 2: Un-underscore the handle.** In `render`, change `_err_dismiss_fh` → `err_dismiss_fh`.

- [ ] **Step 3: Make the dismiss ✕ operable.** In the `ResultRegion::Error` arm, replace the `sql-err-dismiss` child (`:923`–`:933`) with:

```rust
                    .child({
                        let key = cx.listener(|this, _ev: &gpui::KeyDownEvent, _w, cx| {
                            this.region = ResultRegion::Empty;
                            cx.notify();
                        });
                        div()
                            .cursor_pointer()
                            .px_1()
                            .child(SharedString::from("✕"))
                            .focus_stop("sql-err-dismiss", &err_dismiss_fh, 0, key)
                            .a11y(
                                "sql-err-dismiss",
                                AccessRole::Button,
                                dat0_i18n::t("sql.error.dismiss"),
                            )
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.region = ResultRegion::Empty;
                                cx.notify();
                            }))
                    })
```

Note: no focus-on-appear — the error strip must NOT steal focus from the editor (design decision). The `.a11y_label(Alert, msg)` on the parent strip (`:921`) is unchanged.

- [ ] **Step 4: Add the error seam.** In the `#[cfg(feature = "a11y-capture")]` block:

```rust
    /// Put the result region into the DuckDB-error state (bypasses a real run).
    pub fn set_error_region_for_test(&mut self, msg: String, cx: &mut Context<Self>) {
        self.region = ResultRegion::Error(msg);
        cx.notify();
    }
    /// Whether the result region is currently showing an error.
    pub fn error_region_for_test(&self) -> bool {
        matches!(self.region, ResultRegion::Error(_))
    }
```

(`ResultRegion` is in scope in this module; the seam is inside `impl SqlConsole`.)

- [ ] **Step 5: Add the error tests.** Append:

```rust
#[gpui::test]
#[serial]
fn error_strip_does_not_steal_focus(cx: &mut TestAppContext) {
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

    // Focus the editor, then raise an error — focus must NOT move to the ✕.
    let editor_fh = console.read_with(&vcx.cx, |c, cx| c.editor_focus_handle_for_test(cx));
    vcx.update(|window, _app| window.focus(&editor_fh));
    vcx.run_until_parked();
    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.set_error_region_for_test("boom".into(), cx))
    });
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "the error strip must not steal focus from the editor"
    );
}

#[gpui::test]
#[serial]
fn error_dismiss_reachable_and_dismisses(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.set_error_region_for_test("boom".into(), cx))
    });
    vcx.run_until_parked();
    // Tab to the dismiss ✕ (named "Dismiss error"), then Enter dismisses.
    focus_shell_neutrally(vcx);
    tab_until(vcx, &dat0_i18n::t("sql.error.dismiss"));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.error_region_for_test()),
        "Enter on the ✕ must dismiss the error"
    );
}

#[gpui::test]
#[serial]
fn escape_dismisses_error_then_run_trap_exit(cx: &mut TestAppContext) {
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

    // Editor focused + error showing → first Escape dismisses the error (rung 4),
    // keeps editor focus; second Escape does the Run trap-exit (rung 5).
    let editor_fh = console.read_with(&vcx.cx, |c, cx| c.editor_focus_handle_for_test(cx));
    vcx.update(|window, _app| window.focus(&editor_fh));
    vcx.update(|_w, app| {
        console.update(app, |c, cx| c.set_error_region_for_test("boom".into(), cx))
    });
    vcx.run_until_parked();

    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.error_region_for_test()),
        "first Escape must dismiss the error"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "first Escape must keep focus in the editor"
    );

    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.run")),
        "second Escape must do the editor→Run trap-exit"
    );
}
```

- [ ] **Step 6: Run + gate.**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav`
Expected: 10 tests PASS. Then fmt + clippy + `cargo test -p dat0-app --no-fail-fast`.

- [ ] **Step 7: Commit.**

```bash
git add crates/dat0-app/src/view/sql_console.rs crates/dat0-i18n/src/strings/en.json crates/dat0-app/tests/sql_console_transient_nav.rs
git commit -s -m "feat(sql-console): keyboard-operable error-strip dismiss + Escape rung

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: History overlay listbox

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs`, `crates/dat0-app/src/view/query_library.rs`, `crates/dat0-i18n/src/strings/en.json`
- Test: `crates/dat0-app/tests/sql_console_transient_nav.rs`

**Interfaces:**
- Consumes: `history_active`, `EDITOR_FOCUS`, hoisted `history_close_fh`/`history_list_fh` (drop `_`), Escape ladder rung 1.
- Produces: `render_history_list(entries, active, on_pick)` (signature change); operable history listbox + close; seams `show_fake_history_for_test`, `history_active_for_test`, `history_open_for_test`.

- [ ] **Step 1: Add the i18n key.** In `en.json`, after `"sql.history": "History",` (line 82), add:

```json
  "sql.history.close": "Close history",
```

- [ ] **Step 2: Reset the active index in `show_history`.** Replace `show_history` (`:477`–`:484`) with:

```rust
    pub fn show_history(
        &mut self,
        entries: Vec<crate::session::queries::HistoryEntry>,
        cx: &mut Context<Self>,
    ) {
        self.history_overlay = Some(entries);
        self.history_active = 0;
        self.pending_focus = Some("sql-history-list"); // focus the list on open
        cx.notify();
    }
```

- [ ] **Step 3: Add the `active` param + ring to `render_history_list`.** In `query_library.rs`, replace the whole `render_history_list` fn (`:29`–`:57`) with:

```rust
/// Render a history list (newest first). `active` is the index (in DISPLAY /
/// newest-first order) of the keyboard-selected row; it gets an active-row ring.
/// `on_pick` is called with the chosen SQL plus the live `Window`/`App` from the
/// click, so the caller can load it into a new tab (which needs a `&mut Window`).
pub fn render_history_list(
    entries: &[HistoryEntry],
    active: usize,
    on_pick: impl Fn(String, &mut Window, &mut App) + 'static + Clone,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .children(entries.iter().rev().enumerate().map(move |(i, e)| {
            let sql = e.sql.clone();
            let on_pick = on_pick.clone();
            let preview: SharedString = first_line(&e.sql, 80).into();
            let meta: SharedString =
                format!("{} · {} ms", if e.ok { "ok" } else { "err" }, e.elapsed_ms).into();
            let mut row = div()
                .id(("hist-row", i))
                .flex()
                .flex_row()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1()
                .cursor_pointer()
                .child(preview)
                .child(meta)
                .on_click(move |_ev, window, cx| on_pick(sql.clone(), window, cx));
            if i == active {
                row = row
                    .border_1()
                    .border_color(gpui::rgb(crate::a11y::FOCUS_RING));
            }
            row
        }))
}
```

- [ ] **Step 4: Rebuild the history overlay as a listbox.** In `sql_console.rs`, un-underscore `history_close_fh`/`history_list_fh`, then replace the whole `history_overlay` builder (`:956`–`:1002`, the `let history_overlay: Option<gpui::AnyElement> = self.history_overlay.as_ref().map(|entries| { … });`) with:

```rust
        let history_overlay: Option<gpui::AnyElement> =
            self.history_overlay.as_ref().map(|entries| {
                let len = entries.len();
                let active = self.history_active.min(len.saturating_sub(1));
                // DISPLAY order is newest-first (the list renders `.iter().rev()`),
                // so the active row's SQL is `reversed[active]`.
                let picks: Vec<String> = entries.iter().rev().map(|e| e.sql.clone()).collect();

                // Row click path (mouse) — load + close + return focus to editor.
                let this = cx.entity();
                let on_pick = move |sql: String, window: &mut Window, app: &mut gpui::App| {
                    this.update(app, |c, cx| {
                        c.history_overlay = None;
                        c.pending_focus = Some(EDITOR_FOCUS);
                        c.load_into_new_tab(sql, window, cx);
                    });
                };

                // Enter/Space (keyboard) — SAME load path via the active index.
                let picks_for_enter = picks.clone();
                let activate = cx.listener(move |c, _ev: &gpui::KeyDownEvent, window, cx| {
                    if let Some(sql) = picks_for_enter.get(active).cloned() {
                        c.history_overlay = None;
                        c.pending_focus = Some(EDITOR_FOCUS);
                        c.load_into_new_tab(sql, window, cx);
                    }
                });

                // ↑/↓ move the active index (second on_key_down, chained after
                // focus_stop — gpui fires both). `len` captured for the down-clamp.
                let arrows = cx.listener(move |c, ev: &gpui::KeyDownEvent, _window, cx| {
                    match ev.keystroke.key.as_str() {
                        "down" => {
                            c.history_active = (c.history_active + 1).min(len.saturating_sub(1))
                        }
                        "up" => c.history_active = c.history_active.saturating_sub(1),
                        _ => return,
                    }
                    cx.notify();
                });

                // Close ✕ (Enter/Space + click) — clear + return focus to editor.
                let close_entity = cx.entity();
                let close_key = move |_ev: &gpui::KeyDownEvent, _window: &mut Window, app: &mut gpui::App| {
                    close_entity.update(app, |c, cx| {
                        c.history_overlay = None;
                        c.pending_focus = Some(EDITOR_FOCUS);
                        cx.notify();
                    });
                };
                let close_click = cx.entity();

                div()
                    .absolute()
                    .top_8()
                    .right_2()
                    .w(gpui::px(420.))
                    .max_h(gpui::px(320.))
                    .overflow_hidden()
                    .border_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .px_2()
                            .py_1()
                            .border_b_1()
                            .child(SharedString::from(dat0_i18n::t("sql.history")))
                            .child(
                                div()
                                    .cursor_pointer()
                                    .px_1()
                                    .child(SharedString::from("✕"))
                                    .focus_stop("sql-history-close", &history_close_fh, 1, close_key)
                                    .a11y(
                                        "sql-history-close",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.history.close"),
                                    )
                                    .on_click(move |_ev, _window, cx| {
                                        close_click.update(cx, |c, cx| {
                                            c.history_overlay = None;
                                            c.pending_focus = Some(EDITOR_FOCUS);
                                            cx.notify();
                                        });
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .focus_stop("sql-history-list", &history_list_fh, 0, activate)
                            .on_key_down(arrows)
                            .a11y(
                                "sql-history-list",
                                AccessRole::Button,
                                dat0_i18n::t("sql.history"),
                            )
                            .child(crate::view::query_library::render_history_list(
                                entries, active, on_pick,
                            )),
                    )
                    .into_any_element()
            });
```

- [ ] **Step 5: Add the history seams.** In the `#[cfg(feature = "a11y-capture")]` block:

```rust
    /// Open the history overlay with fake entries (bypasses the session store).
    pub fn show_fake_history_for_test(&mut self, sqls: Vec<String>, cx: &mut Context<Self>) {
        let entries = sqls
            .into_iter()
            .map(|s| crate::session::queries::HistoryEntry {
                sql: s,
                ran_at: 0,
                ok: true,
                elapsed_ms: 0,
            })
            .collect();
        self.show_history(entries, cx);
    }
    /// The history listbox active row index.
    pub fn history_active_for_test(&self) -> usize {
        self.history_active
    }
    /// Whether the history overlay is open.
    pub fn history_open_for_test(&self) -> bool {
        self.history_overlay.is_some()
    }
    /// The active tab's SQL buffer (to assert which history row loaded).
    pub fn active_sql_for_test(&self, cx: &gpui::App) -> String {
        self.active_sql_and_cursor(cx).0
    }
```

- [ ] **Step 6: Add the history tests.** Append:

```rust
#[gpui::test]
#[serial]
fn history_opens_focuses_list_at_row0(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.show_fake_history_for_test(vec!["a".into(), "b".into(), "c".into()], cx)
        })
    });
    vcx.run_until_parked();
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some(dat0_i18n::t("sql.history")),
        "opening history must focus the list container"
    );
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.history_active_for_test()),
        0,
        "history opens with row 0 active"
    );
}

#[gpui::test]
#[serial]
fn history_arrows_move_active_clamped(cx: &mut TestAppContext) {
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

    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.show_fake_history_for_test(vec!["a".into(), "b".into(), "c".into()], cx)
        })
    });
    vcx.run_until_parked();
    vcx.simulate_keystrokes("down down down"); // clamp at len-1 == 2
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.history_active_for_test()),
        2,
        "down clamps at the last row"
    );
    vcx.simulate_keystrokes("up up up"); // clamp at 0
    vcx.run_until_parked();
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.history_active_for_test()),
        0,
        "up clamps at the first row"
    );
}

#[gpui::test]
#[serial]
fn history_enter_picks_active_into_new_tab(cx: &mut TestAppContext) {
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

    // Entries ["a","b","c"] render newest-first as ["c","b","a"]; active 1 == "b".
    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.show_fake_history_for_test(vec!["a".into(), "b".into(), "c".into()], cx)
        })
    });
    vcx.run_until_parked();
    let before = console.read_with(&vcx.cx, |c, _| c.tab_count_for_test());
    vcx.simulate_keystrokes("down"); // active 0 → 1 ("b")
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.history_open_for_test()),
        "Enter must close the overlay"
    );
    assert_eq!(
        console.read_with(&vcx.cx, |c, _| c.tab_count_for_test()),
        before + 1,
        "Enter must open a new tab for the picked query"
    );
    assert_eq!(
        console.read_with(&vcx.cx, |c, cx| c.active_sql_for_test(cx)),
        "b",
        "the picked row (display index 1) is the SQL loaded"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "pick must land focus on the new tab's editor"
    );
}

#[gpui::test]
#[serial]
fn history_close_and_escape_return_focus(cx: &mut TestAppContext) {
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

    // Escape closes the overlay and returns focus to the editor.
    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.show_fake_history_for_test(vec!["a".into()], cx)
        })
    });
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.history_open_for_test()),
        "Escape must close the history overlay"
    );
    assert!(
        vcx.update(|window, app| console.read(app).editor_focused_for_test(window, app)),
        "Escape must return focus to the editor"
    );
}
```

- [ ] **Step 7: Run + gate.**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav`
Expected: 14 tests PASS. Then fmt + clippy + `cargo test -p dat0-app --no-fail-fast`. Verify `sql_console_integration.rs` / any other caller of `render_history_list` still compiles (grep confirms the sole call site is the one edited in Step 4).

- [ ] **Step 8: Commit.**

```bash
git add crates/dat0-app/src/view/sql_console.rs crates/dat0-app/src/view/query_library.rs crates/dat0-i18n/src/strings/en.json crates/dat0-app/tests/sql_console_transient_nav.rs
git commit -s -m "feat(sql-console): full keyboard listbox nav for the history overlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Escape priority + non-vacuity regression sweep

**Files:**
- Test: `crates/dat0-app/tests/sql_console_transient_nav.rs`

**Interfaces:**
- Consumes: all seams from Tasks 1–4 + `tab_labels`.

- [ ] **Step 1: Add the cross-cutting tests.** Append:

```rust
#[gpui::test]
#[serial]
fn escape_history_beats_error(cx: &mut TestAppContext) {
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

    // Both an error and the history overlay are present. Escape must hit rung 1
    // (history) first, leaving the error intact.
    vcx.update(|_w, app| {
        console.update(app, |c, cx| {
            c.set_error_region_for_test("boom".into(), cx);
            c.show_fake_history_for_test(vec!["a".into()], cx);
        })
    });
    vcx.run_until_parked();
    vcx.dispatch_action(gpui_component::input::Escape);
    vcx.run_until_parked();
    assert!(
        !console.read_with(&vcx.cx, |c, _| c.history_open_for_test()),
        "Escape must close history first (rung 1)"
    );
    assert!(
        console.read_with(&vcx.cx, |c, _| c.error_region_for_test()),
        "the error must survive the history-closing Escape"
    );
}

#[gpui::test]
#[serial]
fn no_transient_button_is_a_tab_stop_when_closed(cx: &mut TestAppContext) {
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

    // Non-vacuity: with every transient bar closed, none of the seven labels is
    // reachable by Tab (they only exist while their bar is mounted).
    focus_shell_neutrally(vcx);
    let seen = tab_labels(vcx, 60);
    for label_key in [
        "sql.ai.stop",
        "sql.nl2sql.insert",
        "sql.nl2sql.discard",
        "sql.explain.close",
        "sql.error.dismiss",
        "sql.history.close",
    ] {
        let label = dat0_i18n::t(label_key);
        assert!(
            !seen.contains(&label),
            "{label_key} must not be a Tab stop when its bar is closed; visited {seen:?}"
        );
    }
}
```

Note on the non-vacuity test: `sql.ai.stop` and `sql.explain.close` and `sql.nl2sql.*` labels are only produced by the transient bars (the persistent toolbar uses `sql.nl2sql.chip`, `sql.explain.button`, `sql.run`, etc.), so their absence proves the transient stops are gone. `sql.history` is deliberately excluded from the list (it is the overlay title, not a button).

- [ ] **Step 2: Run the whole suite + full gate.**

Run: `cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav`
Expected: 16 tests PASS. Then the full controller gate:
`cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p dat0-app --no-fail-fast`
Confirm no regression in `tests/input_nav.rs`, `tests/sql_console_nav.rs`, `tests/a11y_spike.rs`.

- [ ] **Step 3: Commit.**

```bash
git add crates/dat0-app/tests/sql_console_transient_nav.rs
git commit -s -m "test(sql-console): Escape-ladder priority + transient-bar non-vacuity

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-review

**Spec coverage:**
- NL→SQL Stop/Insert/Discard operable + focus-managed → Task 1. ✅
- Explain Stop/Close operable + re-home → Task 2. ✅
- Error ✕ operable, no auto-focus, Escape-dismiss below Run → Task 3. ✅
- History full listbox nav (focus-on-open, ↑/↓, Enter-pick, Close, Escape) → Task 4. ✅
- Consolidated Escape ladder (5 rungs, priority) → built Task 1, verified Task 5. ✅
- Focus-return = active editor uniformly → `EDITOR_FOCUS` sentinel, every close handler. ✅
- 2 i18n keys, 0 deps, 0 new events, 0 schema change → Global Constraints + Tasks 3/4. ✅
- T0 hard gate first → Task 1 Steps 10–11 (STOP clauses). ✅
- Test seams `#[cfg(feature="a11y-capture")]`, prod wiring unconditional → each task. ✅
- Owed human glances (focus-ring contrast, Escape feel, focus-jump feel) → carried in the design doc's backlog section; not code, no task. ✅

**Placeholder scan:** none — every step has exact paths, exact code, exact run commands + expected counts.

**Type consistency:** `pending_focus: Option<&'static str>`, `history_active: usize`, `EDITOR_FOCUS: &str`, `render_history_list(&[HistoryEntry], usize, on_pick)`, seam names (`begin_nl_preview_for_test`, `finish_nl_preview_for_test`, `nl_preview_open_for_test`, `begin_explain_for_test`, `finish_explain_for_test`, `explain_open_for_test`, `set_error_region_for_test`, `error_region_for_test`, `show_fake_history_for_test`, `history_active_for_test`, `history_open_for_test`, `active_sql_for_test`) are used identically across tasks. `HistoryEntry { sql, ran_at, ok, elapsed_ms }` matches the struct. `StopAiStream`/`CloseExplain` are existing variants. ✅

## Risks the T0 gate resolves (Task 1)

- Whether gpui drops focus when the focused `Stop` unmounts and whether the `finish_*` `pending_focus` re-home lands under `TestPlatform` — Probes 1–2.
- Whether the consolidated Escape ladder routes the NL rung correctly — Probe 3.
- The history chained `on_key_down(arrows)` firing alongside `focus_stop`'s Enter/Space is validated in Task 4's `history_arrows_move_active_clamped` (the recents precedent says it fires; unverified on this surface until then).
