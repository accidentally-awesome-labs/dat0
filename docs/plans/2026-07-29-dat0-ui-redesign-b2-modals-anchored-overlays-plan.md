# Slice B2 — Modals pt 2 + anchored overlays: Implementation Plan

> **Execution mode:** INLINE by the controller (no subagents — standing owner
> instruction for this project since A5). One commit per task. Steps use
> checkbox (`- [ ]`) syntax.

**Goal:** Put the export dialog and the saved-query picker on B1's `modal_host`
with real Tab traps, give the filter popover and cell editor a shared
`anchored_overlay` surface, and replace `window.rs`'s parallel modal `Option`
fields with a single `ModalContent`-driven collector.

**Architecture:** `src/overlay.rs` grows the vocabulary (`ModalContent` trait,
`modal_button`, `anchored_overlay`); `ExportDialog` and a new
`SavedQueryPicker` entity own their focus handles and implement the trait;
`window.rs` derives render, `open_modal_count` and `modal_trap` from one
`mounted_modals(cx)` list.

**Tech stack:** Rust 2024, gpui 0.2.2, gpui-component pinned rev `0f0ab35`,
`--features a11y-capture` for the headless AccessKit harness.

Design: `docs/plans/2026-07-29-dat0-ui-redesign-b2-modals-anchored-overlays-design.md`.
Branch: `feat/ui-redesign-b2-modals-anchored-overlays` off main `abd47f2`.

---

## Global Constraints

- **No colour literals.** `tests/style_lint.rs` must stay at
  `ALLOW = &[("window.rs", 1)]`. Every colour reads `cx.theme().<token>` or
  `cx.theme().d0().<field>`. **`transparent_black()` / `transparent_white()` /
  `rgb(` / `hsla(` are all banned** — a "ghost" button therefore sets *no*
  background rather than a transparent one.
- **`focus_stop` is 5-arg since A6a:** `focus_stop(id, fh, tab_index, ring,
  on_activate)`. `a11y::FOCUS_RING` does not exist. Pass
  `cx.theme().d0().focus_ring`. Every dat0 `focus_stop` passes `tab_index = 0`
  (gpui's tab_index is global, not sibling-scoped).
- **`a11y()` and `a11y_label()` both PUSH a node.** Never add one to a site that
  already has one. `a11y(id, role, label)` needs a `&'static str` id;
  `a11y_label(role, text)` is the only option for a dynamic id.
- **Keyboard tests use `simulate_keystrokes`, never `dispatch_action`** — the
  keymap is the mechanism under test.
- **A new test binary must call `dat0_app::overlay::register_modal_keys`** in its
  `init_components`; the harness calls only `gpui_component::init`, so the
  `Dat0Modal` bindings are otherwise absent and a green test can hide a dead
  production key path.
- **Every Tab-driven test must click into the shell first** (`focus_shell_neutrally`).
  With nothing focused the dispatch path is the window root alone and Tab is
  completely inert (B1, measured).
- `dat0_i18n::t(key) -> String`. New keys go in
  `crates/dat0-i18n/src/strings/en.json`. **Check for an existing key first —
  JSON silently overwrites duplicates.**
- `cargo fmt --all` before every commit; `git commit -s` (DCO).
- **Never write the literal CI skip marker in any commit message, even quoted.**
- Local gate per task: `cargo fmt --all --check`, `cargo clippy -p dat0-app
  --all-targets -- -D warnings`, and the task's own tests. The full sweep
  (`--workspace` clippy + the three feature combinations) is the controller's
  job at Task 6. `cargo test --workspace` and `cargo bench` are unrunnable on
  this machine (macOS 27 / Xcode 26.6, pre-existing, reproduces on `main`).

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/overlay.rs` | modify | + `ModalContent` trait, `ModalButton` enum, `modal_button()`, `anchored_overlay()` |
| `src/view/export_dialog.rs` | modify | `ExportDialog` owns 4 focus handles, radiogroups become single stops, buttons hand-rolled, Escape, `ModalContent` |
| `src/view/saved_query_picker.rs` | **create** | `SavedQueryPicker` entity — modal listbox over the session's saved queries |
| `src/view/query_library.rs` | modify | delete `render_saved_picker` + its doc paragraph; keep `render_history_list`, `first_line` |
| `src/view/mod.rs` | modify | `pub mod saved_query_picker;` |
| `src/view/name_prompt.rs` | modify | Save/Cancel rebuilt on `modal_button`; `ModalContent` impl |
| `src/window.rs` | modify | `MountedModal`/`mounted_modals`, derived count + trap, export + picker wiring, `pending_modal_focus` drain, `anchored_overlay` at 2 sites |
| `crates/dat0-i18n/src/strings/en.json` | modify | + `export.title` |
| `tests/modal_b2_nav.rs` | **create** | the B2 nav suite (+1 test binary) |
| `tests/modal_trap_nav.rs` | modify | 3 call sites gain a `cx` argument |

---

## Task 0: Hard gate — does focusing from `render` stick, and does one stop own a radio group?

Two mechanisms this slice cannot work without, neither previously exercised in
this codebase. Prove both before building on them.

**Files:**
- Create: `crates/dat0-app/tests/modal_b2_nav.rs`
- Modify: `crates/dat0-app/src/view/export_dialog.rs`
- Modify: `crates/dat0-app/src/window.rs`

**Interfaces produced:**
- `ExportDialog::new(cx: &mut Context<Self>) -> Self` (was `new()`)
- `ExportDialog::run_focus_handle(&self) -> FocusHandle`
- `WorkspaceShell::pending_modal_focus: bool` (private field)
- `WorkspaceShell::open_export_dialog_for_test(&mut self, cx)` (`a11y-capture` only)

- [ ] **Step 1: Scaffold the test binary**

Copy the harness block from `tests/modal_trap_nav.rs` lines 1-140 verbatim
(module docs adapted, `set_config_dir`, `build_empty_session`,
`open_shell_window`, `init_components`, `AsyncHarness`, `enter_async_harness`,
`focus_shell_neutrally`, `tab_until`). This per-binary copy is the established
convention — see `tests/support/mod.rs` for the rationale.

`init_components` **must** keep the `register_modal_keys` line:

```rust
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(dat0_app::overlay::register_modal_keys);
}
```

- [ ] **Step 2: Write the two gate probes**

```rust
/// GATE A — a modal opened with NO `&mut Window` (the real `open_export_dialog`
/// path: `view_actions::dispatch_export` reaches the shell from a bare `&mut
/// App`) must still end up focused, because with nothing focused gpui's
/// dispatch path is the window root alone and Tab is completely inert.
/// The mechanism under test is the render-drain: the open path sets a flag and
/// `WorkspaceShell::render` — which does hold a `Window` — focuses the first
/// stop.
#[gpui::test]
#[serial]
fn gate_a_render_drain_focuses_the_export_dialog(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);

    // Deliberately windowless — mirrors `dispatch_export`.
    vcx.update(|_w, app| shell.update(app, |ws, cx| ws.open_export_dialog_for_test(cx)));
    vcx.run_until_parked();

    let want = vcx
        .update(|_w, app| {
            shell
                .read(app)
                .export_dialog_entity_for_test()
                .map(|d| d.read(app).run_focus_handle())
        })
        .expect("dialog mounted");
    assert!(
        vcx.update(|window, _app| want.is_focused(window)),
        "the render-drain must move focus into the modal; without it Tab is inert"
    );
    drop(state);
}

