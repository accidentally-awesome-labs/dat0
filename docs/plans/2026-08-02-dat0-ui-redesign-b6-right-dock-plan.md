# UI Redesign B6 — Right Dock Implementation Plan

> **For agentic workers:** steps use checkbox (`- [ ]`) syntax for tracking. This
> slice is executed INLINE by the controller (no subagents), per the standing
> preference since A5.

**Goal:** Move the Inspector and Charts panels out of the hand-rolled fixed docks in `WorkspaceShell::render`'s body row into a real `DockArea` right dock.

**Architecture:** Two thin `Panel` implementors (B5's template — one `WeakEntity<WorkspaceShell>` field each, all state stays in the shell) mounted as `DockItem::split(Horizontal, [tab(Inspector), tab(Charts)])` on the dock's right side. The shell's existing visibility bools stay the single source of truth; the dock is reconciled from them at the top of `render`, the only place with a guaranteed `&mut Window`.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35`, DuckDB, `a11y-capture` + AccessKit/kittest test harness.

**Design doc:** `docs/plans/2026-08-02-dat0-ui-redesign-b6-right-dock-design.md` (commit `4cc64b1`). Read §2 before touching dock code — every upstream fact below was verified there against the pinned checkout.

## Global Constraints

- **Pinned rev must not be bumped.** gpui-component stays at `0f0ab35`. Every upstream behavior relied on here was read out of `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/crates/ui/src/dock/`.
- **Session schema stays v10.** No `SessionUiState` field is added, removed or reinterpreted. `chart_panel_visible` remains deliberately unpersisted.
- **`style_lint` ratchet stays exactly `[("window.rs", 1)]`.** No new colour literal anywhere; all colour comes from `cx.theme()` / `cx.theme().d0()`.
- **Existing tests are the regression oracle.** `tests/chart_uat_window.rs`, `tests/chart_panel_wiring.rs`, `tests/a11y_content.rs`, `tests/chart_wire_snapshot.rs` and the nav suite must pass **unmodified**. If one needs editing, stop and report it as a finding.
- **Non-vacuity is mandatory** for every new assertion: perturb the thing being asserted, watch it go red, revert. After reverting a probe file, `touch` it — a `mv`-revert backwards-dates the file and cargo silently reuses the stale binary (A6 lesson).
- **Local gate** (run before every commit that touches Rust): `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p dat0-app`. Full three-feature sweep (`plain` / `a11y-capture` / `a11y-capture,gallery`) at the end of each task. `cargo test --workspace` and `cargo bench` remain unrunnable on this machine (macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift) — that is pre-existing and not a branch signal.
- **Commit messages:** `git commit -s` (DCO). Never write the CI skip marker in any commit message, even quoted in prose. Use `-F -` with a heredoc for any message containing backticks — zsh command-substitutes them inside `-m "…"`.
- **`&'static str` ids only** for `.a11y(..)`; dynamic ids must use `.a11y_label(..)`. Never add a label to an element that already has one — both helpers `push()` a new node (A5).

---

### Task 0: Chrome spike — does `TabPanel` chrome change what the a11y capture sees?

This is a **blocking gate**. It answers the master plan's top risk for B5-B8 with a synthetic panel rather than the real ones, so there are no confounders: the only difference between the two measurements is the upstream chrome.

**Files:**
- Create: `crates/dat0-app/tests/dock_chrome_spike.rs`

**Interfaces:**
- Consumes: `support::A11ySnapshot` (`capture`, `count_label`, `focused_label`), `support::press_tab`.
- Produces: nothing consumed by later tasks. The measured node count is recorded in the commit message and in the design doc's §8 row.

- [ ] **Step 1: Write the spike**

```rust
//! B6 T0 gate — does a `TabPanel`'s chrome change what the single-frame a11y
//! capture sees?
//!
//! B5 measured that `DockItem::Panel` (the center, zero chrome) does NOT
//! double-render. B6 mounts real docks, and a dock's item is built out of
//! `TabPanel`s, which wrap the panel view in `overflow_y_scroll` and then in
//! `.cached(StyleRefinement::default().absolute().size_full())`
//! (`tab_panel.rs:851-861`). A cached element is the one construct that could
//! plausibly break a capture that runs during a single forced frame — and note
//! the likely failure is nodes going MISSING, not duplicating. The
//! generation-counter fallback designed at `a11y/mod.rs:24` fixes duplicates
//! and would NOT fix omissions, so this must be measured before any production
//! code depends on the answer.
//!
//! The probe panel is synthetic on purpose: the only difference between the
//! two mounts is upstream chrome.

mod support;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, Window, div, px,
};
use gpui_component::Root;
use gpui_component::dock::{DockArea, DockItem, Panel, PanelEvent};
use serial_test::serial;

use dat0_app::a11y::{A11yExt as _, AccessRole};
use support::A11ySnapshot;

const PROBE_LABEL: &str = "probe-body";

/// A panel that emits exactly ONE capture node, so a count of 1 / 2 / 0 reads
/// directly as intact / duplicated / swallowed.
struct ProbePanel {
    fh: FocusHandle,
}

impl EventEmitter<PanelEvent> for ProbePanel {}

impl Focusable for ProbePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.fh.clone()
    }
}

impl Panel for ProbePanel {
    fn panel_name(&self) -> &'static str {
        "ProbePanel"
    }
}

impl Render for ProbePanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .a11y_label(AccessRole::Label, PROBE_LABEL.to_string())
            .child(PROBE_LABEL)
    }
}

/// `TestAppContext::add_window_view`'s closure returns the VIEW VALUE, not an
/// `Entity`, so hosting a child entity needs a small owner struct (B5 lesson).
struct DockHost {
    dock: Entity<DockArea>,
}

impl Render for DockHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.dock.clone())
    }
}

/// Baseline: the B5 mount. Center only, `DockItem::Panel`, zero chrome.
#[gpui::test]
#[serial]
fn bare_center_panel_emits_exactly_one_node(cx: &mut TestAppContext) {
    let count = mount_and_count(cx, false);
    assert_eq!(
        count, 1,
        "baseline: a bare DockItem::Panel center must emit the probe's single \
         node exactly once (this is B5's measured result, re-proven here as the \
         control for the chrome case below)"
    );
}

/// The B6 mount: a right dock, whose item is built out of `TabPanel`s.
#[gpui::test]
#[serial]
fn right_dock_tab_panel_chrome_emits_exactly_one_node(cx: &mut TestAppContext) {
    let count = mount_and_count(cx, true);
    assert_eq!(
        count, 1,
        "TabPanel chrome (overflow_y_scroll + .cached(..) + .tab_group()) must \
         not change what a single-frame capture sees. 2 => the dock \
         double-renders and the generation counter at a11y/mod.rs:24 is now \
         needed; 0 => the .cached() wrapper swallowed the frame and the counter \
         would NOT help — see the design doc §8"
    );
}

/// Mount a `DockArea` with a probe panel in the center, optionally also in a
/// right dock, and return how many times the probe's label reached the capture.
fn mount_and_count(cx: &mut TestAppContext, with_right_dock: bool) -> usize {
    let window = cx.add_window(|window, cx| {
        gpui_component::init(cx);
        let dock = cx.new(|cx| {
            let mut dock = DockArea::new("spike-dock", Some(1), window, cx);
            dock.set_locked(true, window, cx);
            dock
        });
        let weak = dock.downgrade();

        let center = cx.new(|cx| ProbePanel { fh: cx.focus_handle() });
        let center_item = DockItem::panel(std::sync::Arc::new(center));

        dock.update(cx, |dock, cx| {
            dock.set_center(center_item, window, cx);
            if with_right_dock {
                let right = cx.new(|cx| ProbePanel { fh: cx.focus_handle() });
                let item = DockItem::tab(right, &weak, window, cx).size(px(288.));
                let split = DockItem::split(gpui::Axis::Horizontal, vec![item], &weak, window, cx);
                dock.set_right_dock(split, Some(px(288.)), true, window, cx);
            }
        });

        Root::new(cx.new(|_| DockHost { dock }).into(), window, cx)
    });

    let cx = &mut gpui::VisualTestContext::from_window(*window, cx);
    let snap = A11ySnapshot::capture(cx);
    // The center probe always contributes 1; the right dock adds another mount
    // of the SAME label, so subtract the baseline to isolate the chrome case.
    let total = snap.count_label(PROBE_LABEL);
    if with_right_dock { total - 1 } else { total }
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p dat0-app --features a11y-capture --test dock_chrome_spike -- --nocapture`

Both tests must compile and run. **Whatever the outcome, record the two numbers.** Do not proceed past Step 4 without them.

- [ ] **Step 3: Prove the probe is not vacuous**

Change `PROBE_LABEL` to a string the tree does not contain (e.g. `"probe-body-XYZ"`) in the assertion only — the baseline test must go red with `0`. Revert, `touch tests/dock_chrome_spike.rs`, re-run, confirm green. A test that counts a label it cannot find would report `0 == 0` forever if the assertion were `>= 0`.

- [ ] **Step 4: Branch on the result**

- **Both counts are 1** — chrome is transparent to the capture. Record it and continue to Task 1.
- **Chrome count is 2** — the dock double-renders. Build the generation counter designed at `a11y/mod.rs:24` (bump on `begin_frame()`, keep only max-generation nodes) as part of this task, then re-run until both are 1.
- **Chrome count is 0** — the `.cached()` wrapper swallowed the frame. The counter does not help. Fix in-slice by invalidating the cache on the capture frame, then re-run. If no in-slice fix works, **stop and report** — the whole B6-B8 mounting strategy depends on this.

- [ ] **Step 5: Probe Tab order**

Add a third test that mounts the right-dock case, presses Tab four times, and records `snap.focused_label()` after each press into a `Vec<Option<String>>`, asserting the observed sequence. This is a **characterization** test: write it against whatever the run actually produces, and state that in a comment. Its job is to fail loudly if `.tab_group()` reordering changes later, not to encode a preference.

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/tests/dock_chrome_spike.rs
git commit -s -F - <<'EOF'
test(theme): B6 T0 — measure TabPanel chrome against the a11y capture

<one line per measured number: baseline count, chrome count, Tab sequence>

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 1: `InspectorPanel` + shell body extraction

**Files:**
- Create: `crates/dat0-app/src/panels/inspector_panel.rs`
- Modify: `crates/dat0-app/src/panels/mod.rs` (add `pub mod inspector_panel;` and a `register_panel` arm)
- Modify: `crates/dat0-app/src/window.rs` (add `render_inspector_body`)
- Modify: `crates/dat0-app/src/inspector/panel.rs:36-41` (drop the body title row)

**Interfaces:**
- Consumes: `WorkspaceShell::inspector`, `WorkspaceShell::inspector_projection()`, `inspector::panel::render_inspector`.
- Produces:
  - `InspectorPanel::PANEL_NAME: &str = "InspectorPanel"`
  - `InspectorPanel::new(shell: WeakEntity<WorkspaceShell>) -> Self`
  - `WorkspaceShell::render_inspector_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement`

- [ ] **Step 1: Write the failing test (panel-name ratchet)**

At the bottom of the new `inspector_panel.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key: `DockArea::load` resolves it through the global
    /// `PanelRegistry`, and upstream's trait docs say it must never change once
    /// defined. Rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        let panel = InspectorPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "InspectorPanel");
        assert_eq!(InspectorPanel::PANEL_NAME, "InspectorPanel");
    }

    /// A shell-less panel must degrade, not panic — the B9 placeholder builder
    /// in `register_panels` hands one out.
    #[test]
    fn shell_less_panel_is_not_visible() {
        let panel = InspectorPanel::new(gpui::WeakEntity::new_invalid());
        // `visible` needs an `App`; assert the weak handle is dead instead,
        // which is the branch `visible()` keys off.
        assert!(panel.shell.upgrade().is_none());
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p dat0-app --lib panels::inspector_panel`
Expected: FAIL — `unresolved import` / module does not exist.

- [ ] **Step 3: Write `inspector_panel.rs`**

```rust
//! B6: the right dock's Inspector panel — a thin wrapper over the shell's
//! inspector body, following B5's `GridPanel` template exactly.
//!
//! The panel owns NO inspector state. [`crate::window::WorkspaceShell`] keeps
//! `inspector`, the projection context and the visibility bool; this panel's
//! `render` delegates straight back into
//! `WorkspaceShell::render_inspector_body`.
//!
//! ## Why `title()` carries the a11y label
//!
//! `inspector::panel::render_inspector` used to draw its own "Inspector" title
//! row. A `TabPanel` paints a 30px title bar above the body, so keeping both
//! would show the word twice. The row moved here rather than being deleted, so
//! the accessible name survives in the capture tree — and it lands OUTSIDE the
//! `.cached()` wrapper (only `active-panel` → `tab-content` is cached,
//! `tab_panel.rs:851-861`), which is the safer side of the T0 risk.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::window::WorkspaceShell;

pub struct InspectorPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl InspectorPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B6 onward.**
    pub const PANEL_NAME: &str = "InspectorPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for InspectorPanel {}

impl Focusable for InspectorPanel {
    /// The SHELL's root handle, not a private one — a private handle is tracked
    /// by no element, so focusing it swallows focus instead of moving it (B5).
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for InspectorPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// Called every frame by the title bar, so this stays a static i18n lookup
    /// and one `push`ed label — never a `format!`.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = dat0_i18n::t("inspector.title");
        div()
            .a11y_label(AccessRole::Label, title.clone())
            .child(SharedString::from(title))
    }

    /// v1 dock scope is resize + collapse only. Note this does NOT remove the
    /// ⋯ button — `tab_panel.rs:483` renders it unconditionally; it only makes
    /// the menu's "Zoom In" row disabled.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// The shell's bool is the single source of truth (design §5). A dead weak
    /// handle means the B9 placeholder builder produced this panel — render
    /// nothing rather than pretending to be an inspector.
    fn visible(&self, cx: &App) -> bool {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).inspector_visible())
            .unwrap_or(false)
    }
}

impl Render for InspectorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_inspector_body(cx))
    }
}
```

- [ ] **Step 4: Add the shell side**

In `window.rs`, next to `render_grid_body` (~`:6486`):

```rust
    /// B6: the Inspector panel's element tree, extracted from the body row's
    /// `.w_72()` block so [`crate::panels::inspector_panel::InspectorPanel`]
    /// can call it. The sizing and border the block used to carry are the
    /// dock's job now, and the title row moved to `InspectorPanel::title`.
    pub(crate) fn render_inspector_body(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        crate::inspector::panel::render_inspector(&self.inspector, self.inspector_projection(), cx)
    }

    /// B6: the Inspector's visibility, read by `InspectorPanel::visible`.
    /// The bool stays the single source of truth; the dock derives from it.
    pub(crate) fn inspector_visible(&self) -> bool {
        self.inspector_panel_visible
    }
```

- [ ] **Step 5: Drop the duplicated title row**

In `inspector/panel.rs`, replace the title-bearing head of `render_inspector`:

```rust
    let mut root = div().flex().flex_col().gap_2().p_2().child(
        div()
            .a11y_label(AccessRole::Label, title.clone())
            .child(SharedString::from(title)),
    );
```

with a root that carries no title child:

```rust
    // B6: the "Inspector" title row moved to `InspectorPanel::title`, which
    // renders it in the dock's 30px title bar. Keeping it here too would show
    // the word twice. The a11y label moved with it, so the accessible name is
    // relocated, not lost.
    let mut root = div().flex().flex_col().gap_2().p_2();
```

Then delete the now-unused `let title = …` binding and any import that goes unused. **Run clippy to find them — do not guess.**

- [ ] **Step 6: Register the panel**

In `panels/mod.rs`, add `pub mod inspector_panel;` and a second `register_panel` arm mirroring the `GridPanel` one exactly, handing back `InspectorPanel::new(gpui::WeakEntity::new_invalid())`.

- [ ] **Step 7: Run the gate**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app
cargo test -p dat0-app --features a11y-capture
```

`tests/a11y_content.rs` must still pass — if it asserted the inspector title it would go red here, which is the signal that the grep in the design was wrong. Nothing mounts `InspectorPanel` yet, so behavior is unchanged.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -s -F - <<'EOF'
feat(theme): B6 T1 — InspectorPanel + inspector body extraction

Adds the thin InspectorPanel (B5's template: one WeakEntity<WorkspaceShell>
field, shell keeps all state) and extracts render_inspector_body from the body
row's fixed-dock block. Not mounted yet.

Moves the inspector's own title row into Panel::title so the coming 30px dock
title bar does not show the word twice; the a11y label moves with it.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: Chart-export command-palette actions

Deliberately **before** the buttons move (Task 3), so at no commit is chart export keyboard-unreachable.

**Files:**
- Modify: `crates/dat0-app/src/actions/builtin.rs` (two `ids` consts + two `register_all` arms + two dispatch fns)
- Modify: `crates/dat0-app/src/window.rs` (`export_chart` → `pub(crate)`)
- Modify: `crates/dat0-app/tests/command_palette.rs` (visibility test)

**Interfaces:**
- Consumes: `WorkspaceShell::export_chart(&mut self, png: bool, cx: &mut Context<Self>)`, `actions::builtin::focused_workspace`.
- Produces: `ids::CHART_EXPORT_PNG = "chart.export.png"`, `ids::CHART_EXPORT_SVG = "chart.export.svg"`.

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/command_palette.rs`:

```rust
/// B6 moves the chart export buttons into the dock title bar, where upstream
/// forces `tab_stop(false)` (`tab_panel.rs:454`). These two descriptors are
/// what keeps export reachable from the keyboard. They are deliberately NOT in
/// `HIDDEN`: that list is for actions dead by construction, whereas these work
/// whenever a chart is rendered — exactly `view.copy`'s situation.
#[test]
fn chart_export_actions_are_visible_in_the_palette() {
    let reg = dat0_app::actions::registry::ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).expect("register_all");

    let titles: Vec<String> = dat0_app::command_palette::visible_items(&reg, "export chart")
        .into_iter()
        .map(|d| d.title)
        .collect();

    assert!(
        titles.iter().any(|t| t == "Export Chart as PNG"),
        "chart.export.png must be reachable from the palette; got {titles:?}"
    );
    assert!(
        titles.iter().any(|t| t == "Export Chart as SVG"),
        "chart.export.svg must be reachable from the palette; got {titles:?}"
    );
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p dat0-app --test command_palette chart_export`
Expected: FAIL — both `assert!`s fire with a `titles` list that omits them.

- [ ] **Step 3: Add the ids**

In `actions/builtin.rs`'s `ids` module, after `VIEW_EXPORT`:

```rust
    // B6: chart export, reachable from ⌘⇧P. The dock title bar's own buttons
    // are forced `tab_stop(false)` by upstream, so these are the keyboard path.
    pub const CHART_EXPORT_PNG: &str = "chart.export.png";
    pub const CHART_EXPORT_SVG: &str = "chart.export.svg";
```

- [ ] **Step 4: Register and dispatch**

In `register_all`, following the `edit_actions.rs` shape exactly:

```rust
    reg.register(ActionDescriptor {
        id: ActionId::from(ids::CHART_EXPORT_PNG),
        title: "Export Chart as PNG".into(),
        group: ActionGroup::File,
        keybinding: None,
        dispatch: Arc::new(|app| {
            dispatch_chart_export(app, true);
        }),
    })?;

    reg.register(ActionDescriptor {
        id: ActionId::from(ids::CHART_EXPORT_SVG),
        title: "Export Chart as SVG".into(),
        group: ActionGroup::File,
        keybinding: None,
        dispatch: Arc::new(|app| {
            dispatch_chart_export(app, false);
        }),
    })?;
```

and a dispatch fn beside the other `dispatch_*` helpers:

```rust
/// A no-op when no chart is rendered — `export_chart` guards on
/// `chart_panel.data`. That is `view.copy`'s precedent (registered, visible,
/// inert without a selection), not the dead-menu-item defect PRs #59/#60 fixed.
fn dispatch_chart_export(app: &mut gpui::App, png: bool) {
    let Some(workspace) = focused_workspace(app) else {
        tracing::debug!(png, "chart.export: no focused workspace");
        return;
    };
    workspace.update(app, |ws, cx| ws.export_chart(png, cx));
}
```

- [ ] **Step 5: Widen `export_chart`**

In `window.rs:4036`, change `fn export_chart(` to `pub(crate) fn export_chart(`.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p dat0-app --test command_palette`
Expected: PASS, including the pre-existing palette tests.

- [ ] **Step 7: Prove non-vacuity**

Temporarily change one registered title to `"Export Chart as PNGX"`; the test must go red on the PNG assertion only. Revert and re-run.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -s -F - <<'EOF'
feat(theme): B6 T2 — chart export reachable from the command palette

Registers chart.export.png / chart.export.svg so chart export has a keyboard
path before B6 T3 moves its buttons into the dock title bar, where upstream
forces tab_stop(false). Not added to HIDDEN: these work whenever a chart is
rendered, which is view.copy's situation rather than a dead-by-construction
action.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: `ChartsPanel` + body extraction + export buttons into the title bar

**Files:**
- Create: `crates/dat0-app/src/panels/charts_panel.rs`
- Modify: `crates/dat0-app/src/panels/mod.rs` (module + registration arm)
- Modify: `crates/dat0-app/src/window.rs` (`render_charts_body`, `chart_visible`, `render_chart_toolbar` loses two buttons)
- Modify: `crates/dat0-i18n/src/strings/en.json` (one new key)

**Interfaces:**
- Consumes: `WorkspaceShell::chart_panel`, `chart_image`, `export_chart` (now `pub(crate)` from Task 2), `charts::panel::render_chart_body`.
- Produces:
  - `ChartsPanel::PANEL_NAME: &str = "ChartsPanel"`
  - `ChartsPanel::new(shell: WeakEntity<WorkspaceShell>) -> Self`
  - `WorkspaceShell::render_charts_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement`
  - `WorkspaceShell::chart_visible(&self) -> bool`

- [ ] **Step 1: Add the i18n key**

In `crates/dat0-i18n/src/strings/en.json`, beside the other `chart.*` keys:

```json
  "charts.title": "Charts",
```

`charts.title` was confirmed absent (only `inspector.title`, `chart.panel.title`, `chart.save` exist). **A duplicate key would be silently overwritten with no error** (A5) — re-grep before adding.

- [ ] **Step 2: Write the failing test**

At the bottom of the new `charts_panel.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_name_is_frozen() {
        let panel = ChartsPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "ChartsPanel");
        assert_eq!(ChartsPanel::PANEL_NAME, "ChartsPanel");
    }

    /// The title bar's two buttons are the ONLY export affordance after this
    /// task, so their ids are load-bearing for `tests/right_dock.rs`.
    #[test]
    fn export_button_ids_are_stable() {
        assert_eq!(ChartsPanel::EXPORT_PNG_ID, "chart-export-png");
        assert_eq!(ChartsPanel::EXPORT_SVG_ID, "chart-export-svg");
    }
}
```

- [ ] **Step 3: Run it to confirm it fails**

Run: `cargo test -p dat0-app --lib panels::charts_panel`
Expected: FAIL — module does not exist.

- [ ] **Step 4: Write `charts_panel.rs`**

```rust
//! B6: the right dock's Charts panel — a thin wrapper over the shell's chart
//! body, following B5's `GridPanel` template.
//!
//! ## Why the export buttons live here and not in the body
//!
//! `Panel::toolbar_buttons` renders into the 30px title bar. Upstream stamps
//! `.xsmall().ghost().tab_stop(false)` on every one of them
//! (`tab_panel.rs:454`), so these two are mouse-only by construction — which is
//! exactly why B6 T2 registered `chart.export.png` / `chart.export.svg` in the
//! command palette first. The chart-type cycle, the per-axis cycles and Save
//! stay in the body: Save carries a real `disabled` state that reads correctly
//! at body size, and the axis labels are long interpolated strings that a 30px
//! `text_ellipsis` bar would truncate to noise.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::button::Button;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::window::WorkspaceShell;

