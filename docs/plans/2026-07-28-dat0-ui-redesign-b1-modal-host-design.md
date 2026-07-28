# Slice B1 — ModalHost: scrim + manual Tab focus-trap (UI redesign)

Branch: `feat/ui-redesign-b1-modal-host` off main `635175d` (A6).
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6, row **B1 Modal foundation**.
Predecessor: A6 surface migrations (`635175d`) — Workstream A is complete.

---

## 0. What this slice is

The first slice of Workstream B. It introduces `src/overlay.rs`, a modal host that wraps a
content element in a full-window scrim plus a centered elevation card, and hand-rolls the
keyboard containment that gpui does not provide: a manual Tab / Shift-Tab trap and a modal-scoped
Escape.

It closes the WCAG 2.4.3 gap deferred out of kbd-nav carve-out #6, recorded in
`crates/dat0-app/src/view/name_prompt.rs:82-88`:

> This slice does not add a Tab focus-trap: the scope is "OK/Cancel are reachable" and "Escape
> cancels from within the modal", not "Tab can never leave it". So a long Tab walk (e.g. proving
> OK/Cancel are stops) can wander focus into the background shell.

It also closes a second, previously unrecorded defect of the same class found while designing this
slice: **Escape does nothing once focus leaves the text field** (§2.3).

Scope is the three `NamePrompt` mount sites (save-name, AI key/model entry, MotherDuck token).
Export dialog, saved-query picker, filter popover and cell editor stay in B2, as planned.

### Owner decisions taken at brainstorm (all as recommended)

| Decision | Choice |
|---|---|
| Trap style | Explicit ordered handle list; never propagates; snaps focus back if it is outside |
| Scrim click | Inert — blocks background clicks, does not dismiss |
| Scope | The three `NamePrompt` sites only |
| a11y role | Yes — the card emits a real `AccessRole::Dialog` node |
| Escape from OK/Cancel | Fix in this slice |
| Focus restore | Yes — capture on open, restore on dismiss |
| Handle plumbing | Plain `Vec<FocusHandle>` parameter; no trait yet |
| Two modals at once | `debug_assert!` on a single-modal invariant + a unit test |

---

## 1. The central fact: a key-down trap is structurally impossible

This is the finding that determines the whole design, and it contradicts the obvious approach
(intercept `"tab"` in an `on_key_down` handler, the way `window.rs`'s shell handler already
intercepts Escape and the arrow keys).

**gpui dispatches action bindings BEFORE `on_key_down` listeners.** In
`gpui-0.2.2/src/window.rs:3833-3848`:

```rust
for binding in match_result.bindings {
    self.dispatch_action_on_node(node_id, binding.action.as_ref(), cx);
    if !cx.propagate_event {
        /* … observers … */
        return;                       // ← key-down listeners never run
    }
}
self.finish_dispatch_key_event(event, dispatch_path, match_result.context_stack, cx);
//   └─ this is what calls dispatch_key_down_up_event
```

gpui-component's `Root` binds Tab as an action (`crates/ui/src/root.rs:21-22`):

```rust
KeyBinding::new("tab",       Tab,     Some("Root")),
KeyBinding::new("shift-tab", TabPrev, Some("Root")),
```

whose handlers call `window.focus_next()` / `focus_prev()` (`root.rs:387-393`) and consume the
event. So dat0's shell `on_key_down` (`window.rs:6834`) **never sees a Tab keystroke at all**.
A trap must therefore be built out of *actions*, not key-down handling.

### 1.1 dat0 cannot reuse gpui-component's Tab action

`gpui_component::root` is a **private module** — `crates/ui/src/lib.rs:11` reads `mod root;` and
line 85 re-exports only `pub use root::{Root, WindowExt};`. The `Tab` / `TabPrev` action types are
not nameable from dat0, so `.on_action::<root::Tab>(…)` is impossible. dat0 must declare its own
actions and bind them to the same keystrokes.

### 1.2 Why dat0's binding wins: deeper context, higher precedence

`gpui-0.2.2/src/keymap.rs:142-190` collects every binding whose context predicate matches the
current context stack, tagging each with the **depth** at which it matched
(`binding_enabled` → `predicate.depth_of(contexts)`, `keymap.rs:209-215`), then:

```rust
matched_bindings.sort_by(|(depth_a, ix_a, _), (depth_b, ix_b, _)| {
    depth_b.cmp(depth_a).then(ix_b.cmp(ix_a))          // deepest first
});
```

