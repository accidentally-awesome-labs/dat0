# dat0 UI Redesign — Slice B4: Command Palette (design)

**Date:** 2026-07-30
**Branch:** `feat/ui-redesign-b4-command-palette` off main `b80cdb1` (B3 status bar)
**Master plan:** `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B4 (size M-L)
**Predecessors this slice builds on:** B1 (`overlay::modal_trap`, `register_modal_keys`), B2 (`ModalContent`, `modal_button`, `MountedModal` registry, focus-set-in-render), B3 (`src/view/` is where rendered shell surfaces live)

---

## 1. What exists today, and why it never shipped

`src/command_palette.rs` is 95 lines: a unit-tested `filter()` and an `open()` that logs a
`tracing::info!` and returns. Its own module doc records the blocker:

> Sheet/Modal mounts require a `&mut Window` context that the action-dispatch path (which has
> only `&mut App`) cannot produce without hopping through `WindowRegistry`.

That blocker is gone. B2 established the flag-drain pattern — an `&mut App` path sets a field, and
`WorkspaceShell::render` (which *has* a `&mut Window`) drains it — and proved that **focus set from
inside `render` sticks**. The export dialog opens exactly this way today.

The same constraint also froze 7 of the registry's descriptors as breadcrumbs. Their dispatch
bodies say so verbatim:

```rust
tracing::debug!("action: sql.new_tab dispatched via registry — handled view-scoped (needs Window); no-op from App path");
```

A palette that lists those and does nothing on Enter is the defect PRs #59/#60 fixed for menus
(`View ▸ Settings…` was greyed out for months because `OpenSettings` had no handler). B4 does not
repeat it.

### 1.1 The 35 descriptors, classified

`tests/action_registry.rs:91` asserts `reg.count() == 35`. Read against the tree:

| Class | Count | Ids |
|---|---|---|
| **Live** — `dispatch(&mut App)` does real work | 22 | `window.new`, `settings.open`, `recovery.review`, `import.cancel`, `workspace.open`, `workspace.save`, `live.refresh`, `onboarding.take_tour`, `view.undo`, `view.redo`, `view.export`, `chart.visualize`, `ai.panel.open`, `sql.run`, `sql.cancel`, `sql.close_tab`, `view.copy`, `view.cut`, `view.paste`, `view.fill_down`, `view.set_null`, `view.delete_rows` |
| **Window-blocked** — needs a `&mut Window` the App path cannot supply | 7 | `console.toggle`, `sql.new_tab`, `sql.save_query`, `sql.load_query`, `sql.history`, `sql.save_as_table`, `view.save_as_table` |
| **Argument-blocked** — a no-arg invocation cannot carry the argument | 2 | `view.set_value` (needs a `Scalar`), `view.delete_column` (needs a `col_ix`) |
| **Stub** — no body at all | 4 | `file.open`, `theme.toggle`, `recents.show`, `sample_data.retry_taxi` |

The Window-blocked/argument-blocked split is a **correction made while reading the code**: the
initial framing treated all 9 as Window-blocked. `edit_actions.rs:88-100` and `:116-128` say
plainly that the context menu bypasses the registry for those two because a no-arg dispatch cannot
carry a value or a column index — a different problem with a different answer.

**Decisions (owner-approved at brainstorm, all recommended options):**

- The 7 Window-blocked ids are **routed** through the shell — the palette has a `&mut Window`.
- The 2 argument-blocked + 4 stub ids are **hidden**. → **29 of 35 shown, every one of them works.**

---

## 2. Module split

Two modules, following B2/B3 (`view/saved_query_picker.rs`, `view/status_bar.rs`):

| File | Contents |
|---|---|
| `src/command_palette.rs` (rewritten in place) | Pure model. `filter()` **byte-unchanged**, plus `rank()`, `visible_items()`, `HIDDEN`, `WINDOW_ROUTED`, and the new `open(&mut App)`. No gpui rendering. |
| `src/view/command_palette.rs` (new) | `CommandPalette` entity: `InputState`, active index, scroll handle, `Render`, `impl ModalContent`, `CommandPaletteEvent`. |

`filter()` keeps its exact signature because `tests/command_palette.rs` calls
`dat0_app::command_palette::filter(&reg, "nw")` and must compile untouched. Keeping the model free
of gpui is what makes ranking unit-testable with no `Window` — the same reason `overlay::next_index`
was extracted in B1.

The two modules share a basename across directories (`crate::command_palette` vs
`crate::view::command_palette`). That is deliberate and matches how the codebase already separates
model from view; the model module's doc comment names its sibling.

---

## 3. Open path and lifecycle

```
⌘⇧P (macOS) / ⌃⇧P (Linux), or View ▸ Command Palette
  └─ global on_action(OpenCommandPalette)          ← moved into register_command_palette_keys(cx)
       └─ command_palette::open(app)
            ├─ window_registry::focused_workspace_weak()
            ├─ upgrade + downcast to Entity<WorkspaceShell>
            └─ ws.update: pending_palette_open = true; cx.notify()
  └─ WorkspaceShell::render drains the flag (the existing block, window.rs:6343)
       ├─ modal_restore_focus = window.focused(cx)      ← BEFORE focusing anything
       ├─ self.command_palette = Some(cx.new(|cx| CommandPalette::new(window, cx)))
       └─ window.focus(&palette.input_focus_handle())   ← focus-in-render sticks (B2)
  └─ mounted_modals() gains ONE line
       └─ scrim + elevation card + Tab trap + single-modal debug_assert, all free