/// GATE B — one dat0 `focus_stop` wrapping a `RadioGroup` whose children are
/// built with `.tab_stop(false)` is a SINGLE tab stop: Tab reaches the group
/// once and the individual radios never take focus of their own.
/// `RadioGroup::render` rewrites each child's id but leaves `tab_stop` alone
/// (gpui-component `radio.rs:333`), which is what makes this possible.
#[gpui::test]
#[serial]
fn gate_b_radio_group_is_one_tab_stop(cx: &mut TestAppContext) {
    // …same setup as gate A, then:
    vcx.update(|_w, app| shell.update(app, |ws, cx| ws.open_export_dialog_for_test(cx)));
    vcx.run_until_parked();

    // Focus starts on the format group (gate A). One Tab must leave the group
    // entirely rather than stepping to a second radio inside it.
    let format_fh = /* export_dialog_entity_for_test().read(app).format_focus_handle() */;
    assert!(vcx.update(|window, _app| format_fh.is_focused(window)));
    press_tab(vcx);
    vcx.run_until_parked();
    assert!(
        !vcx.update(|window, _app| format_fh.is_focused(window)),
        "Tab must leave the radio group, not walk between its radios"
    );
    drop(state);
}
```

- [ ] **Step 3: Run both — expect compile failure**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_b2_nav`
Expected: FAIL — `no method named open_export_dialog_for_test`,
`export_dialog_entity_for_test`, `run_focus_handle`, `format_focus_handle`.

- [ ] **Step 4: Minimal production change to satisfy the gates**

In `src/view/export_dialog.rs`:

```rust
pub struct ExportDialog {
    format_ix: usize,
    scope_ix: usize,
    format_focus: FocusHandle,
    scope_focus: FocusHandle,
    run_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl ExportDialog {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            format_ix: 0,
            scope_ix: 0,
            format_focus: cx.focus_handle(),
            scope_focus: cx.focus_handle(),
            run_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
        }
    }

    pub fn format_focus_handle(&self) -> FocusHandle { self.format_focus.clone() }
    pub fn run_focus_handle(&self) -> FocusHandle { self.run_focus.clone() }
}
```

**Delete `impl Default for ExportDialog`** (it cannot supply a `cx`). First run
`grep -rn "ExportDialog::default\|ExportDialog::new" crates/` and fix every hit
— expected: `window.rs:2891` only.

Wrap the format `RadioGroup` in a focus stop so gate B has something to grab
(the arrow handling arrives in Task 2):

```rust
let ring = cx.theme().d0().focus_ring;
let format_stop = div()
    .focus_stop("export-format-group", &self.format_focus, 0, ring, |_ev, _w, _app| {})
    .child(format_group);
```

`RadioGroup::children` must now be given explicit `Radio`s so `tab_stop(false)`
survives:

```rust
.children([
    Radio::new("csv").label(dat0_i18n::t("export.format.csv")).tab_stop(false),
    Radio::new("json").label(dat0_i18n::t("export.format.json")).tab_stop(false),
    Radio::new("parquet").label(dat0_i18n::t("export.format.parquet")).tab_stop(false),
])
```

(The ids are cosmetic — `RadioGroup::render` overwrites them with the child
index.)

In `src/window.rs`:

```rust
// field, next to `modal_restore_focus`
/// Set by a modal open path that has NO `&mut Window` (only
/// `open_export_dialog`, reached from `view_actions::dispatch_export` with a
/// bare `&mut App`). `render` drains it: it captures the restore target and
/// focuses the modal's first stop. Same shape as `SqlConsole::queue_load` —
/// enqueue windowless, drain in a render that holds a real `Window`.
/// LOAD-BEARING: with nothing focused, Tab is completely inert (B1).
pending_modal_focus: bool,
```

Initialise it to `false` in the constructor. In `open_export_dialog`, after
`self.export_dialog = Some(dialog);` add `self.pending_modal_focus = true;` and
change the construction to `cx.new(|cx| ExportDialog::new(cx))`.

At the TOP of `WorkspaceShell::render`, before any element is built:

```rust
// Drain a windowless modal open (see `pending_modal_focus`). Sequenced so the
// immutable borrow for the handle lookup ends before the field writes.
if self.pending_modal_focus {
    let first = self
        .export_dialog
        .as_ref()
        .map(|d| d.read(cx).run_focus_handle());
    if let Some(fh) = first {
        self.modal_restore_focus = window.focused(cx);
        window.focus(&fh);
        self.pending_modal_focus = false;
    }
}
```

(Task 3 generalises this to `mounted_modals().first()`; the gate only needs the
export dialog. Focusing `run_focus` here rather than `format_focus` is
deliberate — gate B then proves that Tab *leaves* a group it did not start in
only after Task 2 reorders it, so keep this line and revisit it in Task 3.)

Add the `a11y-capture` accessors at the bottom of `window.rs`, in the existing
`#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block:

```rust
/// Open the export dialog the way `dispatch_export` does — with NO `Window`.
pub fn open_export_dialog_for_test(&mut self, cx: &mut Context<Self>) {
    // The production guard returns early with no ViewModel; a nav test has no
    // file loaded, so build the entity directly. Everything the trap and the
    // keyboard path touch is identical.
    let dialog = cx.new(|cx| crate::view::export_dialog::ExportDialog::new(cx));
    let sub = cx.subscribe(&dialog, |ws: &mut Self, _d, ev: &crate::view::export_dialog::ExportEvent, cx| {
        ws.route_export_event(ev.clone(), cx);
    });
    self.export_dialog_sub = Some(sub);
    self.export_dialog = Some(dialog);
    self.pending_modal_focus = true;
    cx.notify();
}

pub fn export_dialog_entity_for_test(
    &self,
) -> Option<gpui::Entity<crate::view::export_dialog::ExportDialog>> {
    self.export_dialog.clone()
}
```

- [ ] **Step 5: Run the gates**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_b2_nav`
Expected: both PASS.

**STOP CLAUSE.** If gate A fails — focus set during `render` does not stick —
do **not** improvise. Stop and report. The pre-analysed fallback is to add a
focused-window accessor to `window_registry` (it already stores
`gpui_handle` per record, `window_registry.rs:245`) and have `dispatch_export`
go through `AnyWindowHandle::update` for a real `&mut Window`; that is a
different, larger change and is the owner's call, not the implementer's.

If gate B fails — Tab walks between radios — the fallback is per-radio focus
stops with hand-rolled radio visuals (brainstorm option B), which changes the
trap order to 7 stops and needs re-approval.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/modal_b2_nav.rs crates/dat0-app/src/view/export_dialog.rs crates/dat0-app/src/window.rs
git commit -s -m "test(theme): B2 T0 — gate the render-drain focus and single-stop radio group"
```

---

## Task 1: `overlay.rs` — `ModalContent`, `modal_button`, `anchored_overlay`

**Files:**
- Modify: `crates/dat0-app/src/overlay.rs`
- Modify: `crates/dat0-app/src/view/name_prompt.rs`
- Test: `crates/dat0-app/tests/modal_trap_nav.rs` (must stay green, unchanged)

**Interfaces produced:**
- `pub trait ModalContent { fn modal_title(&self, cx: &App) -> SharedString; fn modal_focus_order(&self, cx: &App) -> Vec<FocusHandle>; }`
- `pub enum ModalButton { Primary, Ghost }`
- `pub fn modal_button(id: &'static str, label: SharedString, fh: &FocusHandle, variant: ModalButton, cx: &App, on_activate: impl Fn(&mut Window, &mut App) + 'static + Clone) -> gpui::Stateful<gpui::Div>`
- `pub fn anchored_overlay(cx: &App) -> gpui::Div`

- [ ] **Step 1: Write the failing test**

Append to `src/overlay.rs`'s existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn modal_button_variants_are_distinct() {
    // Compile-time proof the enum exists with both arms; the visual difference
    // is exercised by the nav suite and the owed human glance.
    let a = super::ModalButton::Primary;
    let b = super::ModalButton::Ghost;
    assert_ne!(
        std::mem::discriminant(&a),
        std::mem::discriminant(&b)
    );
}
```