`Window::context_stack()` (`window.rs:4196`) builds the stack from the root down to the focused
node, so a larger depth means closer to the focused element. The modal's `Dat0Modal` context sits
below `Root` in the element tree, so a `Dat0Modal`-scoped `"tab"` binding sorts ahead of Root's.

### 1.3 A consequence worth recording: `cx.propagate()` falls through to the next binding

The loop in §1 does not stop at the first binding — it stops at the first binding that *consumes*.
An action handler that calls `cx.propagate()` therefore hands the same keystroke to the
next-highest-precedence binding, i.e. Root's `Tab` → `focus_next()`.

This slice does **not** use that (the owner chose the explicit trap, which never propagates), but
it is what makes the alternative "wrap-only sentinel" design viable, and it is load-bearing for
§2.3's Escape behaviour. Recorded so a future slice does not re-derive it.

---

## 2. Design

### 2.1 New module `src/overlay.rs`

```rust
gpui::actions!(dat0_modal, [ModalTab, ModalTabPrev]);

pub const MODAL_CONTEXT: &str = "Dat0Modal";

/// Bind the modal-scoped keys. MUST be called by production (`run_app`) AND by
/// every test binary's `init_components` — a prod-only binding is invisible to
/// tests (the harness calls only `gpui_component::init`).
pub fn register_modal_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab",       ModalTab,     Some(MODAL_CONTEXT)),
        KeyBinding::new("shift-tab", ModalTabPrev, Some(MODAL_CONTEXT)),
        KeyBinding::new("escape",    gpui_component::input::Escape, Some(MODAL_CONTEXT)),
    ]);
}

/// Wrap `content` in a scrim + centered elevation card with a manual Tab trap.
pub fn modal_host(
    a11y_id: &'static str,
    title: SharedString,
    focus_order: Vec<FocusHandle>,
    content: AnyElement,
    cx: &App,
) -> impl IntoElement;
```

`a11y_id` is `&'static str` because `a11y()` records into the click-id side-map and chains
`debug_selector` (A5 finding: a site with a dynamic id structurally cannot use `a11y()`). All
three call sites have fixed ids, so this is not a constraint in practice.

Render shape — the scrim carries the key context, so every focus stop inside the modal is under
it:

```rust
div()
    .absolute().top_0().left_0().size_full()
    .bg(cx.theme().overlay)                 // A1 gave all three builtins this key
    .occlude()                              // InteractiveElement::occlude — div.rs:997, no .id() needed
    .key_context(MODAL_CONTEXT)
    .on_action(/* ModalTab     */)          // cycle(+1)
    .on_action(/* ModalTabPrev */)          // cycle(-1)
    .flex().items_center().justify_center()
    .child(
        div()
            .elevation(Elevation::Modal, cx.theme())
            .a11y(a11y_id, AccessRole::Dialog, title)
            .child(content),
    )
```

`cx.theme().overlay` is an upstream `ThemeColor` key (`theme/theme_color.rs:201`,
`schema.rs:360`), and A1's three builtin JSONs are full-coverage (109 keys each), so it resolves
in dark, light and high-contrast without a shadcn fallback. **No new colour literal → the A4
style-lint ratchet stays at `ALLOW = &[("window.rs", 1)]`.**

`Elevation::Modal` resolves to `popover` bg + `radius_lg` + `ShadowLevel::Large`, gated on
`theme.shadow` — so under high contrast the card is flat and reads only by its border. That is
existing A2 behaviour, called out here because it drives the owed human glance (§6).

### 2.2 Trap semantics

```rust
fn cycle(handles: &[FocusHandle], delta: isize, window: &mut Window, cx: &App) {
    if handles.is_empty() { return; }
    let idx = window.focused(cx).and_then(|f| handles.iter().position(|h| *h == f));
    let next = match idx {
        Some(i) => (i as isize + delta).rem_euclid(handles.len() as isize) as usize,
        None    => if delta > 0 { 0 } else { handles.len() - 1 },   // focus was outside → snap in
    };
    window.focus(&handles[next]);
}
```

Never calls `cx.propagate()`. The `None` arm is what makes this a real trap rather than a wrap:
if focus has somehow landed outside the modal (a stray click on a still-painted background
element, a programmatic focus from an async completion), the next Tab pulls it back in.

APIs verified against gpui 0.2.2: `Window::focused(&App) -> Option<FocusHandle>` (`window.rs:1380`),
`Window::focus(&FocusHandle)` (`:1386`), `impl PartialEq for FocusHandle` (`:384`).

