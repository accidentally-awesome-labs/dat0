# UAT Slice 6 — Keyboard-nav / focus reachability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reusable headless focus oracle (name the Tab-focused element by label) and a real production `focus_stop()` a11y fix, so P10b §10.1–10.6 keyboard reachability is automated AND actually passes on the previously-unreachable Home hero buttons + Settings DIY toggles.

**Architecture:** A test-only `FocusId→label` side-map (thread-local, mirrors the existing `.a11y` FRAME collector) lets `window.focused()` resolve to a label. A production `focus_stop(id, &FocusHandle, tab_index, on_activate)` element helper chains gpui's `tab_index` + `track_focus` + `on_key_down`(Enter/Space) + `.focus()` ring — this ships in release (unlike `.a11y`, which is an identity no-op). Stable `FocusHandle`s live on the persistent entities (`WorkspaceShell`, `SettingsPanel`), never on the transient `EmptyState`.

**Tech Stack:** Rust, gpui `=0.2.2`, gpui-component `0.5.1`, kittest `0.3.0` / accesskit `0.21.1` (test-only, `a11y-capture` feature), no new deps.

## Global Constraints

- gpui pinned `=0.2.2`, gpui-component `0.5.1` — **NO fork, NO version bump** (D-015 stays open; still no OS AccessKit adapter).
- **Zero new dependencies.** `Cargo.lock` / `Cargo.toml` / `NOTICE` unchanged after the slice.
- The test-capture side-map is gated by the `a11y-capture` feature (auto-on for integration tests via the self-dev-dep). The **production focus wiring (`focus_stop`) ships UNCONDITIONALLY** — it is a real a11y fix, NOT an identity no-op.
- Helper name is **`focus_stop`**, NOT `focusable` — gpui already defines `StatefulInteractiveElement::focusable(self)`; a same-named helper would make method resolution ambiguous.
- Focus ring = `.focus(|s| s.border_2().border_color(gpui::rgb(0x3b82f6)))` — reuses the grid ring hex (`grid/mod.rs:566`). **Ring PIXELS + WCAG contrast stay human (Gap 1)** — tests assert reachability only, never ring pixels.
- Every `focus_stop(id, …)` element MUST also carry a matching `.a11y(id, AccessRole::Button, label)` with the **same `&'static str` id** — that `.a11y` node is the label the oracle joins to. A `focus_stop` with no `.a11y` twin → `focused_label()` returns `None` (the T0 spike guards this).
- Recents-list rows (dynamic `hero-recent-{i}` ids) are **OUT of scope** (dynamic-list nav joins the deferred Catalog/AI cluster). Only fixed-id hero buttons get `focus_stop`.
- Run `cargo fmt --all` before EVERY commit (Slice-5 CI fmt-gate lesson — plan code is not pre-formatted).
- DCO: `git commit -s` OR a literal `Signed-off-by` trailer, **not both**; end the body with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- **Implementer runs ONLY the focused test synchronously**: `cargo test -p dat0-app --test keyboard_nav`. The controller runs the workspace gate (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`) — anti-loop lesson (never background `cargo test --workspace` in an implementer).
- Prefer chaining seams onto EXISTING elements; the controller READS the diff for gratuitous new `div`s (a naive `grep '+.*div()'` false-alarms on rustfmt rewraps).

---

### Task 0: Focus oracle + `focus_stop` helper + one hero button (HARD GATE)

The load-bearing spike. It builds the entire reusable core (side-map, `focus_stop`, `focused_label`, tab combinators), wires exactly ONE hero button (`hero-take-tour`) end-to-end including the `WorkspaceShell` stable-handle plumbing, and proves all six spike criteria. **If any criterion fails → STOP, report, re-scope.** No breadth is built until this gate is green (Slice-3 lesson: spike every asserted surface's mechanism first).

**Files:**
- Modify: `crates/dat0-app/src/a11y/mod.rs` (side-map, `focus_stop`, `focused_label`, extend `reset`)
- Modify: `crates/dat0-app/tests/support/mod.rs` (extend `A11ySnapshot::capture`, add tab combinators)
- Modify: `crates/dat0-app/src/window.rs` (`WorkspaceShell.hero_focus` map + accessor + build `HeroHandles` in `render`)
- Modify: `crates/dat0-app/src/empty_state.rs` (`HeroHandles` struct; `render` takes it; wire `hero-take-tour`)
- Create: `crates/dat0-app/tests/keyboard_nav.rs` (mount helpers + T0 spike test)

**Interfaces:**
- Produces (a11y): `pub trait FocusStopExt { fn focus_stop(self, id: &'static str, fh: &FocusHandle, tab_index: isize, on_activate: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> Self }`; `#[cfg(a11y-capture)] pub fn focused_label(window: &Window) -> Option<String>`; `reset()` now clears the focus side-map too.
- Produces (support): `A11ySnapshot.focused: Option<String>` + `fn focused_label(&self) -> Option<&str>`; `pub fn press_tab(cx: &mut VisualTestContext)`, `pub fn press_shift_tab(cx: &mut VisualTestContext)`.
- Produces (empty_state): `pub struct HeroHandles { pub map: HashMap<&'static str, FocusHandle> }`; `EmptyState::render(&self, hero: &HeroHandles, cx)`.
- Produces (window): `WorkspaceShell::hero_focus_handle(&mut self, id: &'static str, cx: &mut App) -> FocusHandle`.

- [ ] **Step 1: Add the focus oracle side-map + `focus_stop` to `a11y/mod.rs`**

At the **module root** (unconditional — compiles in both feature states), above the `#[cfg(feature = "a11y-capture")] mod capture` block:

```rust
use gpui::{App, FocusHandle, InteractiveElement, KeyDownEvent, Styled as _, Window};

/// Focus-ring hue — matches the grid active-cell ring (`grid/mod.rs:566`).
const FOCUS_RING: u32 = 0x3b82f6;

/// Production a11y: turn an interactive `div` into a real keyboard control —
/// a tab stop that takes focus, activates on Enter/Space, and paints a focus
/// ring. Ships in release (this is a genuine a11y fix, not a test no-op). Under
/// `a11y-capture` it also records `fh → id` into the oracle side-map so a
/// headless test can name the focused element (see [`focused_label`]).
///
/// Named `focus_stop` (not `focusable`) to avoid clashing with gpui's
/// `StatefulInteractiveElement::focusable(self)`.
pub trait FocusStopExt: InteractiveElement + Sized {
    fn focus_stop(
        self,
        id: &'static str,
        fh: &FocusHandle,
        tab_index: isize,
        on_activate: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        record_focus_id(fh, id);
        self.tab_index(tab_index)
            .track_focus(fh)
            .on_key_down(move |ev, window, app| {
                if matches!(ev.keystroke.key.as_str(), "enter" | "space") {
                    on_activate(ev, window, app);
                }
            })
            .focus(|s| s.border_2().border_color(gpui::rgb(FOCUS_RING)))
    }
}
impl<T: InteractiveElement + Sized> FocusStopExt for T {}

#[cfg(feature = "a11y-capture")]
fn record_focus_id(fh: &FocusHandle, id: &'static str) {
    capture::record_focus(fh.clone(), id);
}
#[cfg(not(feature = "a11y-capture"))]
#[inline]
fn record_focus_id(_fh: &FocusHandle, _id: &'static str) {}
```

Inside the `mod capture` block, add the side-map + reset + resolver (next to the existing `FRAME` thread-local):

```rust
thread_local! {
    static FOCUS: RefCell<Vec<(gpui::FocusHandle, &'static str)>> = const { RefCell::new(Vec::new()) };
}

pub fn record_focus(fh: gpui::FocusHandle, id: &'static str) {
    FOCUS.with(|f| f.borrow_mut().push((fh, id)));
}

/// The label of the element the window currently focuses, resolved through the
/// focus side-map (`fh.is_focused` → static id) joined to the FRAME node with
/// that `click_id`. Independent of kittest's focus support — pure local join.
pub fn focused_label(window: &gpui::Window) -> Option<String> {
    let id = FOCUS.with(|f| {
        f.borrow()
            .iter()
            .find(|(fh, _)| fh.is_focused(window))
            .map(|(_, id)| *id)
    })?;
    FRAME.with(|f| {
        f.borrow()
            .iter()
            .find(|c| c.click_id == Some(id))
            .map(|c| c.text.clone())
    })
}
```

Extend the existing `reset()` in `mod capture` to also clear the focus map:

```rust
pub fn reset() {
    FRAME.with(|f| f.borrow_mut().clear());
    FOCUS.with(|f| f.borrow_mut().clear());
}
```

Export `focused_label` from the capture re-export line:

```rust
#[cfg(feature = "a11y-capture")]
pub use capture::{A11yCapture, A11yExt, AccessRole, focused_label, reset, take_tree_update};
```

(`FocusStopExt` / `FOCUS_RING` are module-root items already `pub` — add `FocusStopExt` to the crate's a11y prelude / re-exports if one exists; otherwise callers use `crate::a11y::FocusStopExt`.)

- [ ] **Step 2: Extend `A11ySnapshot::capture` + add tab combinators in `support/mod.rs`**

Add the field to the struct:

```rust
pub struct A11ySnapshot {
    pub state: State,
    pub click_ids: Vec<Option<&'static str>>,
    /// Label of the element focused at capture time (via the focus oracle), or
    /// `None` if nothing focusable is focused.
    pub focused: Option<String>,
}
```

Extend `capture` (compute focused label inside the same frame bracket, before `take_tree_update` drains nothing — both read the live FRAME/FOCUS):

```rust
pub fn capture(cx: &mut VisualTestContext) -> Self {
    dat0_app::a11y::reset();
    cx.update(|window, _app| window.refresh());
    cx.run_until_parked();
    let focused = cx.update(|window, _app| dat0_app::a11y::focused_label(window));
    let cap = dat0_app::a11y::take_tree_update();
    Self {
        state: State::new(cap.update),
        click_ids: cap.click_ids,
        focused,
    }
}

/// The label of the element focused at capture time.
pub fn focused_label(&self) -> Option<&str> {
    self.focused.as_deref()
}
```

Add free combinators (module level in `support/mod.rs`):

```rust
/// Press Tab (routes through gpui-component `Root`'s Tab binding → `focus_next`).
/// Recapture afterward to read the new focus.
pub fn press_tab(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("tab");
}

/// Press Shift-Tab (→ `focus_prev`).
pub fn press_shift_tab(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("shift-tab");
}
```

- [ ] **Step 3: Add the stable hero-handle map to `WorkspaceShell` (`window.rs`)**

Add the field to the `WorkspaceShell` struct (near `focus_handle: FocusHandle` at ~2047):

```rust
/// Stable per-hero-button focus handles, keyed by the button's static id.
/// Created once and reused across renders (the transient `EmptyState` must NOT
/// own these — it is rebuilt every frame).
hero_focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
```

Initialize in the shell constructor (near `focus_handle: cx.focus_handle()` at ~2251):

```rust
hero_focus: std::collections::HashMap::new(),
```

Add the accessor (method on `WorkspaceShell`):

```rust
/// Get (lazily creating, once) the stable focus handle for hero button `id`.
fn hero_focus_handle(&mut self, id: &'static str, cx: &mut gpui::App) -> gpui::FocusHandle {
    self.hero_focus
        .entry(id)
        .or_insert_with(|| cx.focus_handle())
        .clone()
}
```

- [ ] **Step 4: Build `HeroHandles` in `render` and thread it into `EmptyState` (`window.rs` + `empty_state.rs`)**

In `empty_state.rs`, add the carrier struct (module level) and make `sample_static_id` reachable:

```rust
/// Stable hero-button focus handles, passed down from the persistent
/// `WorkspaceShell` (the `EmptyState` is transient — it must not mint handles).
pub struct HeroHandles {
    pub map: std::collections::HashMap<&'static str, gpui::FocusHandle>,
}
impl HeroHandles {
    pub fn get(&self, id: &'static str) -> &gpui::FocusHandle {
        self.map.get(id).expect("hero handle pre-registered in WorkspaceShell::render")
    }
}
```

Ensure `pub(crate) fn sample_static_id(kind: &SampleKind) -> &'static str` is at least `pub(crate)` (it is already used within the file — widen visibility if needed).

Change `EmptyState::render` signature to accept the handles:

```rust
pub fn render(&self, hero: &HeroHandles, cx: &mut gpui::Context<crate::window::WorkspaceShell>) -> gpui::AnyElement {
    // ... existing body, now with access to `hero` ...
}
```

In `window.rs::render` at line 6077, pre-register handles for the fixed hero id set, then pass them:

```rust
let hero_ids: [&'static str; 3] = ["hero-take-tour", "hero-open-demo", "hero-open-file-samples"];
let mut map = std::collections::HashMap::new();
for id in hero_ids {
    map.insert(id, self.hero_focus_handle(id, cx));
}
for entry in crate::sample_data::entries() {
    let id = crate::empty_state::sample_static_id(&entry.kind);
    map.insert(id, self.hero_focus_handle(id, cx));
}
let hero = crate::empty_state::HeroHandles { map };
// ... EmptyState::new(recents_empty, first_run_done).render(&hero, cx)
```

- [ ] **Step 5: Wire ONLY `hero-take-tour` with `focus_stop` (`empty_state.rs`)**

The take-tour button already has `.id("hero-take-tour")`, `.a11y("hero-take-tour", Button, …)`, and `.on_click(take_tour_handler)`. Add `focus_stop` and a keyboard twin of the handler. Build the activation logic ONCE as a view method call so mouse and keyboard cannot drift:

```rust
// the existing on_click handler:
let take_tour_handler = cx.listener(|this, _ev, window, cx| {
    this.take_tour(window, cx); // whatever the current handler body calls
});
// keyboard twin (same view method, KeyDownEvent-typed):
let take_tour_key = cx.listener(|this, _ev: &gpui::KeyDownEvent, window, cx| {
    this.take_tour(window, cx);
});
// ...
div()
    .id("hero-take-tour")
    .focus_stop("hero-take-tour", hero.get("hero-take-tour"), 0, take_tour_key)
    .a11y("hero-take-tour", AccessRole::Button, dat0_i18n::t("hero.take_tour"))
    .child(dat0_i18n::t("hero.take_tour"))
    .on_click(take_tour_handler)
```

Add `use crate::a11y::FocusStopExt as _;` to `empty_state.rs`. (If the current take-tour handler is an inline closure, not a `this.take_tour(...)` method, extract the body into a small `WorkspaceShell` method first so both listeners call it.)

- [ ] **Step 6: Write the T0 spike test in `tests/keyboard_nav.rs`**

Copy the mount helpers from `tests/motherduck_window.rs` (init components, build empty session, open shell window, async harness — precedent: each binary keeps local copies). Then:

```rust
mod support;
use support::{A11ySnapshot, press_tab};

#[gpui::test]
async fn t0_focus_oracle_and_take_tour(cx: &mut gpui::TestAppContext) {
    let (_state, mut vcx) = open_empty_shell(cx); // local mount helper; empty session → hero visible

    // (1) Tab reaches the take-tour button and (5) the oracle names it by label.
    press_tab(&mut vcx);
    let snap = A11ySnapshot::capture(&mut vcx);
    assert_eq!(
        snap.focused_label(),
        Some(dat0_i18n::t("hero.take_tour").as_str()),
        "Tab did not land on the take-tour button (or oracle failed to name it)"
    );

    // (2) Enter on the focused button fires its handler (tour opens).
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    // assert the tour is now showing — reuse an existing observable from the
    // onboarding harness (e.g. the tour dialog / carousel a11y node, or a
    // WorkspaceShell tour-visible accessor). Example:
    let after = A11ySnapshot::capture(&mut vcx);
    assert!(
        after.has_label(dat0_i18n::t("tour.panel.title").as_str()) /* or the real observable */,
        "Enter on the focused take-tour button did not open the tour"
    );
}
```

- [ ] **Step 7: Run the focused test — prove the six spike criteria**

Run: `cargo test -p dat0-app --test keyboard_nav`
Expected: PASS. This green result establishes, in one test: (1) `focus_stop` div is Tab-reachable via `Root`; (2) Enter fires the handler; (3) the `.focus()` ring compiles + applies (build succeeds with the style); (4) `window.focused()` correlates to the stored handle; (5) `focused_label()` reads it back; (6) the `WorkspaceShell`-owned handle survives the recapture re-render with stable identity.

**Hard-gate decision:** if the test cannot be made to pass after honest debugging (e.g. Tab does not move focus under `TestPlatform`, or `simulate_keystrokes("enter")` does not route to `on_key_down`, or the key strings differ from `"enter"`/`"space"`), STOP and report the exact failure — do not proceed to T1–T4. Adjust key strings / add `gpui_component::init` / confirm `Root` mount as needed; these are expected spike adjustments, not gate failures.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/a11y/mod.rs crates/dat0-app/tests/support/mod.rs \
        crates/dat0-app/src/window.rs crates/dat0-app/src/empty_state.rs \
        crates/dat0-app/tests/keyboard_nav.rs
git commit -s -m "feat(uat): focus oracle + focus_stop helper + take-tour (T0 gate)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 1: Wire the remaining hero buttons + full hero Tab-cycle

**Files:**
- Modify: `crates/dat0-app/src/empty_state.rs` (open-demo, 3 sample cards, open-file)
- Modify: `crates/dat0-app/tests/keyboard_nav.rs` (add tests)

**Interfaces:**
- Consumes: `HeroHandles`, `FocusStopExt::focus_stop`, `press_tab`, `A11ySnapshot::focused_label` (Task 0).

- [ ] **Step 1: Write the failing full-cycle test**

```rust
#[gpui::test]
async fn hero_tab_cycle_visits_every_button(cx: &mut gpui::TestAppContext) {
    let (_state, mut vcx) = open_empty_shell(cx);
    let expected = [
        dat0_i18n::t("hero.take_tour"),
        dat0_i18n::t("hero.demo.cta"),
        // 3 sample titles, in sample_data::entries() order:
        crate::sample_titles()[0].clone(),
        crate::sample_titles()[1].clone(),
        crate::sample_titles()[2].clone(),
        dat0_i18n::t("hero.open_file"),
    ];
    for want in expected {
        press_tab(&mut vcx);
        let snap = A11ySnapshot::capture(&mut vcx);
        assert_eq!(snap.focused_label(), Some(want.as_str()), "tab order mismatch at {want}");
    }
}
```

(Replace `crate::sample_titles()` with the real `dat0_app::sample_data::entries()` titles pulled in the test; keep the assertion order identical to DOM order.)

- [ ] **Step 2: Run it — expect FAIL**

Run: `cargo test -p dat0-app --test keyboard_nav hero_tab_cycle`
Expected: FAIL (only take-tour is focusable; the next `press_tab` yields `None` or the wrong label).

- [ ] **Step 3: Wire the remaining fixed hero buttons with `focus_stop`**

Apply the Task-0 Step-5 pattern to each, all `tab_index` `0` (DOM order):
- `hero-open-demo`: add `.a11y("hero-open-demo", Button, t("hero.demo.cta"))` (it currently has none) + `.focus_stop("hero-open-demo", hero.get("hero-open-demo"), 0, open_demo_key)`; keyboard twin calls the same demo-open method as `open_demo_handler`.
- Each sample card in `sample_column`: it already has `.a11y(static_id, Button, title)`; add `.focus_stop(static_id, hero.get(static_id), 0, sample_key)` where `sample_key` calls `this.open_sample_kind(kind.clone(), cx)` (KeyDownEvent-typed twin of the existing `handler`).
- `hero-open-file-samples`: add `.a11y("hero-open-file-samples", Button, t("hero.open_file"))` + `.focus_stop(...)` with an open-file keyboard twin.

`sample_column` takes `cx: &mut Context<WorkspaceShell>` but not `hero` — thread `hero: &HeroHandles` through `sample_column`'s signature (and `render`'s call site).

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test -p dat0-app --test keyboard_nav`
Expected: PASS (both T0 and the cycle test).

- [ ] **Step 5: Add the Enter-activation test for a second button (operability breadth)**

```rust
#[gpui::test]
async fn hero_enter_activates_open_demo(cx: &mut gpui::TestAppContext) {
    let (_state, mut vcx) = open_empty_shell(cx);
    press_tab(&mut vcx); // take-tour
    press_tab(&mut vcx); // open-demo
    let snap = A11ySnapshot::capture(&mut vcx);
    assert_eq!(snap.focused_label(), Some(dat0_i18n::t("hero.demo.cta").as_str()));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    // assert the demo workspace began opening — reuse an existing observable
    // (e.g. a new tab / the demo table in the catalog). Use the same observable
    // the onboarding_gpui.rs demo test asserts.
}
```

- [ ] **Step 6: Run + commit**

```bash
cargo test -p dat0-app --test keyboard_nav
cargo fmt --all
git add crates/dat0-app/src/empty_state.rs crates/dat0-app/tests/keyboard_nav.rs
git commit -s -m "feat(uat): keyboard-reachable hero buttons + tab-cycle tests

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Settings DIY toggles keyboard-operable + reachability tests

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (`toggle_row` + `SettingsPanel` handles)
- Modify: `crates/dat0-app/tests/keyboard_nav.rs` (settings tests; Settings mount = Slice-1 pattern)

**Interfaces:**
- Consumes: `FocusStopExt`, `press_tab`, `A11ySnapshot::focused_label`.
- Note: Settings `Input`s/`Button`s (name/email/budget, theme/log-level/Reset) are gpui-component widgets — **already tab stops**, no change. Only the 3 raw-div `toggle_row`s need `focus_stop`.

- [ ] **Step 1: Failing test — Tab reaches a DIY toggle and Enter flips it**

```rust
#[gpui::test]
async fn settings_toggle_keyboard_reachable_and_operable(cx: &mut gpui::TestAppContext) {
    let (_g, dir) = set_config_dir(); // Slice-1 DAT0_CONFIG_DIR seam (#[serial])
    let (_panel, mut vcx) = open_settings_panel(cx, &dir); // Slice-1 mount helper
    // navigate to the telemetry section first (click sidebar — reuse Slice-1),
    // then Tab until the telemetry toggle is focused:
    let mut found = false;
    for _ in 0..20 {
        press_tab(&mut vcx);
        let snap = A11ySnapshot::capture(&mut vcx);
        if snap.focused_label() == Some(dat0_i18n::t("settings.telemetry.toggle").as_str()) {
            found = true;
            break;
        }
    }
    assert!(found, "Tab never reached the telemetry DIY toggle");
    let before = telemetry_enabled(&dir);
    vcx.simulate_keystrokes("space");
    vcx.run_until_parked();
    assert_ne!(telemetry_enabled(&dir), before, "Space did not flip the toggle");
}
```

- [ ] **Step 2: Run — expect FAIL**

Run: `cargo test -p dat0-app --test keyboard_nav settings_toggle -- --test-threads=1`
Expected: FAIL (toggle not focusable).

- [ ] **Step 3: Add `focus_stop` to `toggle_row` + stable handles on `SettingsPanel`**

Add a handle map to `SettingsPanel` (persistent entity):

```rust
toggle_focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
```

Initialize `toggle_focus: HashMap::new()` in `SettingsPanel::new`. In `toggle_row` (which already receives `id: &'static str` and `cx: &mut Context<Self>`), fetch the stable handle and chain `focus_stop`. The activation flips the setting exactly like the existing `on_click`:

```rust
fn toggle_row(&mut self, id, label_key, on, cx, set: fn(&SettingsStore, bool) -> anyhow::Result<()>) -> impl IntoElement {
    let fh = self.toggle_focus.entry(id).or_insert_with(|| cx.focus_handle()).clone();
    let key_activate = cx.listener(move |this, _ev: &gpui::KeyDownEvent, _w, cx| {
        let _ = set(&this.store, !on);
        cx.notify();
    });
    div()
        .id(id)
        .cursor_pointer()
        // ... existing flex/child([x]/[ ])/child(label) ...
        .focus_stop(id, &fh, 0, key_activate)
        .a11y(id, AccessRole::Button, dat0_i18n::t(label_key))
        .on_click(cx.listener(move |this, _ev, _w, cx| { let _ = set(&this.store, !on); cx.notify(); }))
}
```

`toggle_row` must take `&mut self` now (for `toggle_focus.entry`) — update its 3 call sites (they already pass `self.toggle_row(...)`; confirm the receiver is `&mut`). Add `use crate::a11y::FocusStopExt as _;` (already imports `A11yExt`).

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test -p dat0-app --test keyboard_nav settings_toggle -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Add a sidebar/inputs reachability test**

```rust
#[gpui::test]
async fn settings_profile_inputs_reachable(cx: &mut gpui::TestAppContext) {
    let (_g, dir) = set_config_dir();
    let (_panel, mut vcx) = open_settings_panel(cx, &dir);
    // Profile section is default-selected; Tab should reach the name + email inputs.
    // gpui-component Input registers tab_index → Tab already visits them; assert
    // focus lands (labels are the placeholders / adjacent .a11y labels).
    let mut labels = vec![];
    for _ in 0..8 {
        press_tab(&mut vcx);
        labels.push(A11ySnapshot::capture(&mut vcx).focused_label().map(String::from));
    }
    assert!(labels.iter().flatten().count() >= 2, "Tab reached fewer than 2 focusable Settings elements");
}
```

(If gpui-component `Input` does not surface a label the oracle can read — its FocusHandle is internal, not registered via `focus_stop` → `focused_label()` returns `None` for it — assert instead that Tab *reaches the DIY toggles + Reset button* which DO carry `focus_stop`/`.a11y`. Confirm empirically in Step 6 and keep whichever assertion is honest; note the limitation in a code comment.)

- [ ] **Step 6: Run all + commit**

```bash
cargo test -p dat0-app --test keyboard_nav -- --test-threads=1
cargo fmt --all
git add crates/dat0-app/src/settings_ui/panel.rs crates/dat0-app/tests/keyboard_nav.rs
git commit -s -m "feat(uat): keyboard-operable Settings DIY toggles + reachability tests

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Grid Tab-reachability + arrow-nav via SelectionModel

The grid uses a DIFFERENT keyboard mechanism than the tab-focus chain: Tab reaches the **grid shell** (one focus handle), then arrow keys drive the `SelectionModel` via `grid/keymap.rs` (NOT gpui focus). Its ring is `SelectionModel::is_active` (`grid/mod.rs:562-569`), decoupled from `window.focused()`. So this task asserts arrow-nav via the `SelectionModel`, not via `focused_label`.

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (add `.tab_index(...)` to the shell `.track_focus` element ~6422; add a `#[cfg(feature="a11y-capture")]` SelectionModel accessor if none exists)
- Modify: `crates/dat0-app/tests/keyboard_nav.rs` (grid test; seeded-data mount = Slice-3/5 pattern)

**Interfaces:**
- Consumes: `SelectionModel::active(&self) -> CellCoord` (`grid/selection.rs:78`); Slice-3/5 grid-data seed helper.
- Produces: `#[cfg(feature="a11y-capture")] pub fn grid_active_cell_for_test(&self) -> CellCoord` on `WorkspaceShell` (if the shell doesn't already expose the grid's selection).

- [ ] **Step 1: Failing test — Tab reaches grid, arrow moves the active cell**

```rust
#[gpui::test]
async fn grid_tab_reach_then_arrow_moves_active_cell(cx: &mut gpui::TestAppContext) {
    let (state, mut vcx) = open_shell_with_seeded_grid(cx); // Slice-3/5 seed: a small table rendered
    // Tab into the grid shell:
    press_tab(&mut vcx);
    vcx.run_until_parked();
    let before = shell_active_cell(&state, &mut vcx); // via grid_active_cell_for_test
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    let after = shell_active_cell(&state, &mut vcx);
    assert_ne!(before, after, "arrow key did not move the grid active cell");
    assert_eq!(after.row, before.row + 1, "Down should advance one row");
}
```

- [ ] **Step 2: Run — expect FAIL**

Run: `cargo test -p dat0-app --test keyboard_nav grid_tab_reach`
Expected: FAIL — Tab does not land on the grid (shell has `track_focus` but no `tab_index`), so the arrow keys never reach `keymap`.

- [ ] **Step 3: Make the shell a tab stop + expose the active cell**

In `window.rs` at the shell root element (~6422, where `.track_focus(&self.focus_handle)` is chained), add a tab stop so Tab reaches the grid region:

```rust
.track_focus(&self.focus_handle)
.tab_index(0)
```

Add the test accessor (before the `#[cfg(test)] mod tests`, with the other `#[cfg(feature="a11y-capture")] pub fn *_for_test` shims):

```rust
#[cfg(feature = "a11y-capture")]
pub fn grid_active_cell_for_test(&self) -> crate::grid::CellCoord {
    self.grid_selection.active() // adjust to the real field holding the SelectionModel
}
```

(Locate the shell field that owns the grid's `SelectionModel`; if the selection lives inside a `GridView` entity, read it through that entity in the accessor.)

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test -p dat0-app --test keyboard_nav grid_tab_reach`
Expected: PASS.

- [ ] **Step 5: Run all + commit**

```bash
cargo test -p dat0-app --test keyboard_nav
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/keyboard_nav.rs
git commit -s -m "feat(uat): grid Tab-reachability + arrow-nav SelectionModel test

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Docs — mark §10 automation status + owed focus-ring glance + final gate

**Files:**
- Modify: `docs/a11y.md` (A1 status)
- Modify: `docs/plans/2026-06-23-dat0-p10b-uat.md` (§10 annotate automated items)

- [ ] **Step 1: Update `docs/a11y.md` A1**

Change A1 (keyboard-nav reaches every interactive element) from "UAT-pending" to note the automated coverage:
- Hero buttons, Settings DIY toggles: now keyboard-reachable + operable, **automated** in `tests/keyboard_nav.rs` (Slice 6).
- Grid Tab-reach + arrow-nav: automated (via `SelectionModel`).
- Still UAT-pending / human: A2 focus-**ring** pixel appearance + WCAG contrast (Gap 1); Catalog/AI/SQL-editor/cell-editor internal nav (deferred slice).

Keep the change factual — do NOT claim A2 (ring contrast) is closed; it is not.

- [ ] **Step 2: Annotate P10b UAT §10**

In `docs/plans/2026-06-23-dat0-p10b-uat.md` §10, mark §10.1 (hero), the Settings toggle rows in §10.2–10.5, and the §10.6 grid arrow-nav line as "automated (Slice 6, `tests/keyboard_nav.rs`)"; leave the focus-**ring visual** confirmation (§11.4) as the remaining human glance.

- [ ] **Step 3: Commit docs**

```bash
git add docs/a11y.md docs/plans/2026-06-23-dat0-p10b-uat.md
git commit -s -m "docs(uat): mark Slice 6 keyboard-nav automation; ring glance stays human

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: CONTROLLER final gate (not the implementer)**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff main --stat   # confirm Cargo.lock / Cargo.toml / NOTICE UNCHANGED
```

Expected: all green; dependency/NOTICE files untouched (zero-new-deps constraint). Confirm the release build compiles with the feature OFF (the `focus_stop` production wiring must build without `a11y-capture`): `cargo build -p dat0-app --release`.

---

## Owed human follow-up (record in the merge notes / memory)

- **Focus-ring visual glance** (§11.4 / A2): eyeball the 2px ring on hero buttons + Settings DIY toggles in each built-in theme, and its WCAG ≥3:1 contrast. This is the FIRST slice to ship real release code (`focus_stop`), so this glance is genuinely owed and joins the standing About/Charts/Settings glances.
- **WATCH the post-merge main run** (push-to-main-only macOS grid-scroll bench can redden main silently).
- Deferred follow-on slice: Catalog-tree / AI-prompt / SQL-editor / cell-editor internal keyboard nav.

## Self-Review

**Spec coverage:** §A goal → all tasks. §B oracle → T0 (side-map, `focused_label`, tab combinators). §C `focus_stop` + surfaces → T0 (helper + take-tour), T1 (hero), T2 (toggles), T3 (grid). §D tests → T0–T3. §E boundary: IN = T0–T3; DEFER (Catalog/AI/SQL/cell-editor) explicitly out + noted; STAYS HUMAN (ring pixels) = T4 docs + owed-glance. §F spike gate → T0 Step 7 (all six criteria). §G mechanics (new binary, feature gating, zero deps, glance) → Global Constraints + T4 gate.

**Placeholder scan:** The test bodies reference existing-but-unnamed observables (tour-visible, demo-opened, Settings mount helper `open_settings_panel`, `telemetry_enabled`, seeded-grid helper) — these are pointers to REAL Slice-1/3/5 patterns the implementer copies, not invented APIs; each is flagged with its source slice. The gpui key strings (`"enter"`/`"space"`/`"down"`) and `.focus()` StyleRefinement builder are marked spike-verified in T0.

**Type consistency:** `focus_stop(id, &FocusHandle, isize, Fn(&KeyDownEvent,&mut Window,&mut App))` used identically in T0/T1/T2. `HeroHandles::get -> &FocusHandle`, `hero_focus_handle -> FocusHandle` consistent. `focused_label` returns `Option<String>` (a11y) / `Option<&str>` (snapshot method) — intentional (owned vs borrowed), used correctly at call sites. `SelectionModel::active() -> CellCoord` matches T3 accessor.