The real behavioural coverage for `modal_button` is Task 6's nav suite (a
button that is not a focus stop fails `tab_wraps_*`), and for
`anchored_overlay` it is `Elevation::Overlay`'s existing resolution tests in
`theme/tokens.rs`. Do not invent a window-free render test — `Styled::style()`
read-back cannot see `.focus_stop`'s handle wiring, which is the part that
matters.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p dat0-app --lib overlay::tests`
Expected: FAIL — `cannot find type ModalButton in module super`.

- [ ] **Step 3: Implement**

Add to `src/overlay.rs` (extend the existing `use` lines — `Div`, `Hsla`,
`Stateful` from `gpui`; `Sp`/`SpStyled`/`TextRole`/`TypoStyled` and
`Dat0Theme` from `crate::theme::tokens`; `FocusStopExt` from `crate::a11y`):

```rust
/// What a modal must tell the shell about itself. Implemented by every modal
/// body, so `window.rs` can mount, trap and count modals from ONE list instead
/// of three hand-maintained places (B1 shipped that hazard knowingly).
pub trait ModalContent {
    /// Accessible name of the `Dialog` node `modal_host` paints.
    fn modal_title(&self, cx: &App) -> SharedString;
    /// The modal's focus stops in VISUAL order — the trap's only source of
    /// truth, since gpui's `tab_index` is global rather than sibling-scoped.
    fn modal_focus_order(&self, cx: &App) -> Vec<FocusHandle>;
}

/// Visual weight of a modal button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalButton {
    /// The affirmative action — theme `primary` fill.
    Primary,
    /// The dismissive action — no fill, `foreground` text.
    Ghost,
}

/// A modal's action button: a dat0-owned focus stop, activated by Enter/Space
/// *and* click, styled from tokens.
///
/// Hand-rolled rather than `gpui_component::Button` because a `Button` builds
/// its focus handle with `window.use_keyed_state`, which is keyed by the GLOBAL
/// element-id path — the handle is unreachable from outside, and
/// `overlay::modal_trap` needs an owned `Vec<FocusHandle>`. `Button::render`
/// also calls `track_focus` on its own base after any builder chain, so a
/// chained `.track_focus(&ours)` is overwritten.
///
/// `Ghost` sets NO background rather than a transparent one: `transparent_black()`
/// is banned by `tests/style_lint.rs`.
pub fn modal_button(
    id: &'static str,
    label: SharedString,
    fh: &FocusHandle,
    variant: ModalButton,
    cx: &App,
    on_activate: impl Fn(&mut Window, &mut App) + 'static + Clone,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    let ring = theme.d0().focus_ring;
    let fg = match variant {
        ModalButton::Primary => theme.primary_foreground,
        ModalButton::Ghost => theme.foreground,
    };
    let primary_bg = theme.primary;
    let keyed = on_activate.clone();
    div()
        .id(id)
        .px_sp(Sp::S12)
        .py_sp(Sp::S4)
        .rounded(theme.radius)
        .text_role(TextRole::Body)
        .text_color(fg)
        .cursor_pointer()
        .map(|d| match variant {
            ModalButton::Primary => d.bg(primary_bg),
            ModalButton::Ghost => d,
        })
        .focus_stop(id, fh, 0, ring, move |_ev, window, app| keyed(window, app))
        .a11y(id, AccessRole::Button, label.to_string())
        .child(label)
        .on_click(move |_ev, window, app| on_activate(window, app))
}

/// A non-modal floating surface: elevation card + `occlude`, positioned by the
/// caller. No scrim and no trap — these overlays stay usable alongside the
/// shell, unlike `modal_host`.
///
/// `occlude` additionally stops a click on the overlay's own padding from
/// falling through to the grid underneath.
pub fn anchored_overlay(cx: &App) -> gpui::Div {
    div().elevation(Elevation::Overlay, cx.theme()).occlude()
}
```

Then move `NamePrompt`'s two methods onto the trait in
`src/view/name_prompt.rs` (keep the inherent ones as delegates so B1's call
sites and tests do not churn):

```rust
impl crate::overlay::ModalContent for NamePrompt {
    fn modal_title(&self, _cx: &gpui::App) -> SharedString {
        self.title.clone()
    }
    fn modal_focus_order(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        self.focus_order(cx)
    }
}
```

And rebuild its Save/Cancel on `modal_button` — **ids and a11y labels stay
byte-identical** (`name-prompt-ok` / "Save", `name-prompt-cancel` / "Cancel"),
which is what keeps `tests/modal_trap_nav.rs` green:

```rust
let entity_ok = cx.entity();
let ok_btn = crate::overlay::modal_button(
    "name-prompt-ok",
    SharedString::from("Save"),
    &ok_fh,
    crate::overlay::ModalButton::Primary,
    cx,
    move |_window, app| {
        entity_ok.update(app, |this, cx| {
            let v = this.value(cx);
            cx.emit(NamePromptEvent::Confirm(v));
        });
    },
);
let entity_cancel = cx.entity();
let cancel_btn = crate::overlay::modal_button(
    "name-prompt-cancel",
    SharedString::from("Cancel"),
    &cancel_fh,
    crate::overlay::ModalButton::Ghost,
    cx,
    move |_window, app| {
        entity_cancel.update(app, |_this, cx| cx.emit(NamePromptEvent::Cancel));
    },
);
```

Replace the two inline `div()` button blocks with `.child(ok_btn)` /
`.child(cancel_btn)`. The old blocks carried both `.focus_stop` and `.a11y` —
`modal_button` now supplies both, so **do not add a second `.a11y`** (A5's rule:
both helpers push a node).

- [ ] **Step 4: Run the tests**

```
cargo test -p dat0-app --lib overlay::tests
cargo test -p dat0-app --features a11y-capture --test modal_trap_nav
```
Expected: PASS. B1's suite is the real regression gate here — if `modal_button`
moved an id or label, `tab_until("Cancel")` panics.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/overlay.rs crates/dat0-app/src/view/name_prompt.rs
git commit -s -m "feat(theme): B2 T1 — ModalContent trait, modal_button, anchored_overlay"
```

---

## Task 2: `ExportDialog` — four owned stops, arrow selection, Escape

**Files:**
- Modify: `crates/dat0-app/src/view/export_dialog.rs`
- Modify: `crates/dat0-i18n/src/strings/en.json`
- Test: `crates/dat0-app/tests/modal_b2_nav.rs`

**Interfaces consumed:** `overlay::{ModalContent, ModalButton, modal_button}` (Task 1).
**Interfaces produced:**
- `ExportDialog::{format_focus_handle, scope_focus_handle, run_focus_handle, cancel_focus_handle}(&self) -> FocusHandle`
- `impl ModalContent for ExportDialog` — focus order `[format, scope, run, cancel]`
- `ExportDialog::{format_for_test, scope_for_test}` under `a11y-capture`

- [ ] **Step 1: Write the failing tests**

Append to `tests/modal_b2_nav.rs`:

```rust
/// Left/Right cycle the format radio group while it holds focus — the WAI-ARIA
/// radiogroup pattern: the group is one tab stop and arrows move the selection.
#[gpui::test]
#[serial]
fn arrows_change_the_export_format(cx: &mut TestAppContext) {
    // …standard setup + focus_shell_neutrally + open_export_dialog_for_test…
    let dialog = vcx
        .update(|_w, app| shell.read(app).export_dialog_entity_for_test())
        .expect("dialog mounted");
    vcx.update(|window, app| window.focus(&dialog.read(app).format_focus_handle()));
    vcx.run_until_parked();

    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).format_for_test()),
        dat0_engine::types::ExportFormat::Csv
    );
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).format_for_test()),
        dat0_engine::types::ExportFormat::Json,
        "Right moves to the next format"
    );
    vcx.simulate_keystrokes("left left");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| dialog.read(app).format_for_test()),
        dat0_engine::types::ExportFormat::Parquet,
        "Left wraps past the first entry to the last"
    );
    drop(state);
}

/// Enter on the Export stop emits the ARROW-SELECTED scope and format, not the
/// defaults — proves the keyboard path reaches the same state the mouse does.
#[gpui::test]
#[serial]
fn enter_on_export_emits_the_selected_scope_and_format(cx: &mut TestAppContext) {
    // …setup…
    let dialog = /* … */;
    let log: Rc<RefCell<Vec<ExportEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let log2 = log.clone();
    let sub = vcx.cx.update(|app| {
        app.subscribe(&dialog, move |_d, ev: &ExportEvent, _app| log2.borrow_mut().push(ev.clone()))
    });
    std::mem::forget(sub);
    vcx.run_until_parked();

    vcx.update(|window, app| window.focus(&dialog.read(app).format_focus_handle()));
    vcx.simulate_keystrokes("right");                     // → Json
    vcx.update(|window, app| window.focus(&dialog.read(app).scope_focus_handle()));
    vcx.simulate_keystrokes("down");                      // → FullTable
    vcx.update(|window, app| window.focus(&dialog.read(app).run_focus_handle()));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();

    assert!(
        log.borrow().iter().any(|e| matches!(
            e,
            ExportEvent::Export { scope: ExportScope::FullTable, format: ExportFormat::Json }
        )),
        "Enter on Export must carry the arrow-selected values, got {:?}",
        log.borrow()
    );
    drop(state);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_b2_nav`
Expected: FAIL — `format_for_test` / `scope_focus_handle` do not exist.

- [ ] **Step 3: Implement the render**

Replace `ExportDialog::render`'s body. Pure index helpers first (module-level,
unit-testable, no window):

```rust
/// Cycle an index within `len`, wrapping in both directions. Radio groups wrap
/// (WAI-ARIA); the list surfaces deliberately clamp instead
/// (`empty_state.rs:436-439`), because a 2-item group that dead-ends is worse
/// than one that cycles.
fn cycle_ix(cur: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (cur as isize + delta).rem_euclid(len as isize) as usize
}
```

with tests in the file's `#[cfg(test)] mod tests`:

```rust
#[test]
fn cycle_ix_wraps_both_ways() {
    assert_eq!(cycle_ix(0, 3, 1), 1);
    assert_eq!(cycle_ix(2, 3, 1), 0);
    assert_eq!(cycle_ix(0, 3, -1), 2);
    assert_eq!(cycle_ix(0, 0, 1), 0, "empty group cannot panic");
}
```

Render:

```rust
impl Render for ExportDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let format_ix = self.format_ix;
        let scope_ix = self.scope_ix;
        let ring = cx.theme().d0().focus_ring;

        let format_group = RadioGroup::horizontal("export-format")
            .children([
                Radio::new("csv").label(dat0_i18n::t("export.format.csv")).tab_stop(false),
                Radio::new("json").label(dat0_i18n::t("export.format.json")).tab_stop(false),
                Radio::new("parquet").label(dat0_i18n::t("export.format.parquet")).tab_stop(false),
            ])
            .selected_index(Some(format_ix))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.format_ix = *ix;
                cx.notify();
            }));

        // ONE tab stop for the whole group; Left/Right move the selection.
        // `focus_stop`'s Enter/Space activation is a deliberate no-op — on a
        // radiogroup the selection IS the state, and a second submit path from
        // inside a group would surprise. Chaining a second `on_key_down` after
        // `focus_stop` is the established shape (`empty_state.rs:451-452`).
        let format_stop = div()
            .focus_stop("export-format-group", &self.format_focus, 0, ring, |_ev, _w, _app| {})
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                let delta = match ev.keystroke.key.as_str() {
                    "left" => -1,
                    "right" => 1,
                    _ => return,
                };
                this.format_ix = cycle_ix(this.format_ix, Self::FORMATS.len(), delta);
                cx.notify();
            }))
            .a11y("export-format-group", AccessRole::Button, dat0_i18n::t("export.format"))
            .child(format_group);
```

The scope group is the same shape with `RadioGroup::vertical("export-scope")`,
two `Radio`s (`export.scope.current` / `export.scope.full`), id
`"export-scope-group"`, `&self.scope_focus`, and `"up" => -1 / "down" => 1`.

Buttons and assembly:

```rust
        let entity_run = cx.entity();
        let export_btn = crate::overlay::modal_button(
            "export-run",
            dat0_i18n::t("export.run").into(),
            &self.run_focus,
            crate::overlay::ModalButton::Primary,
            cx,
            move |_window, app| {
                entity_run.update(app, |this, cx| {
                    cx.emit(ExportEvent::Export { scope: this.scope(), format: this.format() });
                });
            },
        );
        let entity_cancel = cx.entity();
        let cancel_btn = crate::overlay::modal_button(
            "export-cancel",
            dat0_i18n::t("export.cancel").into(),
            &self.cancel_focus,
            crate::overlay::ModalButton::Ghost,
            cx,
            move |_window, app| {
                entity_cancel.update(app, |_this, cx| cx.emit(ExportEvent::Cancel));
            },
        );

        v_flex()
            .gap_sp(Sp::S12)
            .p_sp(Sp::S16)
            .min_w(gpui::px(320.))
            // Escape cancels from ANY stop. `register_modal_keys` binds
            // `escape` → `gpui_component::input::Escape` under `Dat0Modal`, so
            // this ancestor handler catches it; upstream binds `escape` only
            // under key context "Input".
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(ExportEvent::Cancel);
            }))
            .child(Label::new(dat0_i18n::t("export.format")))
            .child(format_stop)
            .child(Label::new(dat0_i18n::t("export.scope")))
            .child(scope_stop)
            .child(h_flex().gap_sp(Sp::S8).child(export_btn).child(cancel_btn))
    }
}
```

`ModalContent` + test accessors:

```rust
impl crate::overlay::ModalContent for ExportDialog {
    fn modal_title(&self, _cx: &gpui::App) -> SharedString {
        dat0_i18n::t("export.title").into()
    }
    fn modal_focus_order(&self, _cx: &gpui::App) -> Vec<FocusHandle> {
        vec![
            self.format_focus.clone(),
            self.scope_focus.clone(),
            self.run_focus.clone(),
            self.cancel_focus.clone(),
        ]
    }
}

#[cfg(feature = "a11y-capture")]
impl ExportDialog {
    pub fn format_for_test(&self) -> ExportFormat { self.format() }
    pub fn scope_for_test(&self) -> ExportScope { self.scope() }
}
```

plus `scope_focus_handle` / `cancel_focus_handle` next to the two added in
Task 0.

- [ ] **Step 4: Add the i18n key**

In `crates/dat0-i18n/src/strings/en.json`, next to the other `export.*` keys:

```json
  "export.title": "Export",
```

Verified absent today. Run `grep -c '"export.title"' crates/dat0-i18n/src/strings/en.json`
→ must print `1` after the edit (a duplicate key would silently overwrite).

- [ ] **Step 5: Run the tests**

