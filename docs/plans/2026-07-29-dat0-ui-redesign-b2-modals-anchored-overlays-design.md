# Slice B2 — Modals pt 2 + anchored overlays (UI redesign)

Branch: `feat/ui-redesign-b2-modals-anchored-overlays` off main `abd47f2` (B1).
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6, row **B2**.
Predecessor: `docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md`.

---

## 0. What this slice is

B1 built the modal machinery — scrim, centred elevation card, `Dialog` a11y node,
a hand-rolled Tab trap, Escape, focus restore — and migrated the three
`NamePrompt`-backed modals onto it. B2 finishes the overlay story:

1. The **export dialog** moves onto `modal_host` and gains a real keyboard trap.
2. The **saved-query picker** becomes its own entity in
   `src/view/saved_query_picker.rs`, built on the listbox pattern, and moves onto
   `modal_host`.
3. The **filter popover** and the **cell editor** stay `.absolute()` but gain a
   shared `anchored_overlay(cx)` treatment so they read as surfaces rather than
   transparent floating text.
4. A `ModalContent` trait plus a single mounted-modal collector replace the
   parallel `Option` fields + `or` chain, so B3/B4 cannot repeat B1's
   "two edits, not one" hazard.

Those four `.absolute()` sites in `window.rs` (6438 popover, 6451 cell editor,
6464 export, 6564 picker) are the complete B2 surface. There are no others.

### Owner decisions taken at brainstorm (all as recommended)

| Question | Choice |
|---|---|
| Scope | Full master-plan §6 in one PR (precise anchoring stays out — §6 calls it a stretch goal) |
| Export dialog controls | Each radio group = ONE dat0 focus stop, arrows change selection (WAI-ARIA radiogroup); Export/Cancel = dat0 focus stops |
| Saved-query picker | Full modal + listbox (not an anchored panel, not styling-only) |
| `ModalContent` trait | Extract now, with 3 real implementors |

---

## 1. Verified facts this design rests on

Everything below was read at the pinned revisions, not recalled.

### 1.1 gpui-component widget focus handles are unreachable from outside

`Button` (`crates/ui/src/button/button.rs:436`) and `Radio`
(`crates/ui/src/radio.rs:130`) both build their focus handle with

```rust
window.use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
```

`Window::use_keyed_state` (`gpui-0.2.2/src/window.rs:2578`) resolves through
`with_global_id` → `with_element_state`, i.e. it is keyed by the **global
element-id path**, not by the bare key. The same key read from
`WorkspaceShell::render` sits at a different path and resolves to a *different*
state entity. So a `Vec<FocusHandle>` for `overlay::modal_trap` can never be
assembled from gpui-component widgets: **dat0 must own the handles.**

`Button` exposes `tab_index`/`tab_stop` builders but no focus-handle setter, and
its `render` calls `track_focus` on `self.base` *after* any builder chain — so
chaining `.track_focus(&our_handle)` onto a `Button` is overwritten. Hand-rolling
is the only route for the two export buttons.

### 1.2 gpui tab *groups* reorder, they do not contain

gpui 0.2.2 has `src/tab_stop.rs` with a hierarchical `TabStopMap`
(`begin_group`/`end_group`, `Window::with_tab_group` at `window.rs:2740`) — a
feature B1 did not mention. It does **not** give us a native trap:
`TabStopMap::next` falls back to `self.next(None)` (first stop in the whole
window) when the cursor runs off the end. Groups affect ordering only. B1's
conclusion — a trap must be hand-rolled from a handle list — is re-confirmed
against the newer API, not merely inherited.

### 1.3 `RadioGroup` preserves `tab_stop` but overwrites `id`

`RadioGroup::render` (`radio.rs:333`) rewrites each child's id
(`radio.id = ix.into()`) and sets `disabled`/`checked`/`on_click`. It touches
nothing else, so `Radio::new(..).label(..).tab_stop(false)` survives the group.
That is what lets a single dat0 `focus_stop` own a group without five inner
stops competing for Tab. (Explicit child ids are pointless — the group discards
them.)

### 1.4 The export dialog's open path has no `Window`

`open_export_dialog(&mut self, cx: &mut Context<Self>)` (`window.rs:2883`) has
exactly one caller: `view_actions::dispatch_export` (`view_actions.rs:304`),
which reaches the shell via `window_registry::focused_workspace_weak()` from a
bare `&mut gpui::App`. The registry stores a `gpui_handle` **per workspace
path** (`window_registry.rs:245`) but has no focused-window accessor, and
`App::active_window()` is `platform.active_window()` — untrustworthy under
`TestPlatform`.