**gpui `tab_index` is global, not sibling-scoped** — every dat0 `focus_stop` passes `0` and relies
on paint order — which is precisely why the trap cannot be expressed as tab-index ordering and has
to be an explicit list.

### 2.3 Escape needs no new action type

`escape` is bound upstream **only** under key context `"Input"`
(`crates/ui/src/input/state.rs:120`, applied by `input/input.rs:277`). `NamePrompt`'s
`.on_action(Escape)` at `name_prompt.rs:116` therefore fires only while the text field holds
focus. Tab to OK or Cancel and Escape is dead: no binding matches, the keystroke falls through to
the shell's `on_key_down`, and the modal stays up.

Binding `"escape"` → the **existing** `gpui_component::input::Escape` action under `Dat0Modal`
fixes this without introducing a new type, and `NamePrompt`'s current handler catches it unchanged.
This mirrors `register_sql_console_keys` (`view/sql_console.rs:50-56`), which binds the same
upstream action under `"SqlConsole"`.

**The risk this creates** is double dispatch. With the field focused, two bindings now match:
Input's (deeper) and ours. Input's `escape()` is documented as a no-op that propagates for a
single-line field, so the loop in §1.3 would reach our binding too — and both dispatch the *same*
action into the *same* path, so `NamePrompt`'s handler could emit `Cancel` twice. Analysis says it
cannot: `on_action` handlers consume by default, so the first dispatch stops the loop. That is
exactly the shape of the cell-editor slice's Enter-double-fire bug, so **T0 asserts it rather than
trusting the analysis** (§4).

### 2.4 `NamePrompt` changes

Two production accessors, not `cfg`-gated:

```rust
pub fn focus_order(&self, cx: &App) -> Vec<FocusHandle>;  // [input, ok_focus, cancel_focus]
pub fn title(&self) -> SharedString;
```

The order is the visual order and is the trap's only source of truth; a future edit that reorders
the rendered buttons must update it. The stale doc comment at `name_prompt.rs:82-88` — which
documents the *absence* of the trap — is deleted.

`NamePrompt` already owns everything else this needs: `ok_focus` / `cancel_focus`, the Escape
`on_action`, focus-on-open (`name_prompt.rs:59`), and the `a11y-capture` accessors
`input_focused_for_test` / `input_focus_handle_for_test` / `seed_value_for_test`.

The card itself currently carries **no** `.a11y` / `.a11y_label` node, so adding one in
`modal_host` cannot produce the duplicate-accessible-name failure A5 documented (both helpers
`push()` a new node; they do not set an attribute).

### 2.5 Mount sites in `window.rs`

The three blocks at `window.rs:6397-6432` (`name_prompt_overlay`, `md_token_prompt_overlay`,
`ai_entry_prompt_overlay`) are each an `.absolute().top_16().left_1_2()` wrapper. Each becomes one
`overlay::modal_host(...)` call with a fixed id:

| Field | id | Opened by |
|---|---|---|
| `name_prompt` | `name-prompt-modal` | `open_name_prompt_with` (5 intents) |
| `ai_entry_prompt` | `ai-entry-prompt-modal` | `open_ai_entry_prompt` |
| `md_token_prompt` | `md-token-prompt-modal` | `open_md_token_prompt` |

Attachment order at `window.rs:6907-6913` is unchanged.

### 2.6 Focus restore

One shell field:

```rust
/// Focus to return to when the currently-open modal dismisses. Set from
/// `window.focused(cx)` at open; taken (not cloned) at dismiss.
modal_restore_focus: Option<FocusHandle>,
```

Set in the three open paths, consumed in the dismiss paths — the `Confirm` **and** `Cancel` arms of
`on_name_prompt_event` and the AI / MD equivalents. `Option::take` in the dismiss path means a
double dismiss cannot re-focus a stale handle.

Without this, dismissing leaves focus nowhere and the next Tab restarts from the top of the shell.
The master plan already assumes `modal_host` provides it — B4 says the command palette gets
"scrim/trap/focus-restore free" — so building it now avoids reopening these call sites.

### 2.7 Single-modal invariant

The three fields are independent `Option`s, so two modals stacking is representable even though no
current flow produces it. Rather than build a modal stack for a case that cannot occur:

```rust
fn open_modal_count(&self) -> usize;   // count of the three Somes
debug_assert!(self.open_modal_count() <= 1, "…");
```

plus a unit test. If a future flow breaks the invariant it fails loudly in tests instead of
silently painting two scrims and two traps.

---

## 3. Registration

`register_modal_keys(cx)` is called:

1. in production, beside `crate::view::sql_console::register_sql_console_keys(cx)` at
   `window.rs:1790`;