```
cargo test -p dat0-app --lib export_dialog
cargo test -p dat0-app --features a11y-capture --test modal_b2_nav
cargo test -p dat0-app --test export_select_build
```
Expected: PASS. `export_select_build` must be untouched — `build_export` did not
change.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/export_dialog.rs crates/dat0-i18n/src/strings/en.json crates/dat0-app/tests/modal_b2_nav.rs
git commit -s -m "feat(theme): B2 T2 — export dialog gets four owned focus stops and arrow selection"
```

---

## Task 3: `window.rs` — one modal registry; export mounted through `modal_host`

**Files:**
- Modify: `crates/dat0-app/src/window.rs`
- Modify: `crates/dat0-app/tests/modal_trap_nav.rs` (3 call sites)
- Test: `crates/dat0-app/tests/modal_b2_nav.rs`

**Interfaces consumed:** `ModalContent` (Task 1), `impl ModalContent for ExportDialog` (Task 2).
**Interfaces produced:**
- `struct MountedModal { a11y_id: &'static str, title: SharedString, focus_order: Vec<FocusHandle>, content: AnyElement }`
- `WorkspaceShell::mounted_modals(&self, cx: &App) -> Vec<MountedModal>`
- `WorkspaceShell::open_modal_count(&self, cx: &App) -> usize` (signature change)
- `WorkspaceShell::open_modal_count_for_test(&self, cx: &App) -> usize` (signature change)

- [ ] **Step 1: Write the failing test**

Append to `tests/modal_b2_nav.rs`:

```rust
/// The export modal traps Tab: four stops, wrapping, never escaping into the
/// obscured shell. This is the WCAG 2.4.3 fix for the export dialog.
#[gpui::test]
#[serial]
fn export_modal_tab_cycles_four_stops(cx: &mut TestAppContext) {
    // …setup + focus_shell_neutrally + open_export_dialog_for_test…
    let dialog = /* … */;
    let (fmt, scope, run, cancel) = vcx.update(|_w, app| {
        let d = dialog.read(app);
        (d.format_focus_handle(), d.scope_focus_handle(), d.run_focus_handle(), d.cancel_focus_handle())
    });

    // The drain focuses the FIRST stop.
    assert!(vcx.update(|window, _app| fmt.is_focused(window)), "opens on the format group");
    for want in [&scope, &run, &cancel, &fmt] {
        press_tab(vcx);
        vcx.run_until_parked();
        assert!(
            vcx.update(|window, _app| want.is_focused(window)),
            "Tab must stay inside the modal and wrap"
        );
    }
    press_shift_tab(vcx);
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, _app| cancel.is_focused(window)),
        "Shift-Tab from the first stop wraps to the last"
    );
    drop(state);
}

/// Escape from a non-field stop emits exactly ONE Cancel. Two bindings match
/// while a modal is up; `on_action` handlers consume by default, but this is
/// the cell-editor double-fire class and is asserted, not assumed.
#[gpui::test]
#[serial]
fn escape_from_export_emits_exactly_one_cancel(cx: &mut TestAppContext) {
    // …setup, subscribe a log as in Task 2…
    vcx.update(|window, app| window.focus(&dialog.read(app).cancel_focus_handle()));
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(
        log.borrow().iter().filter(|e| matches!(e, ExportEvent::Cancel)).count(),
        1,
        "exactly one Cancel per Escape"
    );
    drop(state);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_b2_nav`
Expected: FAIL — Tab escapes the dialog (it is not in the trap's `or` chain yet)
and Escape is dead (the dialog is not inside `modal_host`, so no
`Dat0Modal` context is installed while it is the only modal open).

- [ ] **Step 3: Build the registry**

Near `open_modal_count`, replace it and add:

```rust
/// One mounted modal, everything `render`, the trap and the count need.
pub(crate) struct MountedModal {
    a11y_id: &'static str,
    title: gpui::SharedString,
    focus_order: Vec<FocusHandle>,
    content: gpui::AnyElement,
}

/// Push `slot`'s modal, if mounted. Generic so each call monomorphizes for its
/// concrete entity type — no `dyn`, no boxing at the slot level.
/// `into_any_element` wraps the entity; it does NOT render it.
fn push_modal<T: crate::overlay::ModalContent + Render>(
    out: &mut Vec<MountedModal>,
    a11y_id: &'static str,
    slot: &Option<Entity<T>>,
    cx: &App,
) {
    if let Some(entity) = slot {
        let view = entity.read(cx);
        out.push(MountedModal {
            a11y_id,
            title: view.modal_title(cx),
            focus_order: view.modal_focus_order(cx),
            content: entity.clone().into_any_element(),
        });
    }
}

impl WorkspaceShell {
    /// EVERY mounted modal, in priority order. The single source of truth: the
    /// render mount, `overlay::modal_trap`'s focus order and
    /// [`open_modal_count`](Self::open_modal_count) all derive from this list.
    ///
    /// B1 kept three hand-maintained places in sync instead (an `or` chain, a
    /// count, and the mount), so a new modal was styled but silently UNTRAPPED
    /// unless all three were edited. Adding a modal is now one line here.
    fn mounted_modals(&self, cx: &App) -> Vec<MountedModal> {
        let mut v = Vec::new();
        push_modal(&mut v, "name-prompt-modal", &self.name_prompt, cx);
        push_modal(&mut v, "md-token-prompt-modal", &self.md_token_prompt, cx);
        push_modal(&mut v, "ai-entry-prompt-modal", &self.ai_entry_prompt, cx);
        push_modal(&mut v, "export-modal", &self.export_dialog, cx);
        v
    }

    /// How many modals are mounted. The single-modal invariant is that this is
    /// never > 1; `render` traps only `mounted_modals().first()`, so a second
    /// one would be the one NOT trapped.
    pub(crate) fn open_modal_count(&self, cx: &App) -> usize {
        self.mounted_modals(cx).len()
    }
}
```

(The picker joins the list in Task 4.)

Update the three `debug_assert!(self.open_modal_count() <= 1, ...)` call sites
(`window.rs:4861`, `:5297`, `:5896`) to `self.open_modal_count(cx)`. Update
`open_modal_count_for_test` to take and forward `cx: &App`.

In `render`, replace the export/name/md/ai overlay blocks and the
`modal_focus_order` `or` chain with:

```rust
// B2: mount, trap and count all derive from ONE list. At most one modal is
// ever open (`open_modal_count`), so `first()` is the live one.
let mut modals = self.mounted_modals(cx);
let modal = (!modals.is_empty()).then(|| modals.remove(0));
let modal_focus_order: Option<Vec<FocusHandle>> =
    modal.as_ref().map(|m| m.focus_order.clone());
let modal_overlay: Option<gpui::AnyElement> = modal.map(|m| {
    crate::overlay::modal_host(m.a11y_id, m.title, m.content, cx).into_any_element()
});
```

Delete the four `let *_overlay` blocks for export/name/md/ai (window.rs
~6458-6524) and replace the five `.children(...)` lines at 7015-7019 with one
`.children(modal_overlay)` (keeping `.children(popover_overlay)` and
`.children(editor_overlay)` above it).

Generalise the Task-0 drain to the registry:

```rust
if self.pending_modal_focus {
    let first = self
        .mounted_modals(cx)
        .first()
        .and_then(|m| m.focus_order.first().cloned());
    if let Some(fh) = first {
        self.modal_restore_focus = window.focused(cx);
        window.focus(&fh);
        self.pending_modal_focus = false;
    }
}
```

**Focus restore on dismiss — the export dialog cannot use `subscribe_in`.**
`cx.subscribe_in` requires a `&mut Window` at SUBSCRIPTION time, and
`open_export_dialog` has none (§1.4 of the design — that is the whole reason the
drain exists). So the export subscription stays `cx.subscribe`, and the restore
rides the same render-drain mechanism in the opposite direction. Add a second
field beside `pending_modal_focus`:

```rust
/// Set by a dismiss path that has no `&mut Window` (the export dialog's
/// `cx.subscribe` handler). `render` drains it into `restore_modal_focus`.
/// The mirror image of `pending_modal_focus`.
pending_modal_restore: bool,
```

drained in `render` immediately after the focus drain:

```rust
if self.pending_modal_restore {
    self.restore_modal_focus(window);
    self.pending_modal_restore = false;
}
```

Both places that clear `export_dialog` — `route_export_event`'s `Cancel` arm
(`window.rs:2917-2921`) and `run_export`'s no-base-table early return
(`window.rs:4362-4366`) — set `self.pending_modal_restore = true` alongside.
Grep for `self.export_dialog = None` to be sure you have every one; expected
count is 3 (Cancel, the early return, and the post-COPY dismissal inside
`run_export`).

The three `NamePrompt` modals and the picker keep their `subscribe_in` +
direct `restore_modal_focus(window)` — they all have a `Window` at open time.

- [ ] **Step 4: Fix the three B1 test call sites**

In `tests/modal_trap_nav.rs:498, 504, 511`:

```rust
vcx.update(|_w, app| shell.read(app).open_modal_count_for_test(app)),
```

Two immutable borrows of `app` — fine.

- [ ] **Step 5: Run the tests**

```
cargo test -p dat0-app --features a11y-capture --test modal_b2_nav
cargo test -p dat0-app --features a11y-capture --test modal_trap_nav
```
Expected: both PASS. `modal_trap_nav` proves the registry did not regress the
three `NamePrompt` modals.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/modal_trap_nav.rs crates/dat0-app/tests/modal_b2_nav.rs
git commit -s -m "feat(theme): B2 T3 — one modal registry; export dialog on modal_host"
```

---

## Task 4: `SavedQueryPicker` — modal listbox

**Files:**
- Create: `crates/dat0-app/src/view/saved_query_picker.rs`
- Modify: `crates/dat0-app/src/view/mod.rs`, `src/view/query_library.rs`, `src/window.rs`
- Test: `crates/dat0-app/tests/modal_b2_nav.rs`

**Interfaces produced:**
- `pub struct SavedQueryPicker`, `pub enum SavedQueryPickerEvent { Pick(String), Delete(uuid::Uuid), Cancel }`
- `SavedQueryPicker::new(session: Arc<Mutex<Session>>, cx: &mut Context<Self>) -> Self`
- `SavedQueryPicker::list_focus_handle(&self) -> FocusHandle`, `close_focus_handle`
- `WorkspaceShell::show_saved_picker(&mut self, window: &mut Window, cx: &mut Context<Self>)` (signature change)

- [ ] **Step 1: Write the failing test**

```rust
/// The picker is a listbox: ONE container tab stop, arrows move the active row,
/// Enter loads it. Never per-row focus handles — the pattern proven by recents
/// (`empty_state.rs:448`) and the shape B4's command palette needs.
#[gpui::test]
#[serial]
fn picker_arrows_move_active_and_enter_picks(cx: &mut TestAppContext) {
    // …setup… then seed two saved queries on the session BEFORE opening:
    //   session.lock().save_named_query(...) — use the same call
    //   `WorkspaceShell::save_named_query` makes, see window.rs:4476.
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.show_saved_picker(window, cx)));
    vcx.run_until_parked();

    let picker = vcx
        .update(|_w, app| shell.read(app).saved_picker_entity_for_test())
        .expect("picker mounted");
    let list = vcx.update(|_w, app| picker.read(app).list_focus_handle());
    assert!(vcx.update(|window, _app| list.is_focused(window)), "opens focused on the list");
    assert_eq!(vcx.update(|_w, app| picker.read(app).active_for_test()), 0);

    let log: Rc<RefCell<Vec<SavedQueryPickerEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let log2 = log.clone();
    let sub = vcx.cx.update(|app| {
        app.subscribe(&picker, move |_p, ev: &SavedQueryPickerEvent, _app| {
            log2.borrow_mut().push(ev.clone())
        })
    });
    std::mem::forget(sub);
    vcx.run_until_parked();

    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(vcx.update(|_w, app| picker.read(app).active_for_test()), 1);
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_w, app| picker.read(app).active_for_test()),
        1,
        "Down clamps at the last row (lists clamp; only radio groups wrap)"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow().iter().any(|e| matches!(e, SavedQueryPickerEvent::Pick(sql) if sql == "select 2")),
        "Enter picks the ACTIVE row's SQL, got {:?}",
        log.borrow()
    );
    drop(state);
}

