# Slice B1 — ModalHost Implementation Plan

> **For agentic workers:** Steps use checkbox (`- [ ]`) syntax for tracking. Execute task-by-task;
> each task ends green (`cargo test -p dat0-app --features a11y-capture` for the touched binaries)
> and gets its own commit.

**Goal:** Give dat0's three `NamePrompt` modals a full-window inert scrim, a centered elevation
card with a real `Dialog` accessibility node, a manual Tab/Shift-Tab focus trap, a modal-scoped
Escape, and focus restore on dismiss.

**Architecture:** A new `src/overlay.rs` declares two dat0-owned actions (`ModalTab`,
`ModalTabPrev`) and binds them — plus the existing `gpui_component::input::Escape` — to
`tab` / `shift-tab` / `escape` under a `Dat0Modal` key context. `modal_host()` wraps a content
element in a scrim div carrying that key context, so the bindings win over gpui-component's
`Root`-scoped Tab bindings by keymap depth precedence. The trap is an explicit ordered
`Vec<FocusHandle>` supplied by the modal.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35`, dat0's A2 token scales,
dat0's `a11y-capture` AccessKit test harness, kittest.

**Design doc:** `docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md` (commit `688de55`).
Read §1 before touching any key handling — it explains why an `on_key_down` trap is impossible.

## Global Constraints

- Branch `feat/ui-redesign-b1-modal-host` off main `635175d`. Every commit `git commit -s` (DCO).
- `cargo fmt --all` before every commit. `cargo clippy --workspace --all-targets -D warnings` must
  stay at exit 0. Note `pub const ALL: &'static [T]` fails `clippy::redundant_static_lifetimes` —
  write `&[T; N]`.
- **Drive keyboard behaviour in tests with `simulate_keystrokes`, never `dispatch_action`.**
  `dispatch_action` bypasses the keymap, which is the entire mechanism this slice adds. The
  existing `input_nav.rs` Escape test uses `dispatch_action` and is exactly why the
  Escape-from-Cancel gap was never caught.
- **Zero new colour literals.** The scrim uses `cx.theme().overlay`. The A4 style-lint ratchet must
  stay at `ALLOW = &[("window.rs", 1)]` — `cargo test -p dat0-app --test style_lint` 4/4.
- No new i18n keys: the modal title comes from the existing prompt. Do not touch
  `crates/dat0-i18n/src/strings/en.json`.
- `grid/mod.rs` must not be touched (macOS bench protection).
- Session schema untouched.
- **`cargo test --workspace` and `cargo bench` do not run on this machine** (pre-existing macOS 27 /
  Xcode 26.6 breakage in libduckdb-sys; reproduces on `main`). The substitute local gate is in
  Task 6.
- Implementers run only the focused test commands named in their task. The full sweep is the
  controller's job (Task 6).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/dat0-app/src/overlay.rs` | **New.** Modal actions, key registration, `modal_host`, trap math. |
| `crates/dat0-app/src/lib.rs` | Add `pub mod overlay;` |
| `crates/dat0-app/src/view/name_prompt.rs` | Add `focus_order()` + `title()`; delete stale doc comment. |
| `crates/dat0-app/src/window.rs` | Register keys; route 3 mount sites through `modal_host`; focus-restore field + helper; single-modal invariant. |
| `crates/dat0-app/tests/modal_trap_nav.rs` | **New.** Gate probes, trap behaviour, Escape, focus restore, `Dialog` node. |

---

## Task 0: Hard gate — characterize today's broken behaviour

**Files:**
- Create: `crates/dat0-app/tests/modal_trap_nav.rs`

**Interfaces:**
- Consumes: `support::{A11ySnapshot, press_tab, press_shift_tab}`,
  `WorkspaceShell::{open_name_prompt_for_test, name_prompt_entity_for_test, name_prompt_open_for_test}`,
  `NamePrompt::seed_value_for_test`, all existing.
- Produces: the harness functions `set_config_dir`, `build_empty_session`, `open_shell_window`,
  `init_components`, `enter_async_harness`, `open_prompt_with_log`, `tab_until` — copied verbatim
  from `tests/input_nav.rs` per the per-binary-copy convention. Tasks 3-5 add tests to this file.

**STOP CLAUSE:** if `gate_tab_escapes_the_modal_today` or `gate_escape_from_cancel_is_dead_today`
FAILS on unmodified `635175d`, the premise of this slice is wrong. Stop, report which probe passed,
and do not proceed to Task 1.

- [ ] **Step 1: Copy the harness**

Create `crates/dat0-app/tests/modal_trap_nav.rs`. Copy verbatim from `tests/input_nav.rs`:
the module header attributes (`#![allow(dead_code, unused_imports)]`), `mod support;`, the `use`
block, `const BUDGET`, and the functions `set_config_dir` (lines 41-45), `build_empty_session`
(51-60), `open_shell_window` (66-80), `init_components` (85-87), the `AsyncHarness` struct +
`enter_async_harness` (96-~130), `tab_until` (148-156), and `open_prompt_with_log` (484-505).

Replace the module doc comment with:

```rust
//! Modal focus-trap coverage (UI redesign B1).
//!
//! Drives Tab / Shift-Tab / Escape through the REAL keymap with
//! `simulate_keystrokes`. Never `dispatch_action` — that bypasses the keymap,
//! and the keymap is the mechanism under test (design doc §1).
//!
//! Task 0's `gate_*` tests characterize the PRE-B1 behaviour and are inverted
//! into the real assertions by Task 3. They exist so the defect is proven to be
//! real before any fix is written.
```