This matters because B1 measured that **with nothing focused the dispatch path
is the window root alone and Tab is completely inert**. A modal that opens
without taking focus is keyboard-dead. §5 solves it without touching action
dispatch.

### 1.5 Current state of the two migrating overlays

- Export dialog: `.absolute().top_16().left_1_2()` — no scrim, no card, pushed
  half a viewport right. Controls are `RadioGroup` ×2 + `Button` ×2, all
  gpui-component. Tests cover `build_export` (pure) only.
- Saved-query picker: `.absolute().top_16().right_2()` with `.border_1()` and
  **no background** — a transparent bordered box over the grid. Rendered by the
  free function `query_library::render_saved_picker`, mouse-only (`div().id(..)`
  + `on_click`, zero `focus_stop`), **zero tests**.
- Filter popover: `.absolute().top_8().right_4()`; `filter_popover_entity.rs`
  contains no `FocusHandle` at all.
- Cell editor: `.absolute().top_8().left_4()`; renders a bare
  `h_flex().gap_1().p_1()` — transparent, though its inner `Input` has
  `.appearance(true)`.

### 1.6 Carried forward from B1/A6 (do not rediscover)

- `focus_stop(id, fh, tab_index, ring, on_activate)` — 5 args since A6a;
  `a11y::FOCUS_RING` no longer exists. Pass `cx.theme().d0().focus_ring`.
- `a11y()` and `a11y_label()` both **push** a node. Never add a label to a site
  that already has one.
- Drive keyboard tests with `simulate_keystrokes`, never `dispatch_action`.
- A prod-only key binding is invisible to tests — a new test binary must call
  `overlay::register_modal_keys` in its `init_components`.
- `escape` is bound upstream only under context `"Input"`; `register_modal_keys`
  already binds `gpui_component::input::Escape` under `Dat0Modal`, so any modal
  that adds `.on_action(|_: &Escape|)` gets Escape for free.

---

## 2. Design

### 2.1 `ModalContent` + a single mounted-modal collector

`src/overlay.rs` gains:

```rust
pub trait ModalContent {
    fn modal_title(&self, cx: &App) -> SharedString;
    fn modal_focus_order(&self, cx: &App) -> Vec<FocusHandle>;
}
```

Implemented by `NamePrompt`, `ExportDialog`, `SavedQueryPicker`.
`NamePrompt`'s existing `title()` / `focus_order(cx)` become the trait bodies
(keep the inherent methods as thin delegates so B1's tests and call sites do not
churn).

`window.rs` gains one collector — the single source of truth for render, count
and trap:

```rust
pub(crate) struct MountedModal {
    a11y_id: &'static str,
    title: SharedString,
    focus_order: Vec<FocusHandle>,
    content: AnyElement,
}

fn push_modal<T: ModalContent + Render>(
    out: &mut Vec<MountedModal>,
    a11y_id: &'static str,
    slot: &Option<Entity<T>>,
    cx: &App,
) { /* if let Some(e) = slot { out.push(MountedModal { .. }) } */ }

fn mounted_modals(&self, cx: &App) -> Vec<MountedModal> {
    let mut v = Vec::new();
    push_modal(&mut v, "name-prompt-modal",     &self.name_prompt,     cx);
    push_modal(&mut v, "md-token-prompt-modal", &self.md_token_prompt, cx);
    push_modal(&mut v, "ai-entry-prompt-modal", &self.ai_entry_prompt, cx);
    push_modal(&mut v, "export-modal",          &self.export_dialog,   cx);
    push_modal(&mut v, "saved-picker-modal",    &self.saved_picker,    cx);
    v
}
```

`push_modal` is generic, so each call monomorphizes for its concrete entity type
— no `dyn` and no boxing needed at the slot level.

Derived from it, replacing the B1 hand-maintained pair:

- `open_modal_count()` → `self.mounted_modals(cx).len()`;
- `render` mounts `mounted.first()` through `overlay::modal_host` and passes
  `first.focus_order` to `overlay::modal_trap` on the shell root.

**Why this is worth the churn.** B1 shipped a hazard it documented but could not
remove: a new modal is styled by `modal_host` yet silently **untrapped** unless
it is also added to the `or` chain *and* to `open_modal_count`. Two of the three
edits are invisible to the compiler. After B2 a new modal is one `push_modal`
line and every consumer follows.