```

### 3.1 Why the global handler stays

The master plan proposed moving the handler to a shell-root `.on_action` "which has `&mut Window`".
B1 measured why that is wrong: **with nothing focused, the dispatch path is the window root alone**
— eight `press_tab` hops from a fresh window moved focus not at all. A shell-root handler is
therefore silently dead on a freshly-opened window, and ⌘⇧P is precisely the key a user presses
before clicking anything. The global handler fires regardless of focus; the flag-drain gives it the
`&mut Window` it lacks. This design note supersedes the master plan's line, which predates B2.

### 3.2 Registration in tests

`register_command_palette_keys(cx)` is called by `run_app` **and** by every test binary's
`init_components`. A prod-only binding is invisible to tests, which is exactly how carve-out #7's
Escape ladder shipped broken past five reviews. Same rule `overlay::register_modal_keys` already
carries.

### 3.3 Dismiss before dispatch

The palette unmounts, restores focus, and *then* runs the action. `Save Query…` and
`Save as Table…` open `NamePrompt`; with the palette still mounted, `open_modal_count(cx) <= 1`
would fire. The event handler order is not cosmetic:

```rust
self.command_palette = None;
self.pending_modal_restore = true;   // render drains it → restore_modal_focus(window)
// …only now route the action
```

---

## 4. The keyboard contract

`Input` sets key context `"Input"` on its own node, which is the **deepest** entry in the context
stack, and gpui sorts matched bindings by context depth descending (`keymap.rs:165`). So the Input
wins the keystroke→action lookup for every key it binds. Measured at pinned rev `0f0ab35`, the four
keys the palette needs behave *differently*, and only one of them is a problem:

| Key | Upstream binding + behaviour | B4's route |
|---|---|---|
| `enter` | `Enter` action → `state.rs:1168` emits `InputEvent::PressEnter` | Subscribe to `PressEnter` (`name_prompt.rs:47` precedent) |
| `escape` | `Escape` action → `state.rs:1198` calls `cx.propagate()` on a single-line field | Bubble `on_action(Escape)` on the palette root (`name_prompt.rs:168` precedent) |
| `tab` | `IndentInline` → `indent.rs:255` propagates when `!mode.is_indentable()` | B1's `modal_trap` on the shell root, unchanged |
| `up` / `down` | `MoveUp`/`MoveDown` → **`movement.rs:141`, `:163` return on single-line WITHOUT `cx.propagate()`** | ⚠ **`capture_action`** on the palette root + `cx.stop_propagation()` |

### 4.1 Why arrows need the capture phase

`on_action` handlers consume by default; a handler that returns without `cx.propagate()` ends the
keystroke. `InputState::up`'s single-line early return does exactly that. Consequences, in order:

1. A binding of `"down"` under a `"CommandPalette"` key context is **matched but never reached** —
   `MoveDown` wins on depth, is handled, and consumes.
2. An `on_key_down` listener never fires either: action bindings are dispatched *before* key-down
   listeners, and only a fully unconsumed keystroke reaches `dispatch_key_down_up_event`
   (B1's finding, `gpui-0.2.2/src/window.rs:3833-3848`).
3. Re-binding `up`/`down` under context `"Input"` ourselves would win on insertion order — and
   break arrow keys in the multi-line SQL editor, app-wide. Rejected.

`capture_action` (`gpui-0.2.2/src/elements/div.rs:328`) is the remaining interception point. The
capture phase walks the dispatch path **root→leaf** and breaks as soon as `propagate_event` goes
false (`window.rs:4028-4040`), so a capture handler on the palette root sees `MoveDown` *before*
`InputState`'s bubble handler and can stop it. It is scoped to the palette's subtree, so the SQL
editor is untouched.

**This is the slice's single largest technical risk and gets a T0 hard gate (§7).**

### 4.2 Escape ladder

Escape from either stop dismisses. `register_modal_keys` already binds `escape` under the
`Dat0Modal` context that `modal_trap` installs on the shell root, and the single-line Input
propagates, so the palette's own `on_action(Escape)` catches it from the text field *and* from the
close button. No new action type.

---

## 5. Data model

### 5.1 Visibility and routing, and what keeps them honest

Two consts in `src/command_palette.rs`:

```rust
/// Registered but never shown: no dispatch body, or an argument a no-arg
/// invocation cannot carry. Each entry names its reason.
pub const HIDDEN: &[&str] = &[ /* 6 ids */ ];