Also copy `open_console_with_log` (input_nav.rs `~172`) — Task 3 needs it.

Add to `init_components` — nothing yet; Task 1 adds the `register_modal_keys` line here.

Note the two-`press_tab` shape in the probe below: the first Tab moves off Cancel, the second moves
one further. Two hops make the "escaped" case unambiguous — a single hop could land on the modal's
own unlabelled text field and read as `None` for the wrong reason.

- [ ] **Step 2: Write the three gate probes**

```rust
/// PRE-B1: Tab past Cancel leaves the modal entirely — the WCAG 2.4.3 gap.
/// Task 3 inverts this into `tab_wraps_from_last_stop_to_first`.
#[gpui::test]
#[serial]
fn gate_tab_escapes_the_modal_today(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    press_tab(vcx);
    press_tab(vcx);
    let after = A11ySnapshot::capture(vcx).focused_label().map(str::to_string);
    assert!(
        !matches!(after.as_deref(), Some("Save") | Some("Cancel")),
        "PRE-B1 premise: Tab past Cancel escapes the modal; landed on {after:?}"
    );
    drop(state);
}

/// PRE-B1: Escape does nothing once focus leaves the text field, because
/// `escape` is bound only under key context "Input". Task 3 inverts this into
/// `escape_from_cancel_dismisses`.
#[gpui::test]
#[serial]
fn gate_escape_from_cancel_is_dead_today(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        log.borrow().is_empty(),
        "PRE-B1 premise: Escape from Cancel emits nothing; got {:?}",
        log.borrow()
    );
    assert!(
        vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "PRE-B1 premise: the modal is still open after Escape from Cancel"
    );
    drop(state);
}

/// Control that must hold BEFORE and AFTER B1: one Escape with the text field
/// focused emits exactly ONE Cancel. Task 1 adds a second `escape` binding, so
/// two bindings will then match; this guards against the cell-editor slice's
/// Enter-double-fire failure mode.
#[gpui::test]
#[serial]
fn escape_with_field_focused_emits_exactly_one_cancel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, log) = open_prompt_with_log(&shell, vcx);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    let cancels = log
        .borrow()
        .iter()
        .filter(|e| matches!(e, NamePromptEvent::Cancel))
        .count();
    assert_eq!(cancels, 1, "exactly one Cancel per Escape; got {cancels}");
    drop(state);
}
```

Delete the first `assert_ne!` in `gate_tab_escapes_the_modal_today` — it is a placeholder line and
must not ship. The `assert!(!matches!(...))` is the real premise assertion.

- [ ] **Step 3: Run the gate**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav`
Expected: **3 passed, 0 failed.** All three describe today's behaviour.

If `gate_tab_escapes_the_modal_today` or `gate_escape_from_cancel_is_dead_today` FAILS, invoke the
STOP CLAUSE above.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/modal_trap_nav.rs
git commit -s -m "test(theme): B1 T0 — gate probes proving the modal trap + Escape gaps are real"
```

---

## Task 1: `src/overlay.rs` — actions, key registration, `modal_host`

**Files:**
- Create: `crates/dat0-app/src/overlay.rs`
- Modify: `crates/dat0-app/src/lib.rs` (add `pub mod overlay;` in alphabetical position, between
  `pub mod onboarding;` and `pub mod package;`)
- Modify: `crates/dat0-app/src/window.rs:1790` (register in `run_app`)
- Modify: `crates/dat0-app/tests/modal_trap_nav.rs` (register in `init_components`)

**Interfaces:**
- Consumes: `crate::a11y::{A11yExt, AccessRole}`, `crate::theme::tokens::{Elevation,
  ElevationStyled}`, `gpui_component::ActiveTheme` for `cx.theme()`.
- Produces:
  - `pub const MODAL_CONTEXT: &str = "Dat0Modal";`
  - `pub fn register_modal_keys(cx: &mut gpui::App)`
  - `pub fn modal_host(a11y_id: &'static str, title: SharedString, focus_order: Vec<FocusHandle>,
    content: AnyElement, cx: &App) -> impl IntoElement`
  - actions `ModalTab`, `ModalTabPrev` in the `dat0_modal` namespace.

- [ ] **Step 1: Write the failing unit test**

Create `crates/dat0-app/src/overlay.rs` containing ONLY the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::next_index;

    #[test]
    fn next_index_cycles_forward_with_wrap() {
        assert_eq!(next_index(3, Some(0), 1), 1);
        assert_eq!(next_index(3, Some(1), 1), 2);
        assert_eq!(next_index(3, Some(2), 1), 0, "last wraps to first");
    }

    #[test]
    fn next_index_cycles_backward_with_wrap() {
        assert_eq!(next_index(3, Some(2), -1), 1);
        assert_eq!(next_index(3, Some(0), -1), 2, "first wraps to last");
    }

    #[test]
    fn next_index_snaps_back_when_focus_is_outside() {
        assert_eq!(next_index(3, None, 1), 0, "Tab from outside enters at the first stop");
        assert_eq!(next_index(3, None, -1), 2, "Shift-Tab from outside enters at the last stop");
    }
}
```

Add `pub mod overlay;` to `crates/dat0-app/src/lib.rs`.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p dat0-app --lib overlay::`
Expected: FAIL — `cannot find function 'next_index' in this scope`.