pub struct ChartsPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl ChartsPanel {
    /// Frozen from B6 onward — B9's serialization key.
    pub const PANEL_NAME: &str = "ChartsPanel";
    /// Kept identical to the ids these buttons carried in the body toolbar, so
    /// the move is invisible to anything keyed on them.
    pub const EXPORT_PNG_ID: &str = "chart-export-png";
    pub const EXPORT_SVG_ID: &str = "chart-export-svg";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for ChartsPanel {}

impl Focusable for ChartsPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for ChartsPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = dat0_i18n::t("charts.title");
        div()
            .a11y_label(AccessRole::Label, title.clone())
            .child(SharedString::from(title))
    }

    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    fn visible(&self, cx: &App) -> bool {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).chart_visible())
            .unwrap_or(false)
    }

    /// Short uppercase labels rather than icons: no export-shaped icon is
    /// bundled (86 upstream icons, nearest is `external-link`), and a bare
    /// glyph loses meaning first at high contrast.
    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        let png = {
            let shell = self.shell.clone();
            Button::new(Self::EXPORT_PNG_ID)
                .label(dat0_i18n::t("chart.export.png"))
                .on_click(move |_ev, _window, app| {
                    if let Some(ws) = shell.upgrade() {
                        ws.update(app, |ws, cx| ws.export_chart(true, cx));
                    }
                })
        };
        let svg = {
            let shell = self.shell.clone();
            Button::new(Self::EXPORT_SVG_ID)
                .label(dat0_i18n::t("chart.export.svg"))
                .on_click(move |_ev, _window, app| {
                    if let Some(ws) = shell.upgrade() {
                        ws.update(app, |ws, cx| ws.export_chart(false, cx));
                    }
                })
        };
        Some(vec![png, svg])
    }
}