/// Shown, but the registry closure is a breadcrumb — the shell routes these
/// with the `&mut Window` the palette has and the App path does not.
pub const WINDOW_ROUTED: &[&str] = &[ /* 7 ids */ ];
```

A gate test asserts: every listed id is actually registered; the two lists are disjoint; and every
`WINDOW_ROUTED` id makes the router return `true`. Without that test a typo'd or stale id silently
does nothing — the exact failure this classification exists to prevent.

Adding a 36th action later breaks `tests/action_registry.rs:91` (`count == 35`), which forces a human
through that file; its doc comment will point at these consts. This is a weaker ratchet than a field
on `ActionDescriptor` would give, and that trade was made deliberately: a field means editing 35
struct literals and moving UI policy into the action data model.

### 5.2 The router

```rust
/// Run `id` with the Window the registry closure cannot have. Returns false if
/// this id is not window-routed, and the caller falls back to desc.dispatch(app).
pub(crate) fn run_palette_action(&mut self, id: &ActionId, window: &mut Window, cx: &mut Context<Self>) -> bool
```

One `match` on `id.as_str()`, one arm per `WINDOW_ROUTED` id, each calling a method the shell already
has (`toggle_sql_console`, `open_saved_picker`, `open_name_prompt_with(.., SaveQuery, ..)`, the
history overlay toggle, …). Two of the seven (`console.toggle`, `sql.new_tab`) also have declared
gpui action types with live shell handlers; those arms call the same shell methods rather than
`window.dispatch_action`, so all seven read identically and the router is the only place to look.

### 5.3 Ranking

`ActionRegistry::iter()` snapshots a `HashMap`, so its order is non-deterministic — the palette must
impose one. `filter()` stays as-is; ordering is a new pure function:

```rust
/// None = no match. Higher is better: 3 = case-insensitive prefix,
/// 2 = word-boundary prefix, 1 = plain subsequence (what filter() accepts).
fn rank(title: &str, query: &str) -> Option<u8>
```

`visible_items(reg, query)` = `filter()` → drop `HIDDEN` → sort by `(score desc, title asc)`. An
empty query yields all 29, title ascending. Unit-tested with no `Window`: "con" must float
"Toggle SQL Console" above "Cancel Import".

### 5.4 Keybinding hints

All 35 descriptors carry `keybinding: None` today, so the master plan's `Kbd` hints would render
nothing. **Six** ids have a real chord bound in `window.rs`; each gains
`keybinding: gpui::Keystroke::parse("…").ok()`, `cfg`-split cmd/ctrl exactly as `window.rs` binds it:

| Id | macOS | Linux | Bound at |
|---|---|---|---|
| `view.undo` | `cmd-z` | `ctrl-z` | `window.rs:1358` |
| `view.redo` | `cmd-shift-z` | `ctrl-shift-z` | `window.rs:1358` |
| `view.export` | `cmd-e` | `ctrl-e` | `window.rs:1392` |
| `sql.run` | `cmd-enter` | `ctrl-enter` | `window.rs:1536` |
| `sql.cancel` | `cmd-.` | `ctrl-.` | `window.rs:1536` |
| `console.toggle` | `cmd-shift-c` | `ctrl-shift-c` | `window.rs:1536` |

`Keystroke: Display` (`platform/keystroke.rs:431`) renders them. A unit test asserts every string
parses — a typo would otherwise silently degrade to no hint.

⚠ **`window.new` gets NO hint, and that is a finding rather than an omission.** The natural
assumption is ⌘N, but `grep '"cmd-n"'` over `src/` returns nothing: `NewWindow` has a global
`on_action` handler (`window.rs:1307`) and a File-menu item, but **no `KeyBinding`**. On macOS a
menu item's key equivalent is derived from the keymap, so File ▸ New Window shows no chord and ⌘N
does nothing — the same class as the dead menu items PR #59/#60 fixed, just one layer down. B4 does
not fix it (out of scope, and it wants its own reachability assertion); the plan records it as a
follow-up. Hinting a key that does nothing would be worse than hinting none.

`sql.new_tab` / `sql.close_tab` deliberately have no chord (`window.rs:1528-1530`: avoiding a
collision with the editor's own keymap), so they correctly get no hint either.

The hint is *descriptive*, not a second source of truth: nothing dispatches from it. A drift gate
between these strings and `window.rs`'s `bind_keys` calls is possible but out of scope; the plan
records it as a follow-up.

---

## 6. Rendering

`modal_host` supplies the scrim, the elevation card and the `Dialog` a11y node. Inside:

```
header:  title (TextRole::Title) ......................... close modal_button (Ghost)
input:   gpui-component Input, single-line, placeholder = palette.placeholder
results: focusable div  ← the container focus_stop (LISTBOX pattern)
           └─ uniform_list("palette-results", n, …).track_scroll(handle)
                row: title · muted ActionGroup tag ............ muted Kbd hint