- [ ] **Step 3: Write the module**

Put this ABOVE the `#[cfg(test)] mod tests` block in `crates/dat0-app/src/overlay.rs`:

```rust
//! Modal overlay host (UI redesign B1) — full-window scrim, centered elevation
//! card, and a hand-rolled Tab focus trap.
//!
//! ## Why the trap is built from actions, not `on_key_down`
//!
//! gpui dispatches action bindings BEFORE `on_key_down` listeners
//! (`gpui-0.2.2/src/window.rs:3833-3848`: the binding loop `return`s as soon as
//! one binding consumes, and only a fully-unconsumed keystroke reaches
//! `finish_dispatch_key_event` → `dispatch_key_down_up_event`). gpui-component's
//! `Root` binds `tab`/`shift-tab` as actions under key context "Root"
//! (`crates/ui/src/root.rs:21-22`) and consumes them, so no `on_key_down`
//! handler in dat0 ever sees a Tab keystroke.
//!
//! Those upstream action TYPES are not nameable — `gpui_component`'s `root`
//! module is private (`crates/ui/src/lib.rs:11`) — so dat0 declares its own and
//! binds them to the same keystrokes under a DEEPER key context. gpui's keymap
//! sorts matched bindings by context depth, deepest first
//! (`gpui-0.2.2/src/keymap.rs:165`), and `Window::context_stack` builds the
//! stack root-first, so `Dat0Modal` — mounted below `Root` — wins.
//!
//! `escape` reuses the EXISTING `gpui_component::input::Escape` action rather
//! than declaring a new one, so `NamePrompt`'s current `on_action(Escape)`
//! handler catches it unchanged. Upstream binds `escape` only under key context
//! "Input" (`crates/ui/src/input/state.rs:120`), which is why Escape used to do
//! nothing once focus left a modal's text field.

use gpui::{
    AnyElement, App, FocusHandle, InteractiveElement, IntoElement, KeyBinding, ParentElement as _,
    SharedString, Styled as _, Window, div,
};
use gpui_component::ActiveTheme as _;

use crate::a11y::{A11yExt as _, AccessRole};
use crate::theme::tokens::{Elevation, ElevationStyled as _};

gpui::actions!(dat0_modal, [ModalTab, ModalTabPrev]);

/// Key context carried by the scrim. Every focus stop inside a modal sits below
/// it, so the modal-scoped bindings outrank `Root`'s.
pub const MODAL_CONTEXT: &str = "Dat0Modal";

/// Bind the modal-scoped keys.
///
/// MUST be called by production (`run_app`) **and** by every test binary's
/// `init_components` — the test harness calls only `gpui_component::init`, so a
/// prod-only binding is invisible to tests and a green suite can hide a dead
/// production key path.
pub fn register_modal_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", ModalTab, Some(MODAL_CONTEXT)),
        KeyBinding::new("shift-tab", ModalTabPrev, Some(MODAL_CONTEXT)),
        KeyBinding::new(
            "escape",
            gpui_component::input::Escape,
            Some(MODAL_CONTEXT),
        ),
    ]);
}

/// Pure index arithmetic for the trap, extracted so it is unit-testable without
/// a `Window`. `cur == None` means focus is currently OUTSIDE the modal — the
/// next Tab pulls it back in rather than letting it wander.
fn next_index(len: usize, cur: Option<usize>, delta: isize) -> usize {
    match cur {
        Some(i) => (i as isize + delta).rem_euclid(len as isize) as usize,
        None if delta > 0 => 0,
        None => len - 1,
    }
}

/// Move focus one stop along `handles`, wrapping. Never propagates: this is a
/// trap, not a wrap-around convenience.
fn cycle(handles: &[FocusHandle], delta: isize, window: &mut Window, cx: &App) {
    if handles.is_empty() {
        return;
    }
    let cur = window
        .focused(cx)
        .and_then(|f| handles.iter().position(|h| *h == f));
    window.focus(&handles[next_index(handles.len(), cur, delta)]);
}

/// Wrap `content` in a scrim + centered elevation card with a manual Tab trap.
///
/// `focus_order` is the modal's stops in VISUAL order and is the trap's only
/// source of truth — gpui's `tab_index` is global rather than sibling-scoped
/// (every dat0 `focus_stop` passes 0 and relies on paint order), so the cycle
/// cannot be expressed as tab-index ordering.
///
/// `a11y_id` must be `&'static str`: `a11y()` records into the click-id side-map
/// and chains `debug_selector`.
pub fn modal_host(
    a11y_id: &'static str,
    title: SharedString,
    focus_order: Vec<FocusHandle>,
    content: AnyElement,
    cx: &App,
) -> impl IntoElement {
    let forward = focus_order.clone();
    let backward = focus_order;
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        // Inert scrim: `occlude` blocks the mouse from everything behind it, so
        // the obscured shell cannot be operated while a modal is up, but
        // clicking the scrim does NOT dismiss. All three prompts hold typed text
        // (a query name, an API key, a MotherDuck token) that a stray click
        // must not discard.
        .bg(cx.theme().overlay)
        .occlude()
        .key_context(MODAL_CONTEXT)
        .on_action(move |_: &ModalTab, window, app| cycle(&forward, 1, window, app))
        .on_action(move |_: &ModalTabPrev, window, app| cycle(&backward, -1, window, app))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .elevation(Elevation::Modal, cx.theme())
                .a11y(a11y_id, AccessRole::Dialog, title.to_string())
                .child(content),
        )
}
```

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p dat0-app --lib overlay::`
Expected: **3 passed.**