impl Render for ChartsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_charts_body(cx))
    }
}
```

⚠ The `chart.export.png` / `chart.export.svg` i18n values are currently the body buttons' full labels. Check what they read; if they are longer than ~6 characters, add `chart.export.png.short` = `"PNG"` and `chart.export.svg.short` = `"SVG"` and use those instead — the design specifies short uppercase labels for a 30px bar.

- [ ] **Step 5: Add the shell side**

In `window.rs`, beside `render_inspector_body`:

```rust
    /// B6: the Charts panel's element tree, extracted from the body row's
    /// `.w(px(560.))` block. The `w`/`border_l` the block carried are the
    /// dock's job now, and the two export buttons moved to
    /// `ChartsPanel::toolbar_buttons`.
    pub(crate) fn render_charts_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(self.render_chart_toolbar(cx))
            .child(crate::charts::panel::render_chart_body(
                &self.chart_panel,
                self.chart_image.clone(),
                (520.0, 360.0),
                cx,
            ))
            .into_any_element()
    }

    /// B6: the Charts panel's visibility, read by `ChartsPanel::visible`.
    pub(crate) fn chart_visible(&self) -> bool {
        self.chart_panel_visible
    }
```

- [ ] **Step 6: Strip the export buttons from the body toolbar**

In `render_chart_toolbar` (`window.rs:3927-4007`), delete the `png_btn` and `svg_btn` bindings (`:3980-3989`) and change the tail from

```rust
        row.child(png_btn).child(svg_btn).child(save_btn)