`open_modal_count` gains a `&App` parameter (it reads titles/focus orders
through the entities). Its three `debug_assert!` call sites in the open paths
already hold a `cx`. The `a11y-capture` accessor `open_modal_count_for_test`
gains the same parameter, so B1's three call sites in `modal_trap_nav.rs:498-511`
become `shell.read(app).open_modal_count_for_test(app)` — two immutable borrows
of `app`, which is fine. That is the only churn B2 forces on an existing test.

### 2.2 Export dialog → modal with four owned stops

`ExportDialog` gains four `FocusHandle` fields: `format_focus`, `scope_focus`,
`run_focus`, `cancel_focus`, built in `new()` from `cx.focus_handle()`
(construction already runs under `cx.new(|_| ..)`; it becomes
`cx.new(|cx| ExportDialog::new(cx))`).

**Radio groups become one stop each** — the WAI-ARIA radiogroup pattern, which is
both the correct semantics and cheaper than five stops:

```rust
div()
    .focus_stop("export-format-group", &format_focus, 0, ring, activate_noop)
    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
        match ev.keystroke.key.as_str() {
            "left"  => { this.format_ix = prev(this.format_ix, 3); cx.notify(); }
            "right" => { this.format_ix = next(this.format_ix, 3); cx.notify(); }
            _ => {}
        }
    }))
    .a11y("export-format-group", AccessRole::Button, t("export.format"))
    .child(RadioGroup::horizontal("export-format").children([
        Radio::new("csv").label(t("export.format.csv")).tab_stop(false),
        // …
    ]))
```

Horizontal format group → Left/Right; vertical scope group → Up/Down. Selection
**wraps**, per the WAI-ARIA radiogroup convention — deliberately unlike the
list surfaces, whose arrows clamp (`empty_state.rs:436-439` uses
`.min(len-1)` / `saturating_sub`); a 2- or 3-item radio group that dead-ends is
worse than one that cycles. `focus_stop`'s Enter/Space
`on_activate` is a documented no-op on a radiogroup: the selection *is* the
state, and making Enter submit from a group would be a surprising second submit
path. Chaining a second `.on_key_down` after `focus_stop` is the established
in-repo shape (`empty_state.rs:451-452`).

**Export/Cancel become `focus_stop` divs.** Since `Button` cannot take our
handle (§1.1), `overlay.rs` gains

```rust
pub fn modal_button(
    id: &'static str,
    label: SharedString,
    fh: &FocusHandle,
    ring: Hsla,
    variant: ModalButton,     // Primary | Ghost
    cx: &App,
    on_activate: impl Fn(&mut Window, &mut App) + 'static + Clone,
) -> impl IntoElement
```

styled entirely from tokens — `Primary` = `theme.primary` bg +
`theme.primary_foreground` text, `Ghost` = transparent + `theme.foreground`,
both with `theme.radius` and A2 `Sp` padding. Both colour pairs are already
gated by `theme_contrast_gate.rs`, so no new contrast work and no new colour
literal (A4 ratchet stays at `[("window.rs", 1)]`).

`modal_button` is used by `ExportDialog` (Export = Primary, Cancel = Ghost)
**and by `NamePrompt`**, whose Save/Cancel are currently unstyled
`px_3 py_1` text divs. Ids (`name-prompt-ok`, `name-prompt-cancel`) and a11y
labels stay byte-identical, so B1's `modal_trap_nav.rs` is unaffected. This is
the one deliberate step past a literal reading of §6: without it the app would
ship two modals whose buttons look nothing alike, which is precisely what the
redesign exists to stop.

Escape: `.on_action(cx.listener(|_this, _ev: &Escape, _w, cx| cx.emit(ExportEvent::Cancel)))`
on the dialog root, exactly as `NamePrompt` does.

Trap order: `[format_focus, scope_focus, run_focus, cancel_focus]`.

Title: new i18n key `export.title` = "Export". Verified absent from
`crates/dat0-i18n/src/strings/en.json` — the file has `export.format*`,
`export.scope*`, `export.run`, `export.cancel`, `export.done.title`,
`export.failed.title` and no `export.title`. (A5's lesson: a duplicate key
silently overwrites the live value with no error.)

The dialog's own `Label::new(t("export.format"))` heading stays; `modal_host`
supplies the dialog *title*.