- [ ] **Step 5: Register in production and in the test harness**

In `crates/dat0-app/src/window.rs`, immediately after the
`crate::view::sql_console::register_sql_console_keys(cx);` line (~1790) inside `run_app`'s
`application.run` closure:

```rust
        // Register the `Dat0Modal`-scoped tab/shift-tab/escape bindings so the
        // modal focus trap and modal Escape are live in production. Tests must
        // call this too (see `overlay::register_modal_keys`).
        crate::overlay::register_modal_keys(cx);
```

In `crates/dat0-app/tests/modal_trap_nav.rs`, extend `init_components`:

```rust
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    // The harness calls only `gpui_component::init`, so the modal-scoped
    // bindings production registers in `run_app` are absent unless we add them
    // here (the carve-out #7 lesson: a green test over a dead key path).
    cx.update(dat0_app::overlay::register_modal_keys);
}
```

- [ ] **Step 6: Verify the gate probes still describe reality**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav`
Expected: `gate_tab_escapes_the_modal_today` **now FAILS**? No — it must still PASS. The bindings
are registered, but nothing mounts a `Dat0Modal` key context yet, so no modal-scoped binding can
match. `escape_with_field_focused_emits_exactly_one_cancel` must also still pass.
Expected: **3 passed.** If any fail, the binding registration is reaching a context it should not —
stop and report.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/overlay.rs crates/dat0-app/src/lib.rs \
        crates/dat0-app/src/window.rs crates/dat0-app/tests/modal_trap_nav.rs
git commit -s -F - <<'EOF'
feat(theme): B1 T1 — overlay::modal_host, Dat0Modal key context, trap math

New `src/overlay.rs`: dat0-owned ModalTab/ModalTabPrev actions bound under
a `Dat0Modal` key context alongside the existing input::Escape action, a
scrim + Elevation::Modal card with an AccessRole::Dialog node, and the
pure `next_index` trap arithmetic (unit-tested without a Window).

Registered in `run_app` and in the new test binary's `init_components`.
No call site mounts the host yet — that is T3.
EOF
```

---

## Task 2: `NamePrompt` accessors

**Files:**
- Modify: `crates/dat0-app/src/view/name_prompt.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `NamePrompt::focus_order(&self, cx: &gpui::App) -> Vec<FocusHandle>` and
  `NamePrompt::title(&self) -> SharedString`. Task 3 calls both.

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/modal_trap_nav.rs`:

```rust
/// The prompt's declared focus order is exactly [field, Save, Cancel] — the
/// trap's only source of truth, so a render reorder must break this test.
#[gpui::test]
#[serial]
fn prompt_focus_order_is_field_ok_cancel(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (prompt, _log) = open_prompt_with_log(&shell, vcx);
    let (order, field) = vcx.update(|_w, app| {
        let p = prompt.read(app);
        (p.focus_order(app), p.input_focus_handle_for_test(app))
    });
    assert_eq!(order.len(), 3, "field + Save + Cancel");
    assert_eq!(order[0], field, "the text field is first");
    drop(state);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav prompt_focus_order`
Expected: FAIL — `no method named 'focus_order' found`.

- [ ] **Step 3: Add the accessors**

In `crates/dat0-app/src/view/name_prompt.rs`, inside the plain `impl NamePrompt` block (the one
ending at line 72, NOT the `#[cfg(feature = "a11y-capture")]` block), after `fn value`:

```rust
    /// The prompt's focus stops in VISUAL order — the source of truth for
    /// `overlay::modal_host`'s Tab trap (B1). A render change that reorders the
    /// buttons must update this; `prompt_focus_order_is_field_ok_cancel` in
    /// `tests/modal_trap_nav.rs` guards the head of the list.
    pub fn focus_order(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        vec![
            self.input.read(cx).focus_handle(cx),
            self.ok_focus.clone(),
            self.cancel_focus.clone(),
        ]
    }

    /// The prompt's title, used as the accessible name of the modal's `Dialog`
    /// node (B1).
    pub fn title(&self) -> SharedString {
        self.title.clone()
    }
```

- [ ] **Step 4: Delete the stale doc comment**

In the `#[cfg(feature = "a11y-capture")] impl NamePrompt` block, replace the doc comment on
`input_focus_handle_for_test` (currently lines 82-88, describing the ABSENCE of a trap) with:

```rust
    /// The field's `FocusHandle` — lets a test re-focus INTO the modal or assert
    /// the head of `focus_order`.
```

- [ ] **Step 5: Run the test**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav prompt_focus_order`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/name_prompt.rs crates/dat0-app/tests/modal_trap_nav.rs
git commit -s -m "feat(theme): B1 T2 — NamePrompt declares its focus order and title"
```

---

## Task 3: Wire the three mount sites; invert the gate probes

**Files:**
- Modify: `crates/dat0-app/src/window.rs:6393-6432` (the three overlay blocks)
- Modify: `crates/dat0-app/tests/modal_trap_nav.rs`

**Interfaces:**
- Consumes: `crate::overlay::modal_host` (Task 1), `NamePrompt::{focus_order, title}` (Task 2).
- Produces: the modals are trapped. Task 4 adds focus restore on top of these same call sites.

- [ ] **Step 1: Replace the three overlay blocks**