```

to

```rust
        // B6: PNG/SVG export moved to `ChartsPanel::toolbar_buttons` (the dock
        // title bar). Save stays here — its `disabled` state reads correctly at
        // body size, and it opens a name prompt rather than a file dialog.
        row.child(save_btn)
```

Also delete the now-stale comment at `:4004-4005` about clicking export with no data, and re-check whether `Disableable` is still needed by `save_btn` (it is — do not remove that import).

- [ ] **Step 7: Register the panel**

Add `pub mod charts_panel;` and a third `register_panel` arm in `panels/mod.rs`.

- [ ] **Step 8: Run the gate**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app --features a11y-capture
```

`tests/chart_uat_window.rs` and `tests/chart_panel_wiring.rs` must pass unmodified. Nothing mounts `ChartsPanel` yet, but the toolbar has genuinely lost two buttons — if a test asserted their presence in the body it goes red here, and that is a finding to report, not a diff to write.

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -s -F - <<'EOF'
feat(theme): B6 T3 — ChartsPanel + charts body extraction

Adds the thin ChartsPanel and extracts render_charts_body. The two export
buttons move from the body toolbar into Panel::toolbar_buttons (the dock title
bar); the chart-type cycle, axis cycles and Save stay in the body. Not mounted
yet.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 4: Mount the right dock and reconcile it from the bools