/// Tab cycles the picker's two stops and wraps — the trap covers it exactly
/// like the export modal, because both derive from `mounted_modals`.
#[gpui::test]
#[serial]
fn picker_tab_cycles_list_and_close(cx: &mut TestAppContext) { /* list → close → list */ }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p dat0-app --features a11y-capture --test modal_b2_nav`
Expected: FAIL — `saved_query_picker` module does not exist.

- [ ] **Step 3: Create the entity**

`crates/dat0-app/src/view/saved_query_picker.rs`:

```rust
//! Saved-query picker modal (UI redesign B2).
//!
//! Replaces the window-level `saved_picker_open` flag + the free
//! `query_library::render_saved_picker`, which was mouse-only, untested, and
//! rendered as a transparent bordered box in the top-right corner.
//!
//! This is the LISTBOX pattern — ONE container `focus_stop` plus an active
//! index, never per-row focus handles — proven by the recents list
//! (`empty_state.rs:448`) and the catalog tree. B4's command palette is the
//! same shape, so this is its precedent rather than its guess.
//!
//! The picker READS the session live (`saved_queries()` on every render), so a
//! delete routed through the shell shrinks the list on the next frame. It never
//! mutates: `WorkspaceShell` owns `delete_named_query`.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FocusHandle, ParentElement, SharedString, Styled, Window, div};
use gpui_component::ActiveTheme as _;
use gpui_component::input::Escape;
use parking_lot::Mutex;

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::session::Session;
use crate::session::queries::SavedQuery;
use crate::theme::tokens::{Dat0Theme as _, Sp, SpStyled as _};

#[derive(Debug, Clone)]
pub enum SavedQueryPickerEvent {
    Pick(String),
    Delete(uuid::Uuid),
    Cancel,
}

pub struct SavedQueryPicker {
    session: Arc<Mutex<Session>>,
    active: usize,
    list_focus: FocusHandle,
    close_focus: FocusHandle,
}

impl SavedQueryPicker {
    pub fn new(session: Arc<Mutex<Session>>, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            active: 0,
            list_focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
        }
    }

    pub fn list_focus_handle(&self) -> FocusHandle {
        self.list_focus.clone()
    }
    pub fn close_focus_handle(&self) -> FocusHandle {
        self.close_focus.clone()
    }

    /// Live read — a delete routed through the shell shrinks this next frame.
    fn rows(&self) -> Vec<SavedQuery> {
        self.session.lock().saved_queries().to_vec()
    }
}

#[cfg(feature = "a11y-capture")]
impl SavedQueryPicker {
    pub fn active_for_test(&self) -> usize {
        self.active
    }
}

impl EventEmitter<SavedQueryPickerEvent> for SavedQueryPicker {}