2. in the new test binary's `init_components`, mirroring
   `tests/sql_console_transient_nav.rs:86`.

A prod-only binding is absent in tests, because the harness calls only `gpui_component::init`.
Getting this wrong produces a green suite over a dead production key path — the failure mode that
shipped carve-out #7's Escape ladder broken past five reviews.

---

## 4. Tests — `crates/dat0-app/tests/modal_trap_nav.rs`

**T0 hard gate, red-first against unmodified `635175d`.** Three probes, all of which must fail
before any implementation lands:

| Probe | Asserted today | Meaning |
|---|---|---|
| Tab from Cancel | focus escapes into the shell | the WCAG 2.4.3 gap is real |
| Escape from Cancel | modal stays up | §2.3's second defect is real |
| One Escape with the field focused | exactly **one** `Cancel` event | double-dispatch control (passes today; must still pass after) |

If probe 1 or 2 passes on unmodified main, the premise is wrong and the slice stops for a re-scope
— the cell-editor precedent, where the T0 gate falsified the premise and turned a coverage slice
into a bug fix.

Then the behavioural suite:

- forward cycle `input → ok → cancel → input` (three Tabs, `focused_label()` asserted at each step);
- reverse cycle and wrap via Shift-Tab;
- snap-back: focus a background shell stop, press Tab, land on the modal's first stop;
- Escape from Cancel dismisses;
- Escape with the SQL console open closes the **modal only** and leaves the console open — the
  master plan's named regression;
- focus restore: focus a known shell stop → open → cancel → assert focus returned to it;
- the `Dialog` a11y node exists and carries the prompt title.

All Tab and Escape driven with `simulate_keystrokes`, never `dispatch_action` — `dispatch_action`
bypasses the keymap, which is the entire mechanism under test here.

**Non-vacuity** (A5/A6 lesson, both times it earned its keep): each new assertion is proven red by
perturbation before the suite is called done — reorder `focus_order` and watch the cycle test fail;
drop the `Dat0Modal` escape binding and watch the Escape test fail.

`seed_value_for_test` + the existing `NamePromptEvent` subscription pattern from
`tests/input_nav.rs` supply the harness; the window recipe is copied per-binary per house
convention.

---

## 5. Invariants and gates

- Full nav/a11y suite green under `--features a11y-capture`. `input_nav.rs:598`'s
  `tab_until(vcx, "Cancel")` still terminates — Cancel remains reachable, the walk just cannot
  overshoot any more.
- Escape ladder intact. The new binding only matches while `Dat0Modal` is in the context stack,
  i.e. only while a modal is up, so ladder tests (which run with no modal) are unaffected.
- `grid/mod.rs` untouched → **no macOS grid-scroll bench risk**. Post-merge watch stays routine for
  this slice, unlike A6.
- Style-lint ratchet unchanged at 1 (`window.rs`, the `drag_over` tint held for B10).
- Session schema untouched (additive changes start at B9).
- **macOS CI disk: +1 test binary.** A5's assets plus one binary cost ~0.5 Gi of headroom; main
  currently ends at 5.3 Gi, low-water `after-test` 7.0 Gi. Watch `DISK[after-live-ai]` on the PR
  run; the 2.9 Gi that forced hotfix #65 is the line.
- Local gate (`cargo test --workspace` and `cargo bench` are both unrunnable on this machine —
  pre-existing macOS 27 / Xcode 26.6 breakage, reproduces on main): `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -D warnings`, `cargo test -p dat0-app`,
  `--features a11y-capture`, `--features a11y-capture,gallery`.

---

## 6. Owed human glance (grows)

- Scrim opacity over the shell in all three themes — is `theme.overlay` heavy enough to read as
  modal, light enough to keep context?
- **High contrast especially**: `Elevation::Modal` flattens its shadow when `theme.shadow` is
  false, so the card separates from the scrim by border alone.
- The focus ring on the modal's three stops, and the ring's contrast against the card's `popover`
  background rather than the shell background it was tuned on.
- The modal is now centered rather than pinned at `top_16 left_1_2` — a visible reposition of all
  three prompts.

---

## 7. Non-goals

- Export dialog, saved-query picker, filter popover, cell editor — **B2**.
- A `ModalContent` trait — **B2**, once there are three real implementors and the shape is known.
- A modal stack — nothing needs one; §2.7's assert documents the invariant instead.
- Precise anchoring for the non-modal overlays — B2 stretch goal, unchanged.
- Production accessibility beyond the `Dialog` node (D-015 stays open).