In `crates/dat0-app/src/window.rs`, replace the three `let *_overlay` bindings (currently at
6393-6432) with the following. Keep their positions and the surrounding comments' intent; the
`.children(...)` attachment order at 6907-6913 is unchanged.

```rust
        // Save-query name-prompt modal (P5b T8, re-hosted in B1). Mounted by
        // `open_name_prompt`; emits `NamePromptEvent` routed via the stored
        // `name_prompt_sub` subscription (Confirm → save + dismiss, Cancel →
        // dismiss). `modal_host` supplies the scrim, the centered card, the
        // `Dialog` a11y node and the Tab trap.
        let name_prompt_overlay: Option<gpui::AnyElement> = self.name_prompt.as_ref().map(|p| {
            let (title, order) = {
                let prompt = p.read(cx);
                (prompt.title(), prompt.focus_order(cx))
            };
            crate::overlay::modal_host(
                "name-prompt-modal",
                title,
                order,
                p.clone().into_any_element(),
                cx,
            )
            .into_any_element()
        });

        // MotherDuck token-entry modal (P5c T11, re-hosted in B1).
        let md_token_prompt_overlay: Option<gpui::AnyElement> =
            self.md_token_prompt.as_ref().map(|p| {
                let (title, order) = {
                    let prompt = p.read(cx);
                    (prompt.title(), prompt.focus_order(cx))
                };
                crate::overlay::modal_host(
                    "md-token-prompt-modal",
                    title,
                    order,
                    p.clone().into_any_element(),
                    cx,
                )
                .into_any_element()
            });

        // AI key/model entry modal (P9c-1 T9, re-hosted in B1).
        let ai_entry_prompt_overlay: Option<gpui::AnyElement> =
            self.ai_entry_prompt.as_ref().map(|p| {
                let (title, order) = {
                    let prompt = p.read(cx);
                    (prompt.title(), prompt.focus_order(cx))
                };
                crate::overlay::modal_host(
                    "ai-entry-prompt-modal",
                    title,
                    order,
                    p.clone().into_any_element(),
                    cx,
                )
                .into_any_element()
            });
```

`into_any_element()` needs `gpui::IntoElement` in scope — `window.rs` already imports the gpui
prelude. If the compiler disagrees, add `use gpui::IntoElement as _;` to the existing import block
rather than inventing a new one.

- [ ] **Step 2: Invert the two gate probes**

In `crates/dat0-app/tests/modal_trap_nav.rs`, replace `gate_tab_escapes_the_modal_today` with:

```rust
/// Tab past the last stop wraps to the first — the trap. Inverted from Task 0's
/// `gate_tab_escapes_the_modal_today`, which asserted the pre-B1 escape.
#[gpui::test]
#[serial]
fn tab_wraps_from_last_stop_to_first(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    press_tab(vcx);
    // Wrapping lands on the text field, which carries no a11y label of its own,
    // so assert the NEXT hop is Save rather than the field itself.
    press_tab(vcx);
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some("Save"),
        "Tab past Cancel wraps to the field, then to Save — focus never leaves the modal"
    );
    drop(state);
}
```

and replace `gate_escape_from_cancel_is_dead_today` with:

```rust
/// Escape cancels from ANY stop, not just the text field. Inverted from Task 0's
/// `gate_escape_from_cancel_is_dead_today`.
#[gpui::test]
#[serial]
fn escape_from_cancel_dismisses(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (_p, log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        log.borrow()
            .iter()
            .any(|e| matches!(e, NamePromptEvent::Cancel)),
        "Escape from Cancel emits Cancel; got {:?}",
        log.borrow()
    );
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "Escape from Cancel dismisses the modal"
    );
    drop(state);
}
```

- [ ] **Step 3: Add the remaining trap tests**

Append to the same file:

```rust
/// Shift-Tab from the first stop wraps to the last.
#[gpui::test]
#[serial]
fn shift_tab_wraps_from_first_stop_to_last(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // The field holds focus on open, so one Shift-Tab wraps straight to Cancel.
    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    press_shift_tab(vcx);
    assert_eq!(
        A11ySnapshot::capture(vcx).focused_label(),
        Some("Cancel"),
        "Shift-Tab from the field wraps to the last stop"
    );
    drop(state);
}

/// Focus that is already outside the modal is pulled back in by the next Tab.
#[gpui::test]
#[serial]
fn tab_snaps_focus_back_into_the_modal(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (prompt, _log) = open_prompt_with_log(&shell, vcx);
    // Force focus outside the modal without going through the trap.
    vcx.update(|window, _app| window.blur());
    vcx.run_until_parked();
    press_tab(vcx);
    let landed = vcx.update(|window, app| {
        prompt
            .read(app)
            .focus_order(app)
            .first()
            .map(|h| h.is_focused(window))
            .unwrap_or(false)
    });
    assert!(landed, "Tab with focus outside the modal enters at the first stop");
    drop(state);
}

/// Escape with the SQL console open closes the MODAL ONLY — the console stays.
/// (Master plan's named B1 regression.)
#[gpui::test]
#[serial]
fn escape_over_console_closes_only_the_modal(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, _clog) = open_console_with_log(&shell, vcx);

    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    tab_until(vcx, "Cancel");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        !vcx.update(|_w, app| shell.read(app).name_prompt_open_for_test()),
        "the modal closed"
    );
    // The console has no `_for_test` visibility shim and this slice must not add
    // one; assert on what it PAINTS instead. `sql-run` renders "Run" while idle
    // (`sql_console.rs:826-830`), so the button's presence proves the console is
    // still mounted behind the dismissed modal.
    assert!(
        A11ySnapshot::capture(vcx).query_by_role(
            dat0_app::a11y::AccessRole::Button,
            &dat0_i18n::t("sql.run"),
        ),
        "the console behind it did NOT close"
    );
    drop(state);
}

/// The modal card emits a real `Dialog` node named by the prompt title.
#[gpui::test]
#[serial]
fn modal_emits_a_named_dialog_node(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    let (prompt, _log) = open_prompt_with_log(&shell, vcx);
    let title = vcx.update(|_w, app| prompt.read(app).title().to_string());
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.query_by_role(dat0_app::a11y::AccessRole::Dialog, &title),
        "the modal card emits a Dialog node named {title:?}"
    );
    drop(state);
}
```