impl crate::overlay::ModalContent for SavedQueryPicker {
    fn modal_title(&self, _cx: &gpui::App) -> SharedString {
        dat0_i18n::t("sql.load_query").into()
    }
    fn modal_focus_order(&self, _cx: &gpui::App) -> Vec<FocusHandle> {
        vec![self.list_focus.clone(), self.close_focus.clone()]
    }
}

impl Render for SavedQueryPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows();
        let len = rows.len();
        // Clamp: a delete can leave `active` past the end.
        let active = self.active.min(len.saturating_sub(1));
        self.active = active;
        let ring = cx.theme().d0().focus_ring;

        // Arrows CLAMP (recents semantics). Delete/Backspace removes the active
        // row; the shell performs the mutation and re-notifies us.
        let arrows = cx.listener(move |this, ev: &gpui::KeyDownEvent, _window, cx| {
            match ev.keystroke.key.as_str() {
                "down" => this.active = (this.active + 1).min(len.saturating_sub(1)),
                "up" => this.active = this.active.saturating_sub(1),
                "delete" | "backspace" => {
                    if let Some(q) = this.rows().get(this.active) {
                        cx.emit(SavedQueryPickerEvent::Delete(q.id));
                    }
                }
                _ => return,
            }
            cx.notify();
        });
        let activate = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            if let Some(q) = this.rows().get(this.active) {
                cx.emit(SavedQueryPickerEvent::Pick(q.sql.clone()));
            }
        });

        let close_btn = crate::overlay::modal_button(
            "sql-saved-close",
            dat0_i18n::t("common.close").into(),
            &self.close_focus,
            crate::overlay::ModalButton::Ghost,
            cx,
            {
                let entity = cx.entity();
                move |_window, app| {
                    entity.update(app, |_this, cx| cx.emit(SavedQueryPickerEvent::Cancel));
                }
            },
        );

        let mut list = div()
            .flex()
            .flex_col()
            .gap_sp(Sp::S2)
            .p_sp(Sp::S8)
            .focus_stop("sql-saved-list", &self.list_focus, 0, ring, activate)
            .on_key_down(arrows)
            .a11y("sql-saved-list", AccessRole::Button, dat0_i18n::t("sql.load_query"));

        for (i, q) in rows.into_iter().enumerate() {
            let sql = q.sql.clone();
            let id = q.id;
            let entity_pick = cx.entity();
            let entity_del = cx.entity();
            let mut row = div()
                .id(("saved-row", i))
                .flex()
                .flex_row()
                .justify_between()
                .gap_sp(Sp::S8)
                .px_sp(Sp::S8)
                .py_sp(Sp::S4)
                .cursor_pointer()
                .child(SharedString::from(q.name))
                .child(
                    div()
                        .id(("saved-del", i))
                        .cursor_pointer()
                        .a11y_label(AccessRole::Label, dat0_i18n::t("common.close"))
                        .child(gpui_component::Icon::new(gpui_component::IconName::Close))
                        .on_click(move |_ev, _w, app| {
                            entity_del.update(app, |_t, cx| {
                                cx.emit(SavedQueryPickerEvent::Delete(id))
                            });
                        }),
                )
                .on_click(move |_ev, _w, app| {
                    entity_pick.update(app, |_t, cx| {
                        cx.emit(SavedQueryPickerEvent::Pick(sql.clone()))
                    });
                });
            if i == active {
                row = row.border_1().border_color(ring);
            }
            list = list.child(row);
        }

        div()
            .flex()
            .flex_col()
            .min_w(gpui::px(420.))
            .max_h(gpui::px(320.))
            .overflow_hidden()
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(SavedQueryPickerEvent::Cancel);
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_sp(Sp::S8)
                    .py_sp(Sp::S4)
                    .child(dat0_i18n::t("sql.load_query"))
                    .child(close_btn),
            )
            .child(list)
    }
}
```

⚠ The row's `.child(Icon)` inside a `.on_click` row means a click on ✕ also
hits the row's own `on_click`. That is today's behaviour too (nested
`div().id(..)` with its own handler wins for the inner hit; the outer fires for
the rest of the row). Do not "fix" it here.

- [ ] **Step 4: Wire it into `window.rs`**

- `pub mod saved_query_picker;` in `src/view/mod.rs`.
- Replace the `saved_picker_open: bool` field with
  `saved_picker: Option<Entity<crate::view::saved_query_picker::SavedQueryPicker>>`
  and `saved_picker_sub: Option<Subscription>`; update the constructor.
- Add the fifth `push_modal` line to `mounted_modals`:
  `push_modal(&mut v, "saved-picker-modal", &self.saved_picker, cx);`
- Rewrite the opener:

```rust
pub(crate) fn show_saved_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    use crate::view::saved_query_picker::{SavedQueryPicker, SavedQueryPickerEvent};
    self.modal_restore_focus = window.focused(cx);
    let session = self.session.clone();
    let picker = cx.new(|cx| SavedQueryPicker::new(session, cx));
    let sub = cx.subscribe_in(
        &picker,
        window,
        |ws: &mut Self, _p, ev: &SavedQueryPickerEvent, window, cx| {
            ws.on_saved_picker_event(ev.clone(), window, cx);
        },
    );
    window.focus(&picker.read(cx).list_focus_handle());
    self.saved_picker_sub = Some(sub);
    self.saved_picker = Some(picker);
    debug_assert!(
        self.open_modal_count(cx) <= 1,
        "two modals mounted at once ({})",
        self.open_modal_count(cx)
    );
    cx.notify();
}

fn on_saved_picker_event(
    &mut self,
    ev: crate::view::saved_query_picker::SavedQueryPickerEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
) {
    use crate::view::saved_query_picker::SavedQueryPickerEvent as E;
    match ev {
        E::Pick(sql) => {
            // Windowless load: the console drains `queue_load` in its own
            // render, which holds the `&mut Window` `load_into_new_tab` needs.
            if let Some(console) = self.sql_console.clone() {
                console.update(cx, |c, cx| c.queue_load(sql, cx));
            }
            self.dismiss_saved_picker(window, cx);
        }
        E::Delete(id) => {
            self.delete_named_query(id, cx);
            // The picker reads the session live; re-notify so its next render
            // re-runs the read and the row disappears.
            if let Some(p) = self.saved_picker.clone() {
                p.update(cx, |_p, cx| cx.notify());
            }
            cx.notify();
        }
        E::Cancel => self.dismiss_saved_picker(window, cx),
    }
}

fn dismiss_saved_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.saved_picker = None;
    self.saved_picker_sub = None;
    self.restore_modal_focus(window);
    cx.notify();
}
```

- Update the `ShowSaved` console-event arm (`window.rs:4053`) to
  `self.show_saved_picker(window, cx);` — `window` is already in scope there
  (the `SaveQuery` arm two cases above uses it).
- Delete the whole `saved_picker_overlay` block (window.rs ~6526-6606) and its
  `.children(saved_picker_overlay)` line.
- Delete `render_saved_picker` from `src/view/query_library.rs` **and** the
  second bullet of that file's module doc describing it. Keep
  `render_history_list` and `first_line`.
- Add the `a11y-capture` accessor:

```rust
pub fn saved_picker_entity_for_test(
    &self,
) -> Option<gpui::Entity<crate::view::saved_query_picker::SavedQueryPicker>> {
    self.saved_picker.clone()
}
```

- [ ] **Step 5: Run the tests**

```
cargo test -p dat0-app --features a11y-capture --test modal_b2_nav
cargo test -p dat0-app --features a11y-capture --test sql_console_nav
cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav
```
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/saved_query_picker.rs crates/dat0-app/src/view/mod.rs crates/dat0-app/src/view/query_library.rs crates/dat0-app/src/window.rs crates/dat0-app/tests/modal_b2_nav.rs
git commit -s -m "feat(theme): B2 T4 — saved-query picker as a modal listbox entity"
```