### 2.3 Saved-query picker → `src/view/saved_query_picker.rs`

A new entity replacing the window-level flag + free function. It is the
listbox pattern proven by recents (`empty_state.rs:448`) and the catalog tree:
**ONE container focus stop, an active index, chained arrow handling** — never
per-row focus handles.

```rust
pub struct SavedQueryPicker {
    session: Arc<Mutex<Session>>,   // read LIVE, so a delete shrinks the list next frame
    active: usize,
    list_focus: FocusHandle,
    close_focus: FocusHandle,
}

pub enum SavedQueryPickerEvent { Pick(String), Delete(uuid::Uuid), Cancel }
```

- Keys on the container: Up/Down move `active` (clamped, no wrap — matches
  recents), Enter → `Pick(sql of active)`, Delete/Backspace → `Delete(id of
  active)`, Escape → `Cancel` (via the `Escape` action, as above).
- Mouse behaviour is preserved exactly: row click picks, per-row ✕ deletes, the
  header ✕ closes.
- Rows keep the active-row ring (`cx.theme().d0().focus_ring`), the same visual
  vocabulary as recents.
- Empty list: `active` clamps to 0, Enter/Delete are no-ops. The header and
  close button still render.
- `ModalContent`: title `t("sql.load_query")`, focus order
  `[list_focus, close_focus]`.

Routing stays in `window.rs` and reuses the existing logic verbatim — `Pick`
runs the current `queue_load` closure, `Delete` calls `delete_named_query` then
notifies the picker so its live read re-runs. The shell keeps ownership of
session mutation; the picker only reads.

`query_library::render_saved_picker` and its doc paragraph are deleted;
`render_history_list` and `first_line` stay (the console still uses them).

### 2.4 `anchored_overlay`

```rust
/// A non-modal floating surface: elevation card + occlude, positioned by the
/// caller. No scrim, no trap — these overlays stay operable alongside the shell.
pub fn anchored_overlay(cx: &App) -> Div
```

Returns `div().elevation(Elevation::Overlay, cx.theme()).occlude()`. Call sites
become

```rust
crate::overlay::anchored_overlay(cx).absolute().top_8().right_4().child(p.clone())
```

for the filter popover and the cell editor. Both are transparent today, so this
is the first time either reads as a surface; `.occlude()` additionally stops a
click on the overlay's padding from reaching the grid underneath.

Precise anchoring (popover under its funnel icon, editor over its cell) is
**out of scope** — the master plan lists it as a stretch goal and it needs
element bounds plumbing that has nothing to do with theming.

### 2.5 Open and dismiss plumbing

**Export (no `Window` at the call site — §1.4).** `open_export_dialog` keeps its
signature and sets a new `pending_modal_focus: bool`. `WorkspaceShell::render`
— which *does* hold `&mut Window` — drains it: captures
`modal_restore_focus = window.focused(cx)`, focuses the modal's first stop,
clears the flag. This is the in-repo precedent `SqlConsole::queue_load` already
uses (a windowless enqueue drained by a render that has a real `Window`), and it
avoids touching action dispatch, the window registry, or `TestPlatform`.
Capturing the restore target at drain time is equivalent to capturing it at open
time: nothing has moved focus in between.

**Picker (has a `Window`).** `show_saved_picker` gains a `window` parameter — its
only caller (`window.rs:4053`, the `ShowSaved` console-event arm) already holds
one, next to the `SaveQuery` arm that passes `window` to `open_name_prompt`. It
captures `modal_restore_focus`, builds the entity, focuses `list_focus`
directly, like the prompts.

**Dismiss.** `export_dialog_sub` moves from `cx.subscribe` to `cx.subscribe_in`
so the handler has a `&mut Window`; every dismiss arm (Export-complete, Cancel,
Escape) calls `restore_modal_focus(window)`. Same for the picker's subscription.

### 2.6 What does *not* change

`grid/mod.rs` and the `Table` delegate are untouched. `build_export` and its
test are untouched. Session schema is untouched (B9 owns the next bump). The
`Escape` ladder, the `Dat0Modal` key context and `register_modal_keys` are
unchanged — B2 adds implementors, not mechanism.

---

## 3. Tests — `crates/dat0-app/tests/modal_b2_nav.rs`

