# SQL-console `Input` keyboard operability — Design

> UAT keyboard-nav carve-out #6 (the two text-`Input` surfaces of the SQL console).
> Continues the a11y-nav thread after carve-outs #1–#5 (recents, catalog, AI-config,
> cell-editor, SQL-console toolbar/tab-strip). This slice closes the automatable a11y-nav
> backlog for the SQL console.

## Goal

Make the two text-`Input` surfaces of the SQL console keyboard-operable and headlessly
test-covered, shipping real production behavior fixes:

1. **SQL editor** — fix a genuine keyboard trap (WCAG 2.1.2): once focus is in the
   multi-line code editor, Tab/Shift-Tab indent/outdent, so there is no keyboard way out.
   Add an **Escape → focus `Run`** exit gesture.
2. **`NamePrompt` modal** — the shared single-line prompt (used by NL→SQL, Save-query,
   Save-as-table, AI-key, MotherDuck-token) is currently mouse-only end to end. Make it
   keyboard-operable: focus-on-open, Enter-submit, Escape-cancel, keyboard-reachable
   OK/Cancel. One fix covers all **5** call sites.

## Background — current behavior (verified against main `28b2217`)

- **Editor** (`ConsoleTab.input`, `sql_console.rs:39`; built `:265`/`:427`/`:455`, rendered
  `:871`/`:1243`): a real native focus target (`InputState::new` sets
  `focus_handle.tab_stop(true)`), so it is Tab-**reachable** already, positioned after the
  toolbar row in DOM order. But `gpui_component::input::Input` is **not** an
  `InteractiveElement`, so it cannot carry `.focus_stop`/`.a11y`, and `focused_label()` never
  sees it. In multi-line `code_editor` mode the `Input` binds `tab → IndentInline` /
  `shift-tab → OutdentInline` (gated on `is_multi_line()`), which consume Tab/Shift-Tab →
  **keyboard trap**. The run shortcut `Cmd/Ctrl+Enter → SqlRun` (registered `window.rs:1705`,
  handled on the shell root `window.rs:6631`) already fires even while the editor is focused.
- **`NamePrompt`** (`name_prompt.rs`, single-line; opened via `open_name_prompt_with`,
  `window.rs:4695`; rendered as a bare overlay `window.rs:6269`): nothing focuses the input on
  open; `Enter` emits `InputEvent::PressEnter` but **no subscriber** fires `Confirm`; `Escape`
  is a no-op (`clean_on_escape=false`, no ancestor `Escape` action); OK/Cancel are plain
  `.on_click` divs with **no** `focus_stop` → unreachable by keyboard.

## Surface 1 — SQL editor: Escape → focus Run

- Register an **Escape handler on an ancestor** of the editor (the console/shell scope, e.g. a
  `on_action::<gpui_component::input::Escape>` or an app action bound to `escape`), whose body
  moves focus to the existing `sql-run` `FocusHandle` (already minted in the last slice's
  `toolbar_focus` map — reuse `toolbar_fh("sql-run", cx)` / the stored handle).
- **Layering is free via gpui's bubble order:** when the autocomplete completion popup is open,
  the editor consumes Escape (dismiss popup) and the ancestor handler never runs; when the popup
  is closed, Escape bubbles up and we move focus to `Run`. So the observable behavior is exactly
  "Escape closes the popup if open, otherwise leaves the editor onto Run" — with no explicit
  popup bookkeeping on our side. The T0 gate must confirm the ancestor handler actually fires
  when the editor is focused and the popup is closed.