`A11ySnapshot::query_by_role(role, label) -> bool` is `tests/support/mod.rs:128`. It **panics on
duplicate matches** (it wraps kittest's unique-match `query`), so if the prompt title happens to
also appear as a `Dialog`-role node elsewhere this will panic rather than fail — that would itself
be a finding worth reporting.

- [ ] **Step 4: Run the suite**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav`
Expected: all tests pass, including `escape_with_field_focused_emits_exactly_one_cancel` — that one
is the double-dispatch control and is now meaningful, because two `escape` bindings match while the
field has focus.

- [ ] **Step 5: Prove non-vacuity**

Two perturbations, each run then reverted:

1. In `name_prompt.rs`, swap the last two entries of `focus_order`. Run the suite.
   Expected: `tab_wraps_from_last_stop_to_first` FAILS. Revert.
2. In `overlay.rs`, comment out the `escape` `KeyBinding` line. Run the suite.
   Expected: `escape_from_cancel_dismisses` FAILS. Revert.

**After reverting either probe, run `touch` on the reverted file before re-running** — an
`mv`-style revert can backwards-date the file and cargo will silently reuse the stale binary (the
A6 lesson; it produced a false RED that was briefly committed on).

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav`
Expected: back to all-pass.

- [ ] **Step 6: Regression sweep of the neighbouring suites**

Run:
```bash
cargo test -p dat0-app --features a11y-capture --test input_nav
cargo test -p dat0-app --features a11y-capture --test keyboard_nav
cargo test -p dat0-app --features a11y-capture --test ai_nav
```
Expected: all pass. `input_nav`'s `tab_until(vcx, "Cancel")` still terminates — Cancel is still
reachable, the walk just cannot overshoot any more.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/modal_trap_nav.rs
git commit -s -F - <<'EOF'
feat(theme): B1 T3 — route the three NamePrompt modals through modal_host

All three prompts (save-name, AI key/model entry, MotherDuck token) now
render inside the scrim + trapped card instead of a bare
`.absolute().top_16().left_1_2()` wrapper.

Inverts T0's two gate probes into the real assertions: Tab past the last
stop wraps instead of escaping, and Escape cancels from any stop instead
of only from the text field. Adds Shift-Tab wrap, snap-back, the
console-behind-modal Escape regression, and the Dialog-node assertion.

Closes the WCAG 2.4.3 gap deferred out of kbd-nav carve-out #6.
EOF
```

---

## Task 4: Focus restore on dismiss

**Files:**
- Modify: `crates/dat0-app/src/window.rs` — field near `name_prompt_sub` (~2262); init in `new`
  (~2420); `open_name_prompt_with` (4823-4843); `on_name_prompt_event` (4850-4883);
  `open_md_token_prompt` (5199-5238); `open_ai_entry_prompt` (5783-5824)
- Modify: `crates/dat0-app/tests/modal_trap_nav.rs`

**Interfaces:**
- Consumes: `Window::focused(&App) -> Option<FocusHandle>`, `Window::focus(&FocusHandle)`.
- Produces: `WorkspaceShell::restore_modal_focus(&mut self, window: &mut Window)`;
  `on_name_prompt_event` gains a `window: &mut Window` parameter.

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/modal_trap_nav.rs`:

```rust
/// Dismissing a modal returns focus to whatever held it before the modal opened.
#[gpui::test]
#[serial]
fn dismiss_restores_focus_to_the_pre_open_stop(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // Land focus on a known shell stop and record its label.
    press_tab(vcx);
    let before = A11ySnapshot::capture(vcx)
        .focused_label()
        .map(str::to_string);
    assert!(before.is_some(), "the harness must start from a labelled shell stop");

    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();

    let after = A11ySnapshot::capture(vcx)
        .focused_label()
        .map(str::to_string);
    assert_eq!(after, before, "focus returned to the pre-open stop");
    drop(state);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav dismiss_restores_focus`
Expected: FAIL — `after` is `None` or some other stop, because nothing restores focus yet.

- [ ] **Step 3: Add the field and the helper**

In `crates/dat0-app/src/window.rs`, add beside the other prompt fields (after
`name_prompt_intent`, ~2273):

```rust
    /// Focus to return to when the currently-open modal dismisses (B1). Set from
    /// `window.focused(cx)` in each modal's open path, BEFORE `NamePrompt::new`
    /// moves focus to the field; `take`n in the dismiss path so a double dismiss
    /// cannot re-focus a stale handle.
    modal_restore_focus: Option<gpui::FocusHandle>,
```

Initialise it in `WorkspaceShell::new` beside `name_prompt_intent: None,` (~2423):

```rust
            modal_restore_focus: None,
```

Add the helper next to `on_name_prompt_event`:

```rust
    /// Return focus to the stop that held it before the modal opened (B1). No-op
    /// when nothing was focused (e.g. the modal was opened from a menu action).
    fn restore_modal_focus(&mut self, window: &mut Window) {
        if let Some(fh) = self.modal_restore_focus.take() {
            window.focus(&fh);
        }
    }
```

- [ ] **Step 4: Capture the pre-open focus in all three open paths**

In `open_name_prompt_with`, `open_md_token_prompt` and `open_ai_entry_prompt`, insert as the FIRST
statement after the `use` line — before the `cx.new(|cx| NamePrompt::new(...))` call, which focuses
the field:

```rust
        self.modal_restore_focus = window.focused(cx);
```

- [ ] **Step 5: Restore in the name-prompt dismiss path**

`on_name_prompt_event` has no `&mut Window`. Convert its subscription to the Window-aware form
already used by the other two prompts (`cx.subscribe_in`, window.rs:5203 and 5795).

In `open_name_prompt_with`, replace the subscription:

```rust
        let sub = cx.subscribe_in(
            &prompt,
            window,
            |ws: &mut Self, _prompt, ev: &NamePromptEvent, window, cx| {
                ws.on_name_prompt_event(ev.clone(), window, cx);
            },
        );
```

Change the signature of `on_name_prompt_event`:

```rust
    fn on_name_prompt_event(
        &mut self,
        ev: crate::view::name_prompt::NamePromptEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
```

and add the restore immediately before its closing `cx.notify();`:

```rust
        self.restore_modal_focus(window);
        cx.notify();
```

`on_name_prompt_event` has exactly one caller (the subscription you just edited) — verify with
`rg -n "on_name_prompt_event" crates/dat0-app` before and after; if a second caller appears, stop
and report.

- [ ] **Step 6: Restore in the AI and MotherDuck dismiss paths**

In `open_md_token_prompt`, rename the closure's `_window` parameter to `window` and add
`ws.restore_modal_focus(window);` in **both** arms — in `Confirm` right after
`ws.md_token_prompt_sub = None;`, and in `Cancel` right after `ws.md_token_prompt_sub = None;`.

In `open_ai_entry_prompt`, the closure already binds `window`. Add
`ws.restore_modal_focus(window);` in **both** arms right after `ws.ai_entry_prompt_sub = None;`.
In the `Confirm` arm this must come BEFORE the `ws.handle_ai_panel_event(ev, window, cx);` call, so
that a handler which opens another modal captures the restored focus rather than the field's.

- [ ] **Step 7: Run the test**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav`
Expected: all pass, including `dismiss_restores_focus_to_the_pre_open_stop`.

- [ ] **Step 8: Prove non-vacuity**

Comment out the body of `restore_modal_focus`. Run the suite; expected:
`dismiss_restores_focus_to_the_pre_open_stop` FAILS. Revert, `touch` the file, re-run — back to
all-pass.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/modal_trap_nav.rs
git commit -s -F - <<'EOF'
feat(theme): B1 T4 — restore focus to the pre-open stop on modal dismiss

Records `window.focused(cx)` in each modal's open path and returns focus
there on dismiss, so closing a prompt no longer strands focus and force
the next Tab to restart from the top of the shell.

`on_name_prompt_event` gains a `&mut Window` parameter; its subscription
converts from `cx.subscribe` to `cx.subscribe_in`, matching what the AI
and MotherDuck prompts already do.
EOF
```

---

## Task 5: Single-modal invariant

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (helper + three `debug_assert!`s + a11y-capture shim)
- Modify: `crates/dat0-app/tests/modal_trap_nav.rs`

**Interfaces:**
- Produces: `WorkspaceShell::open_modal_count(&self) -> usize` (crate-private) and
  `open_modal_count_for_test(&self) -> usize` (gated on `a11y-capture`, alongside the other shims
  near window.rs:7126).

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/modal_trap_nav.rs`:

```rust
/// At most one modal is ever mounted. The three prompt fields are independent
/// `Option`s, so this invariant is representable-but-forbidden; a debug_assert
/// in each open path fails loudly if a future flow breaks it.
#[gpui::test]
#[serial]
fn at_most_one_modal_is_open(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert_eq!(
        vcx.update(|_w, app| shell.read(app).open_modal_count_for_test()),
        0,
        "no modal before opening one"
    );
    let (_p, _log) = open_prompt_with_log(&shell, vcx);
    assert_eq!(
        vcx.update(|_w, app| shell.read(app).open_modal_count_for_test()),
        1,
        "exactly one modal while a prompt is up"
    );
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| shell.read(app).open_modal_count_for_test()),
        0,
        "back to zero after dismiss"
    );
    drop(state);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav at_most_one_modal`
Expected: FAIL — `no method named 'open_modal_count_for_test'`.

- [ ] **Step 3: Add the helper, the asserts and the shim**

Next to `restore_modal_focus` in `crates/dat0-app/src/window.rs`:

```rust
    /// How many of the three `NamePrompt`-backed modals are currently mounted
    /// (B1). The fields are independent `Option`s, so two-at-once is
    /// representable; the invariant is that it never happens, and each open path
    /// `debug_assert!`s it rather than the app growing a modal stack nothing
    /// needs.
    fn open_modal_count(&self) -> usize {
        [
            self.name_prompt.is_some(),
            self.md_token_prompt.is_some(),
            self.ai_entry_prompt.is_some(),
        ]
        .iter()
        .filter(|open| **open)
        .count()
    }