The visible commit. Everything before this was dead code.

**Files:**
- Modify: `crates/dat0-app/src/window.rs` — struct fields, the lazy dock block (`:6789-6804`), a new `sync_right_dock`, and the body row (`:7311-7336`)

**Interfaces:**
- Consumes: `InspectorPanel::new`, `ChartsPanel::new`, `inspector_visible()`, `chart_visible()`.
- Produces: `WorkspaceShell::right_dock_open_for_test(&self, cx) -> bool` (behind `a11y-capture`, consumed by Task 5).

- [ ] **Step 1: Add the fields**

Beside `dock_area` / `grid_panel` (`:2129-2132`):

```rust
    /// B6: the right dock's two panels, built lazily with the dock.
    inspector_panel: Option<Entity<crate::panels::inspector_panel::InspectorPanel>>,
    charts_panel: Option<Entity<crate::panels::charts_panel::ChartsPanel>>,
    /// B6: the `DockItem` handed to `set_right_dock`. Kept so a re-open can
    /// re-use the SAME `StackPanel`/`TabPanel` entities: `DockItem` is `Clone`
    /// and cloning shares the views, so the panel tree is not rebuilt.
    right_dock_item: Option<gpui_component::dock::DockItem>,
    /// B6: the (inspector, charts) visibility the dock was last reconciled to.
    /// `DockArea` exposes no public dock-size setter — `set_right_dock` is the
    /// only way in — so the dock is re-set only when this tuple changes.
    right_dock_state: (bool, bool),
```