empty:   palette.no_results when the query matches nothing
```

- **Listbox pattern**: ONE container `focus_stop` + an active index, never per-row focus handles
  (`saved_query_picker.rs` is the worked example). **List surfaces clamp; only radio groups wrap.**
- `focus_stop` is 5-arg since A6a — `(id, &fh, 0, cx.theme().d0().focus_ring, on_activate)`, and
  `a11y::FOCUS_RING` no longer exists. `tab_index` is global, so 0 everywhere.
- `uniform_list` is wrapped in a plain focusable div rather than carrying the `focus_stop` itself:
  `UniformList` does implement `InteractiveElement`/`Styled` (`uniform_list.rs:236`, `:706`), but a
  wrapper keeps the ring on a stable box and the virtualised region purely a child.
- Every arrow calls `scroll_to_item(active, ScrollStrategy::Nearest)`
  (`uniform_list.rs:146`) — without it, arrowing past the fold moves the ring somewhere the user
  cannot see, which is a keyboard-a11y hole in a keyboard-first surface.
- Focus order (`modal_focus_order`, the trap's only source of truth) = `[input, list, close]`.
- Tokens only: `Sp`, `TextRole`, `d0()`, `Elevation` via `modal_host`. The style_lint ratchet stays
  `[("window.rs", 1)]`. Note the scanner matches banned colour names in **prose**, so doc comments
  must not spell them with call parens.

### 6.1 i18n

Ten new flat keys in `crates/dat0-i18n/src/strings/en.json`: `palette.title`,
`palette.placeholder`, `palette.no_results`, and `palette.group.{navigation,theme,file,settings,
recovery,import,edit}`. `dat0_i18n::t` has no interpolation and no plural forms; none is needed
here. Check for pre-existing keys before adding — **JSON silently overwrites duplicates** (A5's
finding, which cost two keys).

---

## 7. Tests and the T0 hard gate

### 7.1 T0 — run first, STOP clauses armed

| Gate | Proves | STOP → fallback |
|---|---|---|
| **G1** | `uniform_list` rows appear in the headless a11y capture tree. The collector brackets a whole frame (`reset → refresh → run_until_parked → take_tree_update`, `a11y/mod.rs:17`) and uniform_list builds items during prepaint, so they *should* land — but this is inference, not measurement. | Replace the virtualised list with a cap-10 plain-`div` list (score-ordered, arrows clamp within the visible set). Everything else in the design is unaffected. |
| **G2** | `capture_action` intercepts `MoveDown` before `InputState` with focus in the field. Driven by `simulate_keystrokes("down")`, **never** `dispatch_action` — the latter bypasses the keymap, and a green test would hide a dead production key path. | Bind `ctrl-n`/`ctrl-p` aliases under a palette context and document the arrow keys as unsupported. |
| **G3** | `⌘⇧P` opens the palette **from the test binary** — i.e. `register_command_palette_keys` really is called by `init_components`, not just `run_app`. | None; this is a wiring bug to fix, not a design fork. |
| **G4** | A probe descriptor flips an `AtomicBool` on Enter. `install_action_registry` is a `OnceCell`, so a test binary installs one test-owned registry. | None. |

### 7.2 Suite

`tests/command_palette_nav.rs` (new binary → **112**; watch macOS `DISK[after-live-ai]`, 4.8 Gi as of
B3, and the #65 hotfix line is 2.9 Gi):

- open via real keystroke; Escape dismisses and **restores focus to where it was**
- typing narrows; ranking order asserted through the rendered tree
- arrows clamp at both ends; Enter runs the active row
- Tab cycles `input → list → close → input` and never escapes into the shell behind
- hidden ids are absent from the tree; `console.toggle` routed through the shell actually mounts
  the console (`sql_console` becomes `Some`)
- single-modal invariant: opening `Save Query…` from the palette leaves exactly one modal mounted

Unit tests in `src/command_palette.rs`: `rank` ordering, `HIDDEN ∩ WINDOW_ROUTED = ∅`, every listed
id registered, every routed id makes the router return `true`, every hint string parses.

**Non-vacuity, both directions** (the A6/B3 discipline): perturb a positive needle and watch it go
red; swap a negative assertion's needle for a string that *is* present and watch that go red too.

### 7.3 The free correctness check

`tests/a11y_spike.rs` asserts an **exact** captured-node count (8 since B3) as a frame-bracket
double-render proof, so it reacts to any capture site added to the shell regardless of label. The
palette is modal and must paint nothing on the empty hero, so **the count must stay 8**. If it
moves, the palette is rendering when it shouldn't. Treat it as a gift, not an obstacle.

---

## 8. Risks and invariants

| Risk | Handling |
|---|---|
| Virtualised rows invisible to the a11y capture | T0 G1, with a fully-specified fallback |
| Capture-phase interception doesn't work as read | T0 G2, with a fallback |
| A routed action opens a second modal while the palette is up | Dismiss before dispatch (§3.3); the existing `debug_assert!` is the backstop |
| `grid/mod.rs` / Table delegate | **Untouched** — no bench risk. Note B3 proved `benches/grid_scroll.rs` never exercises the Table delegate anyway, so a "bench held" reading here would be evidence of nothing either way. Still verify the post-merge run at STEP level and download the artifact. |
| macOS CI disk | +1 test binary. Watch `DISK[after-live-ai]`; unspent lever is adding `--target` to the bench so it stops compiling DuckDB a third time. |
| Session schema | Untouched. The palette holds no persisted state. |
| style_lint ratchet | Must stay `[("window.rs", 1)]`. |

**Local gate** (`cargo test --workspace` and `cargo bench` remain unrunnable on this machine —
macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift): `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -D warnings`, and `-p dat0-app` across
`{plain, a11y-capture, a11y-capture+gallery}`. `cargo build -p dat0-app --bin dat0` does work and is
how the owner drives the human glance.

**Owed human glance** (this slice adds a broadly visible surface): palette card in all 3 themes, HC
most of all — muted group tag legibility, right-aligned Kbd hint contrast, the active-row ring
against the card background, and the scroll behaviour when arrowing past the fold.