```

At the END of each of `open_name_prompt_with`, `open_md_token_prompt` and `open_ai_entry_prompt` —
after the field assignment, before `cx.notify();`:

```rust
        debug_assert!(
            self.open_modal_count() <= 1,
            "two modals mounted at once ({} open) — B1 assumes a single modal; \
             see docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md §2.7",
            self.open_modal_count()
        );
```

In the `#[cfg(feature = "a11y-capture")]` shim block near window.rs:7126, beside
`name_prompt_open_for_test`:

```rust
    /// How many modals are mounted — the B1 single-modal invariant.
    pub fn open_modal_count_for_test(&self) -> usize {
        self.open_modal_count()
    }
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_trap_nav`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/modal_trap_nav.rs
git commit -s -m "feat(theme): B1 T5 — assert the single-modal invariant"
```

---

## Task 6: Controller gate

**Files:** none modified unless a gate fails.

- [ ] **Step 1: Formatting and lints**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: both exit 0.

- [ ] **Step 2: Full local suite (the substitute gate)**

`cargo test --workspace` and `cargo bench` do NOT run on this machine — pre-existing macOS 27 /
Xcode 26.6 breakage in libduckdb-sys, which reproduces on `main`. Verify with
`git stash && cargo test --workspace 2>&1 | tail -5 && git stash pop` only if you doubt it.

```bash
cargo test -p dat0-app
cargo test -p dat0-app --features a11y-capture
cargo test -p dat0-app --features a11y-capture,gallery
```
Expected: 109+ test binaries (108 at A6 plus the new `modal_trap_nav`), 0 failures. **Redirect the
output to a file and count there — never pipe through `head`**, which SIGPIPEs cargo mid-write and
truncates the count (the A6 lesson).

- [ ] **Step 3: Style-lint ratchet unchanged**

```bash
cargo test -p dat0-app --test style_lint
rg -n "ALLOW" crates/dat0-app/tests/style_lint.rs
```
Expected: 4/4 pass, and `ALLOW` still reads `&[("window.rs", 1)]`. If the count moved, a colour
literal was introduced — find it and replace it with a theme read.

- [ ] **Step 4: Confirm no i18n or schema drift**

```bash
git diff --stat main...HEAD -- crates/dat0-i18n/ crates/dat0-app/src/session.rs
```
Expected: empty.

- [ ] **Step 5: Push and open the PR**

```bash
git push -u origin feat/ui-redesign-b1-modal-host
gh pr create --title "feat(theme): B1 — ModalHost scrim + Tab focus-trap (UI redesign)" \
             --body-file <path to a written body file>