- **No new code for run-from-editor** — `Cmd/Ctrl+Enter → SqlRun` already works; this slice only
  adds a test asserting it (and the T0 gate empirically confirms `SqlRun` wins the keymap tie
  over the editor's own `secondary-enter`).
- **No** auto-focus-on-open and **no** `.a11y` wrapper on the editor (YAGNI: entry already works
  via Tab/click; the test oracle is `tab.input.read(cx).focus_handle(cx).is_focused(window)`).

## Surface 2 — `NamePrompt`: full keyboard operability

Applied once in the shared component (fixes all 5 call sites):

1. **Focus on open** — call `.focus(window)` on the prompt's `InputState` when it is opened
   (`open_name_prompt_with`, or in `NamePrompt::new`/its first render).
2. **Enter → Confirm** — subscribe to the input's `InputEvent::PressEnter` and emit
   `NamePromptEvent::Confirm` (the same event the OK button's `on_click` emits). Single-line
   `enter()` already `cx.propagate()`s + emits `PressEnter`; we just add the missing subscriber.
3. **Escape → Cancel** — wire `Escape` to emit `NamePromptEvent::Cancel` (via an ancestor
   `on_action::<input::Escape>` on the overlay, or repurposing `clean_on_escape` semantics;
   the plan will pick the mechanism the T0 gate proves fires).
4. **Keyboard-reachable buttons** — add `focus_stop` + `.a11y(Button, label)` to the OK and
   Cancel buttons (mirroring every toolbar button in this repo), so Tab reaches them and
   Enter/Space activates them. Labels reuse the existing i18n keys the buttons already show.

## Testing

Reuse the shared a11y kit (`focus_stop`/`.a11y`/`focused_label`/`A11ySnapshot`/`press_tab`) +
the `Input` focus oracle. **Drive Inputs via `dispatch_action`, never `simulate_keystrokes`**
(the cell-editor slice proved `simulate_keystrokes("enter")` injects a stray `"\n"` that panics
a still-open single-line Input).

- **T0 hard gate** (throwaway-but-kept spike, STOP-clauses, before the breadth suites):
  1. Tab into the editor (assert `input.focus_handle().is_focused(window)`), then
     `dispatch_action(input::Escape)` → focus lands on `Run` (`focused_label() == t("sql.run")`
     / the `sql-run` handle is focused). **STOP if the ancestor Escape handler doesn't fire.**
  2. With the editor focused, `dispatch_action(input::Enter{secondary:true})` (Cmd/Ctrl+Enter)
     → a `SqlConsoleEvent::Run` is emitted. **STOP if `SqlRun` loses the keymap tie.**
  3. Open a `NamePrompt`; assert its input is focused on open;
     `dispatch_action(input::Enter{secondary:false})` → `Confirm` emitted with the typed value;
     re-open, `dispatch_action(input::Escape)` → `Cancel` emitted. **STOP if either doesn't route.**
  4. OK/Cancel are `focus_stop`-reachable (`tab_labels` contains their labels).
- **Behavioral suite:** editor trap-exit (Escape→Run) + run-from-editor (both grid + the
  `secondary` path); NamePrompt focus-on-open, Enter-submit (value carried), Escape-cancel,
  OK/Cancel reach+operate; a non-vacuity negative (e.g. with the prompt closed its labels are
  not Tab stops).
- **Focus oracles:** `focus_handle().is_focused(window)` for the editor/prompt Inputs;
  `focused_label()`/`A11ySnapshot` for the buttons. Seed prompt text via
  `InputState::set_value` (no keystrokes).
- Only `_for_test` read accessors are `#[cfg(feature = "a11y-capture")]`; production wiring
  (Escape handler, PressEnter subscriber, focus-on-open, button `focus_stop`/`.a11y`) is
  **unconditional real shipped code**.

## Constraints

- **Zero new dependencies**; `Cargo.toml`/`Cargo.lock`/`NOTICE` unchanged. D-015 stays open.
- **Zero new i18n keys** — reuse the labels the editor/prompt/buttons already display.
- Toolchain pinned 1.97.0; `cargo fmt --all` before every commit; `git commit -s` + the
  `Co-Authored-By: Claude Opus 4.8 (1M context)` trailer.
- Executed via subagent-driven-development, **T0 gate first**; controller runs the
  `cargo test --workspace --no-fail-fast` + `clippy --workspace --all-targets -D warnings`
  gate; implementers run only the focused test.

## Out of scope

- Editor internals (typing, autocomplete list navigation, selection) — the widget's own concern.
- Auto-focus-the-editor-on-console-open (entry already works; behavior-change creep).
- An AccessKit role/label wrapper for the editor Input (tested via the focus-handle oracle).
- The `nl_preview` result strip (Insert/Discard/Stop) — not an `Input`; separate if ever needed.

## Owed human glances (non-blocking, join the standing backlog)

- The Escape-exit *feel* in the running editor (popup-open vs popup-closed behavior).
- The NamePrompt keyboard flow across the 5 dialogs (focus-on-open, Enter, Escape) + the
  OK/Cancel focus-ring contrast (WCAG ≥3:1 both themes).

## Risks / empirical unknowns the T0 gate resolves

- Whether an ancestor `on_action::<input::Escape>` actually fires when the code-editor Input is
  focused and the popup is closed (bubble-order assumption) — **Probe 1**.
- Whether `SqlRun` wins the `cmd/ctrl+enter` keymap tie over the editor's `secondary-enter`
  (source analysis says yes; unverified live) — **Probe 2**.
- Whether the `NamePrompt` `PressEnter`/`Escape` dispatch routes headlessly and focus-on-open
  takes under `TestPlatform` — **Probe 3**.