and initialise in the constructor (`:2470`) with `inspector_panel: None, charts_panel: None, right_dock_item: None, right_dock_state: (false, false)`.

- [ ] **Step 2: Build the right dock in the lazy block**

Inside `if self.dock_area.is_none() { … }` (`:6789`), after `dock.update(.., set_center(..))`:

```rust
            // B6: the right dock. Its item must be built from `DockItem::tab`,
            // not `DockItem::panel` — `StackPanel::insert_panel` hard-asserts
            // that a split's children are TabPanel/StackPanel
            // (`stack_panel.rs:106-112`), so the 30px title bar is structural
            // here, not a style choice.
            let weak_dock = dock.downgrade();
            let inspector = cx.new(|_| {
                crate::panels::inspector_panel::InspectorPanel::new(cx.entity().downgrade())
            });
            let charts = cx
                .new(|_| crate::panels::charts_panel::ChartsPanel::new(cx.entity().downgrade()));
            let right = gpui_component::dock::DockItem::split(
                gpui::Axis::Horizontal,
                vec![
                    gpui_component::dock::DockItem::tab(inspector.clone(), &weak_dock, window, cx)
                        .size(gpui::px(288.)),
                    gpui_component::dock::DockItem::tab(charts.clone(), &weak_dock, window, cx)
                        .size(gpui::px(560.)),
                ],
                &weak_dock,
                window,
                cx,
            );
            self.inspector_panel = Some(inspector);
            self.charts_panel = Some(charts);
            self.right_dock_item = Some(right);
            self.right_dock_state = (false, false);
```