```

Poll with `gh pr checks`, not `gh run watch`. Watch the macOS leg's `DISK[after-test]` and
`DISK[after-live-ai]` lines (`grep 'DISK\['`): this slice adds one test binary, A5's binary plus
assets cost ~0.5 Gi, and hotfix #65 was forced at 2.9 Gi.

- [ ] **Step 6: Squash-merge with an explicit body**

Pass `--subject` and `--body-file` explicitly on the squash merge. Never let the squash body
inherit commit subjects — and never write the CI-skip marker anywhere in a commit message, even
quoted in prose (it has silently skipped main CI twice).

- [ ] **Step 7: Watch the post-merge main run**

Confirm all 7 jobs succeed and crash-e2e spawns. `grid/mod.rs` is untouched, so the macOS
grid-scroll bench carries no expected movement — but download the artifact anyway
(`gh run download <run> -n grid-scroll-bench-<sha>…`) and record the ns/iter next to the
A4 → A6 series (16873 → 15066 → 14605 → 15220), since the local bench is unrunnable and this is the
only measurement the slice will get.

---

## Self-review

**Spec coverage:** design §2.1 → T1; §2.2 → T1 (`next_index`/`cycle`) + T3 (behaviour);
§2.3 → T1 (binding) + T0/T3 (probes, double-dispatch control); §2.4 → T2; §2.5 → T3;
§2.6 → T4; §2.7 → T5; §3 → T1 step 5; §4 → T0 + T3; §5 → T6; §6 (owed glance) → recorded, human.

**Placeholder scan:** clean. One placeholder assertion was caught in Task 0 and removed; the two
helper names Task 3 originally guessed at were resolved against the tree before this plan was
committed — `A11ySnapshot::query_by_role` (`tests/support/mod.rs:128`) is real, and
`sql_console_visible_for_test` does **not** exist, so that assertion was rewritten to observe the
console's painted `Run` button instead of adding a production shim this slice has no business
adding.

**Type consistency:** `focus_order` returns `Vec<FocusHandle>` in Task 2 and is consumed as
`Vec<FocusHandle>` by `modal_host` in Tasks 1 and 3. `next_index(len, cur, delta)` keeps the same
argument order in its Task 1 tests and its Task 1 implementation. `open_modal_count` (private) and
`open_modal_count_for_test` (shim) are distinct names throughout Task 5.

**Unverifiable-until-execution:** the exact focus label the harness lands on after one `press_tab`
in Task 4's restore test. The test records it dynamically rather than hardcoding a string, and
asserts it is `Some` first, so a harness change surfaces as a clear failure rather than a silent
tautology.
