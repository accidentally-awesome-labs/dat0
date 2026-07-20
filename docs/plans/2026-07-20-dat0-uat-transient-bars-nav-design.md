# SQL-console transient-bars keyboard operability — Design

> UAT keyboard-nav carve-out #7 (the four *transient* affordance bars of the SQL console).
> Continues the a11y-nav thread after carve-outs #1–#6 (recents, catalog, AI-config,
> cell-editor, SQL-console toolbar/tab-strip, SQL-console Inputs). The prior slice's design
> explicitly deferred this surface ("the `nl_preview` result strip (Insert/Discard/Stop) —
> not an `Input`; separate if ever needed"). This slice picks it up and extends it to all four
> transient bars.

## Goal

Make the four *transient* (mount/unmount) affordance bars of the SQL console fully
keyboard-operable and headlessly test-covered, shipping real production a11y behavior:

1. **NL→SQL preview strip** — `Stop` (streaming), then `Insert` / `Discard` (finished).
2. **Explain side panel** — `Stop` (streaming), then `Close` (finished).
3. **DuckDB error strip** — `Dismiss` (✕).
4. **Query-history overlay** — a scrollable list of past queries (rows load a query into a new
   tab) + `Close` (✕). Gets full listbox keyboard navigation.

Seven buttons + one listbox, all currently raw `div().on_click()` (mouse-only, no focus, no
AccessKit node). All four bars are already keyboard-*initiated* (chip→prompt→submit, Run,
`ShowHistory` action), so moving focus *into* the resulting bar is expected, not disruptive.

## Background — current behavior (verified against main `9804135`)

- **NL→SQL strip** (`sql_console.rs:1246`–`:1306`): rendered from `Option<NlPreview>`
  (`nl_preview`, field `:171`). While `p.streaming`, one `nl2sql-stop` div (`:1260`) →
  `SqlConsoleEvent::StopAiStream`. When finished, `nl2sql-insert` (`:1278`, takes the preview +
  `load_into_new_tab`) and `nl2sql-discard` (`:1293`, `clear_nl_preview`). No `focus_stop`, no
  `.a11y`, no focus movement.
- **Explain panel** (`:1308`–`:1344`): rendered from `Option<ExplainView>` (`explain`, field
  `:173`). While `e.streaming`, `explain-stop` (`:1319`) → `StopAiStream`; else `explain-close`
  (`:1332`) → `SqlConsoleEvent::CloseExplain`. Same gaps.
- **Error strip** (`ResultRegion::Error`, `:909`–`:935`): the strip already carries a content-only
  `.a11y_label(Alert, msg)` (test-only, Gap 2), but the `sql-err-dismiss` ✕ (`:925`, sets
  `region = Empty`) is a raw div — no focus, no keyboard.
- **History overlay** (`:949`–`:1002`): an `absolute` overlay built from `history_overlay:
  Option<Vec<HistoryEntry>>` (field `:162`, opened by `show_history` `:477`). `sql-history-close`
  ✕ (`:986`) clears it. Rows come from `query_library::render_history_list` (`query_library.rs:29`)
  — each `hist-row` is a raw `div().on_click()` (`:45`/`:55`) that calls `on_pick(sql, …)` →
  `load_into_new_tab`. No keyboard path to focus the list, move a selection, or pick a row.
- **Existing Escape handler** (`:1353`): one `on_action::<gpui_component::input::Escape>` on the
  console root, guarded on the active editor holding focus → moves focus to the `sql-run` handle
  (the editor trap-exit from carve-out #6). This slice **replaces** it with a consolidated ladder.

## Interaction model (decided)

**Focus-managed (modal-like).** On appear, focus moves to the primary/only button; the
streaming→finished transition **re-homes** focus across the button swap; Escape cancels; on close,
focus returns to the active editor. One exception: the **error strip does not auto-focus** (it
appears after a failed Run, when the user wants to stay in the editor to fix SQL).

## Mechanism

**`pending_focus` render-drain + consolidated Escape `on_action`** — mirrors the file's existing
`pending_load`/`queue_load` idiom (focus is a `&mut Window` operation, and the state-setters run
in `Context<Self>` methods driven by `WorkspaceShell` with no window in scope; `render` is the one
place that holds the window).

- Add `pending_focus: Option<&'static str>` on `SqlConsole`. State-setters
  (`begin_nl_preview`/`finish_nl_preview`, `begin_explain`/`finish_explain`, `show_history`, and
  the cancel/close/insert/pick handlers) stash the id to focus. `render` drains it →
  `window.focus(&self.toolbar_fh(id))`, or the active editor's `FocusHandle` for the
  return-to-editor cases (a sentinel id, e.g. `"__editor__"`, resolved to the active tab's input
  handle). Draining `finish_*`'s stash re-homes focus for free when `Stop` unmounts and the primary
  button mounts.
- **Stable handles** come from the existing get-or-insert `toolbar_fh(id, cx)` map (`:565`) — it
  lives on `SqlConsole`, so a handle keyed by a button's `&'static str` id survives the button's
  unmount and stays stable across the streaming→finished swap.
- **One** consolidated Escape `on_action` at the console root replaces the current editor-only one,
  dispatching by an ordered ladder (below).

Rejected alternative — per-bar `on_key_down` + inline `window.focus`: focus-on-*appear* still can't
happen inline (no window at `begin_*`), so the render-drain is needed anyway; and N sibling Escape
handlers race on registration order where one ordered ladder is deterministic.

## Per-surface behavior

Every button → `focus_stop(id, handle, tab_index, on_activate)` (production a11y: Tab-stop +
Enter/Space activate + focus ring) + `.a11y(id, AccessRole::Button, label)` (test-only node).
Buttons within a bar get sequential `tab_index` so Tab walks them; **no focus-trap** (Tab may leave
the bar — WCAG 2.4.3 deferred, consistent with the NamePrompt slice).

| Surface | State | Focus-on-appear | Buttons (tab order) | Activate → |
|---|---|---|---|---|
| NL→SQL strip | streaming | → `nl2sql-stop` | Stop | `StopAiStream` (existing emit) |
| | finished | re-home → `nl2sql-insert` | Insert, Discard | Insert: take preview, `load_into_new_tab`, focus=editor · Discard: `clear_nl_preview`, focus=editor |
| Explain panel | streaming | → `explain-stop` | Stop | `StopAiStream` |
| | finished | re-home → `explain-close` | Close | `CloseExplain`, focus=editor |
| Error strip | present | **no move** (stays editor) | Dismiss ✕ | `region = Empty`, keep focus |
| History overlay | open | → listbox, row 0 active | listbox + Close ✕ | row Enter/click: `load_into_new_tab`, focus=new-tab editor · Close: clear overlay, focus=editor · ↑/↓: move active |

### History listbox — reuse the recents pattern verbatim

Mirror `empty_state.rs` recents-list (`:408`–`:450`), the repo's established listbox pattern:

- Add `history_active: usize` on `SqlConsole` (reset to 0 in `show_history`; clamped to the entry
  count at render, exactly like `recents_active.min(len.saturating_sub(1))`).
- The overlay list container gets **one** `focus_stop("sql-history-list", handle, 0, activate)`
  where `activate` (Enter/Space) picks the active row's SQL via the **same** `on_pick`/
  `load_into_new_tab` path a row's `on_click` uses (mouse and keyboard cannot drift).
- Chain a **second** `on_key_down(arrows)` after `focus_stop` (gpui pushes both listeners): `down` →
  `history_active = (history_active+1).min(len-1)`, `up` → `saturating_sub(1)`, else return;
  `cx.notify()`.
- `render_history_list` gains an `active: usize` param and paints the active-row ring
  (`a11y::FOCUS_RING`) on `hist-row == active` (rows stay mouse-clickable; they are **not**
  individual tab stops). `.a11y("sql-history-list", AccessRole::Button, t("sql.history"))` on the
  container (the recents list uses `AccessRole::Button`; there is no ListBox role in the enum).
- `sql-history-close` ✕ → `focus_stop` + `.a11y` like every other button.

## Escape priority + focus-return

Consolidated `on_action::<gpui_component::input::Escape>` at the console root (first match wins;
gpui bubbles to this one ancestor handler):

```
1. history overlay open  → clear overlay, pending_focus = editor
2. nl_preview open        → streaming ? emit StopAiStream : Discard(clear_nl_preview) ; pending_focus = editor
3. explain open           → streaming ? emit StopAiStream : emit CloseExplain ; pending_focus = editor
4. ResultRegion::Error    → region = Empty (dismiss), keep current focus
5. editor focused         → focus the sql-run handle   (existing trap-exit, unchanged)
```

Error-dismiss (4) sits below the bars but above editor→Run (5): Escape-from-editor with an error
showing dismisses the error first; a second Escape then performs the Run trap-exit. Focus-return is
uniformly the active editor via `pending_focus` — Insert and history-pick open a **new** tab (which
becomes active), so focus naturally lands on the new tab's editor, ready to `Escape → Run` it. A
`Stop` (streaming cancel) flips the bar to finished, so its `finish_*` stash re-homes focus to the
primary button (Insert / Close), not the editor.

## Testing

Reuse the shared a11y kit (`focus_stop`/`.a11y`/`focused_label`/`A11ySnapshot`/`press_tab`) in a
new `tests/sql_console_transient_nav.rs`, harness helpers per-binary-copied (crate precedent). New
production wiring (the button `focus_stop`/`.a11y`, `pending_focus` drain, consolidated Escape,
history listbox) is **unconditional shipped code**; only the state-injection read/write seams are
`#[cfg(feature = "a11y-capture")]`.

- **Test seams** — integration tests are a separate crate, so the `pub(crate)` state-setters are
  unreachable and a real SSE stream / failed Run cannot run headless. Add
  `#[cfg(feature = "a11y-capture")] pub fn *_for_test` wrappers to inject each transient state:
  `begin_nl_preview` + `finish_nl_preview`, `begin_explain` + `finish_explain`, `show_history`, and
  a `ResultRegion::Error` setter. Consistent with the existing `*_for_test` accessors (`:1370`–
  `:1409`). Add read oracles: `history_active_for_test`, and reuse `editor_focused_for_test`.
- **T0 hard gate** (throwaway-but-kept spike, STOP-clauses, before the breadth suites) — this gate
  falsifies the "focus-managed" premise and confirms the harness observes these bars:
  1. Inject `begin_nl_preview` → `focused_label()` is **not** `Stop` yet (gap exists), then after
     implementing, focus lands on `nl2sql-stop`. **STOP if the harness cannot render/observe the
     strip.**
  2. Inject `finish_nl_preview(None)` while `nl2sql-stop` is focused → focus re-homes to
     `nl2sql-insert` (the swap-survival probe). **STOP if focus is dropped to nowhere.**
  3. With a bar open, `dispatch_action(input::Escape)` routes to the correct ladder rung (cancel,
     not editor→Run). **STOP if the consolidated ladder mis-prioritizes.**
  4. Inject `show_history([..])` → the `sql-history-list` container is focused, `history_active == 0`;
     `simulate_keystrokes("down")` moves it; Enter loads the active row's SQL into a new tab.
     **STOP if the chained arrow `on_key_down` doesn't fire alongside `focus_stop`.**
- **Behavioral suite:** focus-on-appear for each AI bar + history; streaming→finished re-home
  (nl2sql + explain); Insert opens a new tab and lands focus on its editor; Discard/Close/history-
  Close return focus to the editor; error strip does **not** steal focus on appear but its ✕ is Tab-
  reachable and Enter dismisses; Escape ladder (each rung, incl. error-dismiss-below-Run and the
  Run trap-exit still firing when no bar is open); history ↑/↓ move + Enter-pick + Escape-close;
  a non-vacuity negative (with every bar closed, none of the seven button ids are Tab stops).
- **Drive discipline:** button/listbox activation via `simulate_keystrokes` on the focused
  `focus_stop` div (safe — these are divs, not single-line Inputs, so the cell-editor `"\n"`-panic
  does not apply; the editor is multi-line). Escape via `dispatch_action(input::Escape)`. Seed
  preview/explain/error/history content through the `*_for_test` seams, never keystrokes.
- **Oracle:** `focused_label()`/`A11ySnapshot` for buttons + the history container;
  `editor_focused_for_test` for the return-to-editor assertions; `history_active_for_test` for
  selection movement; existing `tab_count_for_test`/`tab_titles_for_test` for Insert/pick landing a
  new tab.

## Constraints

- **Zero new dependencies**; `Cargo.toml`/`Cargo.lock`/`NOTICE` unchanged. D-015 stays open.
- **Two new i18n keys** for the ✕ buttons' accessible names — `sql.error.dismiss` +
  `sql.history.close` (localized a11y labels beat hardcoded strings). All other labels reuse
  existing keys (`sql.ai.stop`, `sql.nl2sql.insert`, `sql.nl2sql.discard`, `sql.explain.close`,
  `sql.history`).
- **No new `SqlConsoleEvent` variants** — reuse `StopAiStream`/`CloseExplain`; Insert / Discard /
  error-dismiss / history-close / row-pick stay inline handlers.
- No session-schema change; no change to SSE/streaming logic or to what the bars display.
- Toolchain pinned 1.97.0; `cargo fmt --all` before every commit; `git commit -s` + the
  `Co-Authored-By: Claude Opus 4.8 (1M context)` trailer.
- Executed via subagent-driven-development, **T0 gate first**; controller runs the
  `cargo test --workspace --no-fail-fast` + `clippy --workspace --all-targets -D warnings` gate;
  implementers run only the focused test. Model per task shape (opus for the load-bearing T0 gate +
  final review; sonnet for the history-listbox + Escape-ladder judgment; haiku only for pure
  transcription).

## Out of scope

- A Tab focus-trap inside any bar (WCAG 2.4.3) — deferred to a future modal-hardening slice, same as
  the NamePrompt slice.
- The SQL editor itself, the toolbar, and the tab strip (carve-outs #4/#5/#6, already shipped).
- Individual per-row focus stops in the history list (the container-listbox pattern is deliberate).
- Real AI streaming / SSE / failed-Run plumbing — injected via test seams, unchanged in production.

## Owed human glances (non-blocking, join the standing backlog)

- The seven button focus rings + the history active-row ring at WCAG ≥3:1 contrast, both themes.
- The Escape *feel* across all four bars (cancel-while-streaming vs cancel-when-finished vs
  error-dismiss vs the editor Run trap-exit).
- The streaming→finished focus-jump *feel* (Stop → Insert / Close).

## Risks / empirical unknowns the T0 gate resolves

- Whether gpui drops focus to nowhere when the focused `Stop` unmounts, and whether the
  `finish_*` `pending_focus` re-home lands under `TestPlatform` — **Probe 2**.
- Whether the consolidated Escape ladder prioritizes correctly when a bar is open vs when only an
  error is showing vs editor-only — **Probe 3**.
- Whether the chained second `on_key_down(arrows)` fires alongside `focus_stop`'s Enter/Space
  listener on the same history container (the recents pattern says yes; unverified on this
  surface) — **Probe 4**.