Note `cx.entity().downgrade()` is the shell's weak handle — the same value B5 binds as `weak_shell` a few lines above; reuse that binding rather than re-deriving it.

- [ ] **Step 3: Write `sync_right_dock`**

```rust
    /// B6: reconcile the right dock with the visibility bools, which are the
    /// single source of truth (design §5).
    ///
    /// This lives in `render` rather than in the toggles because
    /// `Dock::set_open` and the dock's size both need a `&mut Window`, and
    /// `toggle_chart_panel` has only a `Context`. Reconciling here also makes
    /// every `a11y-capture` shim that writes a bool directly work with no shim
    /// change at all — they write, the next frame reconciles.
    ///
    /// `set_right_dock` is the ONLY public way to set a dock's size
    /// (`DockArea::right_dock` is private and there is no `set_dock_size`), so
    /// a change re-sets the dock — passing the SAME cloned `DockItem`, which
    /// shares the panel views, so nothing in the panel tree is rebuilt.
    fn sync_right_dock(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        let want = (self.inspector_panel_visible, self.chart_panel_visible);
        if want == self.right_dock_state {
            return;
        }
        let (Some(dock), Some(item)) = (self.dock_area.clone(), self.right_dock_item.clone())
        else {
            return;
        };
        self.right_dock_state = want;

        // Widths match what the fixed docks used: 288 inspector, 560 charts.
        // A manual resize survives until the visible SET changes, at which
        // point it is recomputed — remembering it across toggles needs B9's
        // `dock_layout` blob.
        let width = match want {
            (true, true) => 848.0,
            (true, false) => 288.0,
            (false, true) => 560.0,
            (false, false) => 288.0, // closed; size is irrelevant but must be > PANEL_MIN_SIZE
        };
        let open = want.0 || want.1;
        dock.update(cx, |dock, cx| {
            dock.set_right_dock(item, Some(gpui::px(width)), open, window, cx);
        });
    }
```

Call it immediately after the lazy-build block:

```rust
        self.sync_right_dock(window, cx);
```

- [ ] **Step 4: Delete the fixed docks**

Remove both `.children(self.inspector_panel_visible.then(|| …))` and `.children(self.chart_panel_visible.then(|| …))` blocks from the body row (`window.rs:7311-7336`), leaving the trailing `,` and the `.child(div().flex_1().children(dock_el))` line intact. Update the stale comment above them, which still describes the left/right ordering of hand-rolled docks.

- [ ] **Step 5: Add the test shim**

In the `#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block, beside `dock_mounted_for_test`:

```rust
    /// B6: is the right dock open? Integration tests live in another crate and
    /// cannot see the private `dock_area`.
    pub fn right_dock_open_for_test(&self, cx: &gpui::App) -> bool {
        self.dock_area
            .as_ref()
            .map(|d| {
                d.read(cx)
                    .is_dock_open(gpui_component::dock::DockPlacement::Right, cx)
            })
            .unwrap_or(false)
    }
```

- [ ] **Step 6: Run the gate, then boot the app**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app
cargo test -p dat0-app --features a11y-capture
cargo test -p dat0-app --features a11y-capture,gallery
cargo build -p dat0-app --bin dat0
```

Then **boot it with a fresh `DAT0_CONFIG_DIR` and diff the log against a `main` build.** This is not optional: B5's first-run tour regression was invisible to five green test binaries and was found only this way. A silent success logs nothing, so "no line on main, a WARN on the branch" is the entire signal.

- [ ] **Step 7: Re-run the T0 spike and `a11y_spike`**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_chrome_spike
cargo test -p dat0-app --features a11y-capture --test a11y_spike
```

`a11y_spike`'s exact-8 count is now measured against the real shell with a real right dock. Movement is the signal — report the number either way.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -s -F - <<'EOF'
feat(theme): B6 T4 — mount the right dock, reconcile it from the bools

Mounts InspectorPanel + ChartsPanel as a horizontal split on the DockArea's
right side and deletes the two hand-rolled fixed docks from the shell body row.
The visibility bools stay the single source of truth; sync_right_dock
reconciles the dock from them at the top of render, the only place with a
guaranteed Window.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 5: Integration tests

**Files:**
- Create: `crates/dat0-app/tests/right_dock.rs`

**Interfaces:**
- Consumes: `support::A11ySnapshot`, `support::open_shell_window` pattern from `tests/a11y_content.rs:114`, the `a11y-capture` shims `seed_lineage_target_for_test`, `chart_bind_for_test`, `right_dock_open_for_test`, `dock_mounted_for_test`.

- [ ] **Step 1: Write the tests**

```rust
//! B6: the right dock — Inspector + Charts as real `DockArea` panels.
//!
//! Reuses `a11y_content.rs`'s `open_shell_window` harness (a real
//! `WorkspaceShell` under a `Root`), which is the established way to make
//! shell-level rendered-content assertions.