One new binary, harness copied per-binary from `modal_trap_nav.rs` (the
established convention), and its `init_components` **must** call
`overlay::register_modal_keys` — a prod-only binding is otherwise invisible and
a green suite can hide a dead key path. All keyboard input via
`simulate_keystrokes`; `dispatch_action` is banned because the keymap is the
mechanism under test.

Export modal:

1. opens focused on the format group — proves the §2.5 render-drain works;
2. Tab visits format → scope → Export → Cancel → **wraps to format**, never
   reaching a background stop;
3. Shift-Tab walks the same ring backwards;
4. Left/Right change the format selection; Up/Down change the scope selection;
5. Enter on Export emits `ExportEvent::Export` carrying the **arrow-selected**
   scope and format, not the defaults;
6. Escape emits exactly **one** `Cancel` (two bindings match while the modal is
   up — the cell-editor double-fire class).

Picker modal:

7. opens with the list container focused;
8. Down/Up move the active row;
9. Enter emits `Pick` with the active row's SQL;
10. Delete emits `Delete` with the active row's id and the row disappears;
11. Tab cycles `[list, close]` and wraps;
12. Escape closes and restores focus to where it was.

Structural:

13. `mounted_modals`-derived `open_modal_count` agrees with the mounted set for
    each of the five modals in turn;
14. **non-vacuity** — perturb one label/handle and confirm the relevant
    assertions go red before committing (the A5/A6 lesson: a red-first step that
    only proves an `unresolved import` proves nothing).

Existing suites that must stay green untouched: `modal_trap_nav.rs` (B1's trap
over `NamePrompt` — `modal_button` must not move its ids or labels),
`export_select_build.rs`, `filter_popover_entity_smoke.rs`,
`cell_editor_nav.rs`, `sql_console_nav.rs`.

---

## 4. Invariants and gates

- **Style-lint ratchet stays `[("window.rs", 1)]`.** Every colour in
  `modal_button`, `anchored_overlay` and the picker comes from `cx.theme()` or
  `d0()`. Any new literal fails `tests/style_lint.rs`.
- **Contrast.** `primary`/`primary_foreground` and `foreground`/`background` are
  already gated in `theme_contrast_gate.rs` on all three builtins; B2 introduces
  no new pair.
- **a11y suites.** Ids and labels of existing stops are preserved; new stops add
  nodes rather than renaming any.
- **Bench.** `grid/mod.rs` is not in the diff, so the macOS `grid_scroll` bench
  carries no structural risk — but it is still push-to-main-only, so the
  post-merge run gets watched and the artifact downloaded
  (`gh run download <run> -n grid-scroll-bench-<sha>`), continuing the series
  A4 16873 → 15066 → A5 14605 → A6 15220 → B1 14954.
- **macOS disk.** +1 test binary. B1 cost ~0.3 Gi (`DISK[after-live-ai]`
  4.7 → 4.5 Gi); expect similar. The #65 hotfix line is 2.9 Gi. Unspent lever if
  it tightens: add `--target` to the bench so it stops compiling DuckDB a third
  time.
- **Local gate** (the substitute gate — `cargo test --workspace` and
  `cargo bench` are both unrunnable on this machine, macOS 27 / Xcode 26.6,
  pre-existing and reproducing on `main`): `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -D warnings`, `cargo test -p dat0-app`,
  the same with `--features a11y-capture`, and with
  `--features a11y-capture,gallery`.

---

## 5. Owed human glance (grows)

B2 is broadly visible, so it adds to the standing list — all three themes, high
contrast most of all:

- export dialog **centred in a scrim** instead of floating half a viewport right,
  with hand-rolled Primary/Ghost buttons replacing gpui-component `Button`s;
- `NamePrompt`'s Save/Cancel restyled through the same `modal_button`;
- saved-query picker as a **centred modal listbox** instead of a transparent
  top-right box, with an active-row ring;
- filter popover and cell editor as **real cards** (bg + border + shadow, flat
  under HC) for the first time;
- focus rings on 6 new stops (2 radiogroups, 2 export buttons, picker list,
  picker close).

---

## 6. Non-goals

- Precise anchoring of the popover/cell editor (stretch goal, master plan §6).
- Any DockArea work (B5+).
- Session schema changes (B9).
- Giving `filter_popover_entity.rs` its own focus stops — it has none today, and
  making a mouse-only popover keyboard-operable is a slice of its own, not a
  side effect of styling it.
- A modal *stack*. The single-modal invariant from B1 stands; `mounted_modals`
  returns a `Vec` only so the count and the first entry derive from one list.