---

## Task 5: `anchored_overlay` on the filter popover and cell editor

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (two mount sites)

- [ ] **Step 1: Apply it**

`window.rs:6436` (filter popover) and `:6449` (cell editor):

```rust
// Funnel-click filter popover overlay (T0 / PD-016). B2 gives it the shared
// `anchored_overlay` surface — before this it painted no background at all.
// Precise anchoring under the clicked funnel icon is still open.
let popover_overlay: Option<gpui::AnyElement> = self.active_popover.as_ref().map(|p| {
    crate::overlay::anchored_overlay(cx)
        .absolute()
        .top_8()
        .right_4()
        .child(p.clone())
        .into_any_element()
});

// Inline cell-editor overlay (T6). Same treatment: it rendered as a bare
// transparent `h_flex` over the grid. Anchoring it over the active cell is
// still open.
let editor_overlay: Option<gpui::AnyElement> = self.cell_editor.as_ref().map(|e| {
    crate::overlay::anchored_overlay(cx)
        .absolute()
        .top_8()
        .left_4()
        .child(e.clone())
        .into_any_element()
});
```

- [ ] **Step 2: Run the affected suites**

```
cargo test -p dat0-app --features a11y-capture --test cell_editor_nav
cargo test -p dat0-app --features a11y-capture --test cell_editor_smoke
cargo test -p dat0-app --features a11y-capture --test filter_popover_entity_smoke
cargo test -p dat0-app --features a11y-capture --test keyboard_nav
```
Expected: PASS. These assert labels and focus, not pixels, so an added surface
is invisible to them — a failure here means `occlude` changed hit-testing and
must be understood, not worked around.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs
git commit -s -m "feat(theme): B2 T5 — filter popover and cell editor get the anchored_overlay surface"
```

---

## Task 6: Non-vacuity, structural invariant, and the controller gate

**Files:**
- Modify: `crates/dat0-app/tests/modal_b2_nav.rs`

- [ ] **Step 1: Add the structural test**

```rust
/// `open_modal_count` derives from `mounted_modals`, so it cannot drift from
/// what `render` mounts and traps — the B1 hazard this slice removes.
#[gpui::test]
#[serial]
fn modal_count_tracks_the_mounted_set(cx: &mut TestAppContext) {
    // …setup…
    assert_eq!(vcx.update(|_w, app| shell.read(app).open_modal_count_for_test(app)), 0);
    vcx.update(|_w, app| shell.update(app, |ws, cx| ws.open_export_dialog_for_test(cx)));
    vcx.run_until_parked();
    assert_eq!(vcx.update(|_w, app| shell.read(app).open_modal_count_for_test(app)), 1);
    // Escape dismisses → back to 0.
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(vcx.update(|_w, app| shell.read(app).open_modal_count_for_test(app)), 0);
    drop(state);
}
```

- [ ] **Step 2: Prove non-vacuity (perturb, observe red, revert)**

Run each perturbation, confirm the named test FAILS, then revert.
**After every revert run `touch` on the file** — a `mv`-style revert
backwards-dates it and cargo silently reuses the stale binary (A6's trap).

| Perturbation | Must turn red |
|---|---|
| Drop `run_focus` from `ExportDialog::modal_focus_order` | `export_modal_tab_cycles_four_stops` |
| Change `cycle_ix`'s `rem_euclid` to `min(len-1)` | `arrows_change_the_export_format` (the wrap assertion) |
| Remove `self.pending_modal_focus = true` from `open_export_dialog_for_test` | `gate_a_render_drain_focuses_the_export_dialog` |
| Make the picker's `activate` emit `Pick(rows[0].sql)` instead of the active row | `picker_arrows_move_active_and_enter_picks` |

Record the result of each in the commit message. A red-first step that only ever
proved an `unresolved import` proves nothing (A5's lesson).

- [ ] **Step 3: Controller gate — the full local sweep**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app 2>&1 | tee /tmp/b2-plain.txt
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b2-a11y.txt
cargo test -p dat0-app --features a11y-capture,gallery 2>&1 | tee /tmp/b2-gallery.txt
grep -c "test result: ok" /tmp/b2-a11y.txt      # count from a FILE
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --test gallery_smoke --features gallery
```

⚠ **Never pipe a cargo test count through `head`** — `head` SIGPIPEs cargo
mid-write and truncates the output (A6 counted 51 binaries instead of 109).
Redirect to a file and count there.

Expected: `test result: ok` on every binary; style_lint 4/4 with `ALLOW` still
`[("window.rs", 1)]`; binary count = B1's 110 + 1 (the new nav suite).

Also confirm no i18n drift:

```bash
./scripts/i18n-check.sh          # warn-only, read the output
grep -c '"export.title"' crates/dat0-i18n/src/strings/en.json    # → 1
```

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/modal_b2_nav.rs
git commit -s -F - <<'EOF'
test(theme): B2 T6 — modal-count invariant and non-vacuity proofs

Adds the structural test that open_modal_count tracks the mounted set, and
records the four perturbations run to prove the suite is not vacuous.

<record each perturbation and the test that went red>
EOF
```

---

## Self-review

**Spec coverage** — design § → task:

| Design section | Task |
|---|---|
| §2.1 `ModalContent` + `mounted_modals` | T1 (trait), T3 (collector, count, trap) |
| §2.2 export dialog, 4 stops, radiogroups, `modal_button`, Escape, `export.title` | T0 (handles + gate), T1 (`modal_button`), T2 (rest) |
| §2.3 `SavedQueryPicker` | T4 |
| §2.4 `anchored_overlay` | T1 (helper), T5 (call sites) |
| §2.5 open/dismiss plumbing, render-drain | T0 (drain), T3 (generalised + restore), T4 (picker opener) |
| §3 test list, items 1-14 | T0 (1), T2 (4, 5), T3 (2, 3, 6), T4 (7-11), T6 (13, 14) |
| §4 gates | T6 |

**Gap found and closed during review:** design §3 item 12 ("Escape closes the
picker and restores focus to where it was") had no task step. It is covered by
T4's `dismiss_saved_picker` → `restore_modal_focus`, but nothing asserted it.
Add to T4 Step 1:

```rust
/// Escape closes the picker AND hands focus back to where it came from.
#[gpui::test]
#[serial]
fn picker_escape_restores_focus(cx: &mut TestAppContext) {
    // …setup, focus_shell_neutrally, remember `window.focused(app)`…
    let before = vcx.update(|window, app| window.focused(app)).expect("something focused");
    vcx.update(|window, app| shell.update(app, |ws, cx| ws.show_saved_picker(window, cx)));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, _app| before.is_focused(window)),
        "dismissing must restore the pre-modal focus"
    );
    drop(state);
}
```

**Type consistency:** `modal_title`/`modal_focus_order` are used with those exact
names in T1, T2, T3, T4. `open_modal_count(cx)` takes `&App` everywhere after
T3. `run_focus_handle`/`format_focus_handle` are introduced in T0 and reused
verbatim in T2 and T3. `SavedQueryPickerEvent::{Pick, Delete, Cancel}` match
between T4's entity and its window routing.

**Placeholder scan:** the two `// …setup…` elisions in T2/T3/T4 test bodies
refer to the *literal* block written out in full in T0 Step 2 (tempdir,
`set_config_dir`, `init_components`, `enter_async_harness`,
`build_empty_session`, `open_shell_window`, `run_until_parked`,
`focus_shell_neutrally`). Copy it verbatim; every test in `modal_trap_nav.rs`
repeats it the same way.