mod support;
```

Then one test per behavior below. Each asserts through the a11y capture, never through private state:

1. `right_dock_is_closed_when_both_panels_are_hidden` — fresh shell; `right_dock_open_for_test` is `false`; neither `"Inspector"` nor `"Charts"` appears in the capture.
2. `showing_the_inspector_opens_the_dock_and_titles_it` — call `seed_lineage_target_for_test`, run one frame, assert the dock is open and `snap.count_label("Inspector") == 1`. **The count matters**: 2 would mean the body title row survived Task 1's move.
3. `showing_charts_renders_the_charts_title_and_export_buttons` — `chart_bind_for_test`, then assert `"Charts"` is present exactly once and both export labels are present.
4. `hiding_both_panels_closes_the_dock_again` — show both, then flip both bools back, run a frame, assert closed and both titles gone. This is the reconciliation loop's only bidirectional proof.
5. `inspector_body_content_reaches_the_capture_through_the_dock` — assert on real inspector content (a column name from the seeded lineage target), not just the title. **This is the test that would catch the `.cached()` wrapper swallowing the body** while the title bar, which sits outside the cache, kept looking fine.

- [ ] **Step 2: Run them**

Run: `cargo test -p dat0-app --features a11y-capture --test right_dock`
Expected: all PASS.

- [ ] **Step 3: Prove non-vacuity, in both directions**

For each positive assertion, perturb the needle and watch it go red. For each **negative** assertion (`!has_label(..)`), swap the needle for a string that IS present and watch it go red — otherwise a negative check that never reads the tree passes forever. Revert, `touch`, re-run.

- [ ] **Step 4: Full sweep**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app
cargo test -p dat0-app --features a11y-capture
cargo test -p dat0-app --features a11y-capture,gallery
cargo test -p dat0-app --test style_lint
```

Confirm: the `style_lint` ratchet is still exactly `[("window.rs", 1)]`; `git diff main --stat -- crates/dat0-app/src/session crates/dat0-app/src/grid` is **empty**; the binary-count total is recorded (redirect to a file and count there — a `| head` pipeline SIGPIPEs cargo mid-write and truncates, A6 lesson).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -s -F - <<'EOF'
feat(theme): B6 T5 — right-dock integration tests

Covers the reconciliation loop in both directions, exact title counts (2 would
mean the inspector's body title row survived its move), the export buttons in
the title bar, and real panel-body content reaching the capture through the
.cached() wrapper.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Wrap-up (controller, not a task)

- [ ] Write the as-built section into the design doc: T0's measured numbers, every deviation from this plan, and anything the tree contradicted. Commit docs-only.
- [ ] Push, open the PR, watch **both platforms**. Poll `gh pr checks`, not `gh run watch`.
- [ ] Squash-merge with explicit `--subject` / `--body-file` — never let a commit body inherit or quote the CI skip marker.
- [ ] **Watch the post-merge main run.** Verify the bench at **step** level (reclaim → bench → upload all success — a green job can mask a skipped bench) and `gh run download` the artifact for the number. Per B5's ruling, record it and read no meaning into it: `grid_scroll` is a `render_cell` watchdog that never builds a `Window` or a `Table`.
- [ ] Record the owed human glance: two 30px title bars with ⋯ menus, the resizable divider, relocated export buttons, and scrolling where the inspector used to clip — **all three themes, high contrast most of all**. Carried in and still owed: B5's diff-the-pixels pass plus its narrow-window and file-drop specifics, and B4's palette glance.

---

## Self-review

**Spec coverage.** Design §3 (split of two tab panels) → T4 Step 2. §4 panels table → T1, T3. §4.1 title amendment → T1 Steps 3/5, verified by T5 test 2's exact count. §5 single source of truth + sync location + widths → T4 Steps 1-3. §6 export move + palette descriptors → T2, T3 Steps 4/6. §7 shell changes → T1 Step 4, T3 Step 5, T4 Step 4. §8 risks → T0 (cache), T0 Step 5 (tab order), T5 test 5 (body-through-cache). §9 test plan → T0, T5, and the per-task gates.

**Known gaps, called out rather than hidden.** (a) The `.tab_group()` risk gets a characterization test in T0 Step 5, not a preference-encoding one — there is no correct Tab order to assert against, because these panels have no focus stops until B7. (b) T3 Step 4's short-label question is resolved by reading `en.json` at execution time, not guessed here. (c) `PANEL_MIN_SIZE` is not read into the plan; the `(false, false)` width of 288 is above any plausible value, and the dock is closed in that state anyway.

**Type consistency.** `render_inspector_body` / `render_charts_body` both return `gpui::AnyElement` and are used only as `shell.update(..)` bodies. `inspector_visible()` / `chart_visible()` are both `&self -> bool`, matching `Panel::visible`'s `&self, &App` context. `InspectorPanel::new` / `ChartsPanel::new` both take `WeakEntity<WorkspaceShell>`, matching the `register_panels` placeholder arms and the T4 construction sites. `right_dock_open_for_test` takes `&gpui::App`, matching how integration tests read entities.
