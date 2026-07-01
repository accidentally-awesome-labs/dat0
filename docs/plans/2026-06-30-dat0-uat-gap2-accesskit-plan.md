# UAT Gap 2 — AccessKit content-assertion harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the headless gpui behavioral harness the ability to assert on *rendered text* and locate widgets *by label* (not by hand-tuned pixels), by building a test-only AccessKit tree from dat0's own render code and reading it with `kittest`.

**Architecture:** A feature-gated element-wrapper helper (`.a11y(id, role, label)` / `.a11y_label(role, text)`) pushes nodes into a thread-local frame collector during render and chains gpui's existing `.debug_selector(id)`. The collector builds an `accesskit::TreeUpdate`; the test harness wraps it in `kittest::State` to query by label/role and resolves clicks through the existing `debug_bounds(id)`. Three layers, one id. No gpui fork; capture compiles out in release.

**Tech Stack:** Rust, gpui `=0.2.2` (pinned), gpui-component `0.5.1`, `accesskit 0.24` + `accesskit_consumer 0.35` (optional lib deps), `kittest` (dev-dep), existing `#[gpui::test]` + `VisualTestContext` harness in `crates/dat0-app/tests/onboarding_gpui.rs`.

## Global Constraints

- **Pinned deps — do not bump:** gpui `=0.2.2`, gpui-macros `=0.2.2`, gpui-component `0.5.1` (SHA `0f0ab35`), duckdb `=1.4.4`.
- **No gpui fork; test-only.** Capture is gated behind the `a11y-capture` cargo feature. It is OFF in release (`cargo build`) → `.a11y*` helpers are identity no-ops, zero production cost. **D-015 (production screen-reader a11y) stays OPEN.**
- **Feature self-consistency:** `a11y-capture` MUST enable `gpui/test-support` (because `.a11y()` calls `debug_selector`, which only exists under `#[cfg(any(test, feature = "test-support"))]`).
- **`debug_bounds` takes `&'static str`** → clickable annotations use static-literal ids only; dynamic-id elements (grid cells) are content-only (no click path).
- **Determinism:** node labels are i18n strings, NodeIds are sequential, no timestamps/paths/random in the tree → byte-stable on macOS + Linux CI.
- **DCO:** every commit `git commit -s`; end the message with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- **House test discipline (teeth):** every new content assertion must be shown to FAIL when content is wrong before it is trusted.
- **CI runs `cargo test --workspace --target <triple>` with NO feature flags** → the feature must auto-activate for the test build (self-dev-dependency); do not rely on a CLI `--features`.
- Branch: `uat-gap2-accesskit` (already created off `main` `865c9b1`). Design: `docs/plans/2026-06-30-dat0-uat-gap2-accesskit-design.md`.

---

### Task 1: T0 spike — wire deps + feature, minimal capture, prove round-trip + click + frame bracket (HARD GATE)

This task is the go/no-go. It builds a *minimal but real* slice of the infra (one role, one helper), wires the feature, and empirically resolves the two unknowns the design flagged: (a) does capture-during-render fire under the test harness, (b) what is the reliable per-frame reset+read bracket given gpui view-render caching. If it cannot be made green, STOP and surface findings (do not grind).

**Files:**
- Modify: `Cargo.toml` (workspace root — `[workspace.dependencies]`)
- Modify: `crates/dat0-app/Cargo.toml` (`[dependencies]`, new `[features]`, `[dev-dependencies]`)
- Create: `crates/dat0-app/src/a11y/mod.rs`
- Modify: `crates/dat0-app/src/lib.rs` (register module)
- Modify: `crates/dat0-app/src/empty_state.rs:138` (replace the lone `debug_selector` with `.a11y`)
- Create: `crates/dat0-app/tests/a11y_spike.rs`

**Interfaces (Produced — later tasks rely on these exact names):**
- `dat0_app::a11y::A11yExt` trait with `fn a11y(self, id: &'static str, role: AccessRole, label: impl Into<String>) -> Self`.
- `dat0_app::a11y::AccessRole` enum (spike: just `Button`).
- `dat0_app::a11y::reset()`, `dat0_app::a11y::take_tree_update() -> A11yCapture` where `A11yCapture { update: accesskit::TreeUpdate, click_ids: Vec<Option<&'static str>> }`.
- Test-side: `a11y_spike::A11ySnapshot` with `get_by_label`, `click`.

- [ ] **Step 1: Add optional accesskit deps to the workspace + dat0-app, with the self-consistent feature**

Edit workspace root `Cargo.toml` `[workspace.dependencies]` — add (pin exact resolved versions via `cargo add` in Step 2):

```toml
accesskit = "0.24"
accesskit_consumer = "0.35"
kittest = "0.x"   # replace 0.x with the version cargo resolves in Step 2
```

Edit `crates/dat0-app/Cargo.toml`. Under `[dependencies]` add the optional deps:

```toml
accesskit = { workspace = true, optional = true }
accesskit_consumer = { workspace = true, optional = true }
```

Add a new `[features]` section (the feature also turns on gpui test-support so `debug_selector` exists whenever capture is on):

```toml
[features]
a11y-capture = ["dep:accesskit", "dep:accesskit_consumer", "gpui/test-support"]
```

Under `[dev-dependencies]`, add the self-reference (auto-activates the feature for this crate's integration tests with no CI flag) and kittest:

```toml
dat0-app = { path = ".", features = ["a11y-capture"] }
kittest = { workspace = true }
```

- [ ] **Step 2: Pin exact versions and verify the dep graph (no clash, additive only)**

Run:
```bash
cargo add -p dat0-app accesskit@0.24 accesskit_consumer@0.35 --optional
cargo add -p dat0-app --dev kittest
cargo tree -p dat0-app -i accesskit 2>/dev/null | head
```
Expected: accesskit appears ONLY via dat0-app's optional dep / kittest (gpui pulls none — confirms the design's "purely additive" claim). Record the resolved kittest version back into the workspace `[workspace.dependencies]` line. If any *other* crate pins a conflicting accesskit major, STOP and report.

- [ ] **Step 3: Write the minimal a11y module (collector + trait + tree builder)**

Create `crates/dat0-app/src/a11y/mod.rs`:

```rust
//! Test-only AccessKit emission (UAT Gap 2). Gated behind `a11y-capture`; in
//! release builds the helpers are identity no-ops with zero cost. Does NOT
//! provide production accessibility (D-015 stays open).

#[cfg(feature = "a11y-capture")]
mod capture {
    use accesskit::{Node, NodeId, Role, Tree, TreeUpdate};
    use gpui::InteractiveElement;
    use std::cell::RefCell;

    /// dat0's small role vocabulary, mapped to accesskit roles.
    #[derive(Clone, Copy, Debug)]
    pub enum AccessRole {
        Button,
        Label,
    }
    impl AccessRole {
        fn to_accesskit(self) -> Role {
            match self {
                AccessRole::Button => Role::Button,
                AccessRole::Label => Role::Label,
            }
        }
    }

    struct Captured {
        role: AccessRole,
        text: String,
        click_id: Option<&'static str>,
    }

    thread_local! {
        static FRAME: RefCell<Vec<Captured>> = const { RefCell::new(Vec::new()) };
    }

    /// Clear the collector. Called by the harness before forcing a render.
    pub fn reset() {
        FRAME.with(|f| f.borrow_mut().clear());
    }

    fn push(role: AccessRole, text: String, click_id: Option<&'static str>) {
        FRAME.with(|f| f.borrow_mut().push(Captured { role, text, click_id }));
    }

    /// The captured tree plus a NodeId-indexed map back to debug_selector ids.
    pub struct A11yCapture {
        pub update: TreeUpdate,
        /// `click_ids[i]` is the static id for `NodeId(i as u64 + 1)`, if clickable.
        pub click_ids: Vec<Option<&'static str>>,
    }

    /// Snapshot the collector into an accesskit TreeUpdate. Root = NodeId(0)
    /// (Role::Window); captured nodes are NodeId(1..=n) as direct children
    /// (flat tree — kittest queries do not need hierarchy).
    pub fn take_tree_update() -> A11yCapture {
        FRAME.with(|f| {
            let frame = f.borrow();
            let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(frame.len() + 1);
            let mut click_ids: Vec<Option<&'static str>> = Vec::with_capacity(frame.len());
            let child_ids: Vec<NodeId> =
                (0..frame.len()).map(|i| NodeId(i as u64 + 1)).collect();

            let mut root = Node::new(Role::Window);
            root.set_children(child_ids.clone());
            nodes.push((NodeId(0), root));

            for (i, c) in frame.iter().enumerate() {
                let mut n = Node::new(c.role.to_accesskit());
                // accesskit/kittest convention: Role::Label text lives in `value`;
                // everything else (Button…) uses `label`.
                match c.role {
                    AccessRole::Label => n.set_value(c.text.clone()),
                    _ => n.set_label(c.text.clone()),
                }
                nodes.push((NodeId(i as u64 + 1), n));
                click_ids.push(c.click_id);
            }

            let update = TreeUpdate {
                nodes,
                tree: Some(Tree::new(NodeId(0))),
                focus: NodeId(0),
            };
            A11yCapture { update, click_ids }
        })
    }

    /// Element-wrapper helper. `.a11y(id, role, label)` registers a clickable
    /// node (static id → debug_bounds-resolvable) and chains debug_selector.
    pub trait A11yExt: InteractiveElement + Sized {
        fn a11y(self, id: &'static str, role: AccessRole, label: impl Into<String>) -> Self {
            push(role, label.into(), Some(id));
            self.debug_selector(move || id.to_string())
        }
    }
    impl<T: InteractiveElement + Sized> A11yExt for T {}
}

#[cfg(feature = "a11y-capture")]
pub use capture::{reset, take_tree_update, A11yCapture, A11yExt, AccessRole};

// Release / no-capture stubs: identity helper, no accesskit, no debug_selector.
#[cfg(not(feature = "a11y-capture"))]
mod stub {
    #[derive(Clone, Copy, Debug)]
    pub enum AccessRole {
        Button,
        Label,
    }
    pub trait A11yExt: Sized {
        #[inline]
        fn a11y(self, _id: &'static str, _role: AccessRole, _label: impl Into<String>) -> Self {
            self
        }
    }
    impl<T: Sized> A11yExt for T {}
}

#[cfg(not(feature = "a11y-capture"))]
pub use stub::{A11yExt, AccessRole};
```

- [ ] **Step 4: Register the module**

Edit `crates/dat0-app/src/lib.rs` — add alongside the other `pub mod` declarations:

```rust
pub mod a11y;
```

- [ ] **Step 5: Annotate the one existing selector site**

Edit `crates/dat0-app/src/empty_state.rs`. At the top, add the import (near the other `use` lines):

```rust
use crate::a11y::{A11yExt as _, AccessRole};
```

Replace line 138 `.debug_selector(|| "hero-take-tour".into())` with:

```rust
                                .a11y("hero-take-tour", AccessRole::Button, dat0_i18n::t("hero.take_tour"))
```

(`.a11y` chains `debug_selector` internally, so the existing `hero_take_tour_button_opens_tour` test still resolves `debug_bounds("hero-take-tour")`.)

- [ ] **Step 6: Write the spike test (round-trip read + label-click), resolving the frame bracket**

Create `crates/dat0-app/tests/a11y_spike.rs`. This mirrors `onboarding_gpui.rs`'s window setup. The frame-bracket strategy: `a11y::reset()` → force a full re-render via `window.refresh()` → `run_until_parked()` → `take_tree_update()`. (If the spike finds the collector holds duplicate/partial frames, switch to a generation counter — see Step 8.)

```rust
//! T0 spike for UAT Gap 2 — proves AccessKit capture round-trips under the
//! gpui test harness and that label-located clicks reach real widgets.
use std::path::Path;
use std::sync::Arc;

use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use kittest::{Queryable as _, State};
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;

const BUDGET: u64 = 128 * 1024 * 1024;

fn build_empty_session(state_root: &Path) -> Arc<Mutex<Session>> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    Arc::new(Mutex::new(rt.block_on(Session::new(state_root, BUDGET)).unwrap()))
}

/// Reset, force one full re-render, snapshot → kittest State + click-id map.
fn a11y_snapshot(cx: &mut VisualTestContext) -> (State, Vec<Option<&'static str>>) {
    dat0_app::a11y::reset();
    cx.update(|window, _app| window.refresh());
    cx.run_until_parked();
    let cap = dat0_app::a11y::take_tree_update();
    (State::new(cap.update), cap.click_ids)
}

#[gpui::test]
#[serial]
fn a11y_capture_round_trips_and_click_by_label_opens_tour(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    // SAFETY: #[serial].
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", cfg.path()) };
    cx.update(gpui_component::init);

    let session = build_empty_session(state.path());
    let (_root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let shell = cx.new(|c| WorkspaceShell::new(session, c));
        Root::new(shell, window, cx)
    });
    vcx.run_until_parked();

    // (a) CONTENT: the hero "Take a tour" button is in the tree, by label.
    let (tree, click_ids) = a11y_snapshot(vcx);
    let node = tree.get_by_label(&dat0_app::dat0_i18n::t("hero.take_tour"));
    let nid = node.accesskit_node().id(); // NodeId(i)
    let click_id =
        click_ids[(nid.0 - 1) as usize].expect("hero-take-tour must be clickable");
    assert_eq!(click_id, "hero-take-tour");

    // (b) CLICK BY LABEL: resolve id → debug_bounds → real click opens the tour.
    let bounds = vcx.debug_bounds(click_id).expect("painted bounds");
    vcx.simulate_click(bounds.center(), gpui::Modifiers::none());
    vcx.run_until_parked();
    // open_deferred hops the dispatcher; for the spike, assert via the same
    // observable used by onboarding_gpui.rs (dialog presence) after draining.
    // (If the spike only needs to prove capture+bounds, asserting the bounds
    // resolved + click fired is sufficient; tour-open is re-tested in Task 3.)

    drop(state);
}
```

- [ ] **Step 7: Run the spike — both feature states must compile, the test must pass**

Run:
```bash
cargo build -p dat0-app --release            # feature OFF → .a11y is no-op, must compile
cargo test -p dat0-app --test a11y_spike -- --nocapture
```
Expected: release build succeeds (no accesskit, no debug_selector referenced); the spike test passes — `get_by_label` finds the button, the click-id round-trips to `"hero-take-tour"`, and `debug_bounds` resolves.

- [ ] **Step 8: Resolve the frame bracket; record findings**

If `take_tree_update()` returned duplicate nodes (the forced `refresh()` produced >1 render frame) or missing nodes (shell did not re-render), adjust the collector to a **generation counter**: add `static GEN: Cell<u64>`, a `begin_frame()` that bumps it (called at the top of `WorkspaceShell::render`), tag each `Captured` with the current gen, and have `take_tree_update()` keep only max-gen nodes. Add `begin_frame()` to `WorkspaceShell::render` under `#[cfg(feature = "a11y-capture")]`. Re-run Step 7. Write the resolved bracket strategy into the module doc-comment.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml crates/dat0-app/Cargo.toml crates/dat0-app/src/a11y/ \
        crates/dat0-app/src/lib.rs crates/dat0-app/src/empty_state.rs \
        crates/dat0-app/tests/a11y_spike.rs Cargo.lock
git commit -s -m "feat(a11y): T0 spike — AccessKit capture round-trips in gpui harness (Gap 2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

**GATE:** if Steps 7–8 cannot be made green, STOP. Record findings in the design doc's Risks section and return the gap to human-UAT (as Gap 1 was). Do not proceed to Task 2.

---

### Task 2: Finalize the capture API — content-only helper, full role map, query/click ergonomics

**Files:**
- Modify: `crates/dat0-app/src/a11y/mod.rs`
- Create: `crates/dat0-app/tests/support/mod.rs` (shared harness helpers — or inline into each test; see Step 4)
- Modify: `crates/dat0-app/tests/a11y_spike.rs` (extend to two-node content read)

**Interfaces (Produced):**
- `A11yExt::a11y_label(self, role: AccessRole, text: impl Into<String>) -> Self` — content-only (no click id, no debug_selector). For dynamic-id / non-clickable text.
- `AccessRole` extended: `Button, Label, Cell, Row, Dialog, Alert`.
- Test-support `A11ySnapshot { state: kittest::State, click_ids: Vec<Option<&'static str>> }` with methods: `get_by_label(&str)`, `query_by_label(&str) -> Option<_>`, `query_by_role(AccessRole, &str) -> Option<_>`, `click(cx, By)`.

- [ ] **Step 1: Write a failing test for the content-only helper**

Append to `crates/dat0-app/tests/a11y_spike.rs`:

```rust
#[gpui::test]
#[serial]
fn a11y_label_captures_static_text_for_content_assertion(cx: &mut TestAppContext) {
    // A view that renders a plain label via .a11y_label must be findable by
    // value/label even though it is NOT clickable (no debug_selector).
    // (Full surface coverage is Tasks 3-9; this proves the content-only path.)
    // ...build window over empty session as in the first spike test...
    // let (tree, _ids) = a11y_snapshot(vcx);
    // assert!(tree.query_by_label(&dat0_app::dat0_i18n::t("hero.tagline")).is_some());
}
```

- [ ] **Step 2: Run it, watch it fail**

Run: `cargo test -p dat0-app --test a11y_spike a11y_label_captures -- --nocapture`
Expected: FAIL (`a11y_label` does not exist / tagline not annotated yet).

- [ ] **Step 3: Add `a11y_label` + extend `AccessRole`**

In `crates/dat0-app/src/a11y/mod.rs` `capture` module, extend the enum and mapping:

```rust
#[derive(Clone, Copy, Debug)]
pub enum AccessRole { Button, Label, Cell, Row, Dialog, Alert }

impl AccessRole {
    fn to_accesskit(self) -> Role {
        match self {
            AccessRole::Button => Role::Button,
            AccessRole::Label => Role::Label,
            AccessRole::Cell => Role::Cell,
            AccessRole::Row => Role::Row,
            AccessRole::Dialog => Role::Dialog,
            AccessRole::Alert => Role::Alert,
        }
    }
}
```

Add to the `A11yExt` trait (content-only — pushes a node with no click id, no debug_selector):

```rust
    fn a11y_label(self, role: AccessRole, text: impl Into<String>) -> Self {
        push(role, text.into(), None);
        self
    }
```

Mirror `a11y_label` in the `stub` module (identity returning `self`). Map `Cell`/`Row`/`Dialog`/`Alert` text via `set_label` except `Label` which uses `set_value` (already handled by the `match` in `take_tree_update` — extend so `Cell`/`Row` also use `set_value` if you want value-based queries; keep `Label` + `Cell` + `Row` on `set_value`, `Button`/`Dialog`/`Alert` on `set_label`). Adjust the `take_tree_update` match accordingly and document the value-vs-label rule.

- [ ] **Step 4: Build the shared test-support snapshot/query/click API**

Create `crates/dat0-app/tests/support/mod.rs` (included by test files via `mod support;`). It centralizes the snapshot + combinators so every surface test reuses them:

```rust
//! Shared harness support for AccessKit content-assertion tests (Gap 2).
use gpui::VisualTestContext;
use kittest::{by, By, Queryable as _, Role, State};

pub struct A11ySnapshot {
    pub state: State,
    pub click_ids: Vec<Option<&'static str>>,
}

impl A11ySnapshot {
    /// Reset, force a render, snapshot the captured tree.
    pub fn capture(cx: &mut VisualTestContext) -> Self {
        dat0_app::a11y::reset();
        cx.update(|window, _app| window.refresh());
        cx.run_until_parked();
        let cap = dat0_app::a11y::take_tree_update();
        Self { state: State::new(cap.update), click_ids: cap.click_ids }
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.state.query_by_label(label).is_some()
    }

    /// Locate a node by `By`, recover its static id, click it via debug_bounds.
    pub fn click(&self, cx: &mut VisualTestContext, filter: By) {
        let node = self.state.get_by(filter);
        let nid = node.accesskit_node().id();
        let id = self.click_ids[(nid.0 - 1) as usize]
            .expect("clicked node must be a clickable .a11y(id,…) node");
        let bounds = cx.debug_bounds(id).expect("painted bounds for clicked node");
        cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    }
}

/// Re-export the kittest query builder for tests.
pub fn label(s: &str) -> By<'_> { by().label(s) }
pub fn role_label<'a>(role: Role, s: &'a str) -> By<'a> { by().role(role).label(s) }
```

(If the project convention disallows a `tests/support/` shared module, inline these helpers into `a11y_spike.rs` and `#[path]`-include them; verify with `cargo test` either way.)

- [ ] **Step 5: Annotate the hero tagline (content-only proof) and finish the test**

Edit `crates/dat0-app/src/empty_state.rs:130`: change `.child(div().flex_grow().child(dat0_i18n::t("hero.tagline")))` so the inner div carries a content label:

```rust
                        .child(div().flex_grow()
                            .a11y_label(AccessRole::Label, dat0_i18n::t("hero.tagline"))
                            .child(dat0_i18n::t("hero.tagline")))
```

Fill in the Step 1 test body to build the window and assert `tree.query_by_label(t("hero.tagline")).is_some()`.

- [ ] **Step 6: Run — pass**

Run: `cargo test -p dat0-app --test a11y_spike -- --nocapture`
Expected: both spike tests pass.

- [ ] **Step 7: Teeth — prove the content assertion fails on wrong text**

Temporarily change the assertion to `query_by_label("THIS TEXT IS NOT RENDERED")` and confirm `is_some()` is now false (test fails). Revert.

- [ ] **Step 8: Commit**

```bash
git add crates/dat0-app/src/a11y/mod.rs crates/dat0-app/src/empty_state.rs \
        crates/dat0-app/tests/a11y_spike.rs crates/dat0-app/tests/support/
git commit -s -m "feat(a11y): content-only helper + role map + snapshot/query/click API (Gap 2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Onboarding carousel — annotate panel + buttons, un-ignore Next/Back, label-click Skip

**Files:**
- Modify: `crates/dat0-app/src/onboarding/mod.rs` (`present_panel`, lines 92–182)
- Modify: `crates/dat0-app/tests/onboarding_gpui.rs` (un-ignore the dead-end test; convert the `(777,550)` Skip click)

**Interfaces (Consumes):** `A11yExt::{a11y, a11y_label}`, `AccessRole`, `support::A11ySnapshot`, `support::label`.

- [ ] **Step 1: Annotate `present_panel` text + buttons**

Edit `crates/dat0-app/src/onboarding/mod.rs`. Add `use crate::a11y::{A11yExt as _, AccessRole};`. Then:
- Line 181 title: `.child(div().text_xl().a11y_label(AccessRole::Label, title.clone()).child(title.clone()))`
- Line 182 body: `.child(div().a11y_label(AccessRole::Label, body.clone()).child(body.clone()))`
- Skip button (line 104): add `.a11y("tour-skip", AccessRole::Button, dat0_i18n::t("onboarding.tour.skip"))` to the button builder (the button is a gpui-component `Button`; if it is NOT an `InteractiveElement`/`Div`, wrap it in an annotated `div().a11y(...)` parent, OR annotate on the surrounding container — verify the button type accepts `.a11y`; gpui-component `Button` may need wrapping). Mirror for Back (`tour-back`, line 131), Next (`tour-next`, line 115), Get-started (`tour-get-started`, line 113).

> NOTE: gpui-component `Button` may not impl gpui's `InteractiveElement`. If `.a11y` does not apply to `Button`, wrap each button: `div().a11y("tour-skip", AccessRole::Button, label_text).child(Button::new(...))`. Confirm during implementation; the wrapping div carries both the debug_selector and the node.

- [ ] **Step 2: Write the failing un-ignored Next/Back content test**

In `crates/dat0-app/tests/onboarding_gpui.rs`, add `mod support;` at the top and replace the `#[ignore]` stub `carousel_next_back_navigation_is_human_uat` (lines 687–692) with a real test:

```rust
#[gpui::test]
#[serial]
fn carousel_next_advances_panel_text(cx: &mut TestAppContext) {
    use support::{A11ySnapshot, label};
    use kittest::Queryable as _;
    // ...standard setup: cfg/state dirs, init_components, build empty session,
    // open_shell_window, run_until_parked...
    cx.cx.update(dat0_app::onboarding::open);
    cx.run_until_parked();

    // Panel 0 headline is rendered.
    let p1_title = dat0_app::dat0_i18n::t(dat0_app::onboarding::panels::PANELS[0].title_key);
    let p2_title = dat0_app::dat0_i18n::t(dat0_app::onboarding::panels::PANELS[1].title_key);
    let snap = A11ySnapshot::capture(cx);
    assert!(snap.has_label(&p1_title), "panel 0 headline must render");
    assert!(!snap.has_label(&p2_title), "panel 1 headline must NOT render yet");

    // Click Next by label → panel 1 headline now renders.
    snap.click(cx, label(&dat0_app::dat0_i18n::t("onboarding.tour.next")));
    cx.run_until_parked();
    let snap2 = A11ySnapshot::capture(cx);
    assert!(snap2.has_label(&p2_title), "Next must advance to panel 1 headline");
}
```

(Requires `panels` + `PANELS` to be reachable: add `pub mod panels;` / `pub use` in `onboarding/mod.rs` if not already public.)

- [ ] **Step 3: Run it — fail, then pass after annotation**

Run: `cargo test -p dat0-app --test onboarding_gpui carousel_next_advances -- --nocapture`
Expected: passes once Step 1 annotations are in. If it fails because the panel title isn't captured, re-check `a11y_label` is on the rendered div.

- [ ] **Step 4: Convert the Skip pixel-click to a label-click**

In `skip_click_dismisses_and_writes_flag` (line 513), replace the `(777,550)` block (lines 542–547) with:

```rust
    cx.executor().advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    let snap = support::A11ySnapshot::capture(cx);
    snap.click(cx, support::label(&dat0_app::dat0_i18n::t("onboarding.tour.skip")));
    cx.run_until_parked();
```

Keep the existing post-click asserts (dialog dismissed + `first_run_done` persisted). Run: `cargo test -p dat0-app --test onboarding_gpui skip_click -- --nocapture` → pass.

- [ ] **Step 5: Teeth + commit**

Teeth: temporarily change `p2_title` assertion to expect panel 1 BEFORE clicking Next — confirm it fails. Revert. Then:
```bash
git add crates/dat0-app/src/onboarding/ crates/dat0-app/tests/onboarding_gpui.rs
git commit -s -m "test(a11y): onboarding carousel content assertions + label-located Skip (Gap 2)

Un-ignores the per-panel Next/Back dead-end; retires the (777,550) pixel click.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Empty-state hero — annotate sample cards, label-click the sample import

**Files:**
- Modify: `crates/dat0-app/src/empty_state.rs` (sample card render, ~lines 208–235)
- Modify: `crates/dat0-app/tests/onboarding_gpui.rs` (`hero_sample_click_imports_bundled_csv`, convert `(1700,40)`)

- [ ] **Step 1: Annotate sample cards with stable static ids**

In `empty_state.rs` the sample column builds cards in a loop (titles at line 223 `.child(div().child(title))`). Sample ids must be `&'static str` for `debug_bounds`. Since samples are a fixed known set, map each sample to a static id (e.g. a `match sample.kind { Iris => "hero-sample-iris", ... }` returning `&'static str`). Annotate each card's clickable container: `.a11y(sample_static_id(kind), AccessRole::Button, title.clone())`. Annotate the card title text too with `.a11y_label(AccessRole::Label, title.clone())` for content assertions.

- [ ] **Step 2: Convert the `(1700,40)` click in the sample-import test**

In `hero_sample_click_imports_bundled_csv` (line 588), replace the `(1700,40)` `simulate_click` (line 618) with a label/id-located click:

```rust
    let snap = support::A11ySnapshot::capture(cx);
    snap.click(cx, support::label(&dat0_app::dat0_i18n::t("hero.sample.iris"))); // or by static id
```

(Use whichever label the Iris card renders; if titles are dynamic, click by the static id via a `by()` that filters the known node — simplest is to keep a direct `debug_bounds("hero-sample-iris")` since the id is now annotated.) Keep the rest of the async-pump assertions unchanged.

- [ ] **Step 3: Run + teeth + commit**

Run: `cargo test -p dat0-app --test onboarding_gpui hero_sample_click -- --nocapture` → pass (the import still completes; only the click locator changed). Teeth: point the click at a non-existent label/id, confirm it panics/fails. Revert.
```bash
git add crates/dat0-app/src/empty_state.rs crates/dat0-app/tests/onboarding_gpui.rs
git commit -s -m "test(a11y): label/id-located hero sample click; annotate sample cards (Gap 2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Grid cells — content assertion on rendered cell values

**Files:**
- Modify: `crates/dat0-app/src/grid/mod.rs` (`render_td`, line 441; value child at line 582)
- Create/Modify: a test that imports a tiny known table and asserts a cell's displayed value (extend `crates/dat0-app/tests/onboarding_gpui.rs` or a new `crates/dat0-app/tests/a11y_content.rs`).

- [ ] **Step 1: Annotate the cell value (content-only — dynamic ids → no click path)**

In `grid/mod.rs` `render_td`, where the real value is placed (line 582 `el.child(display.text)`), add a content node:

```rust
            el.a11y_label(crate::a11y::AccessRole::Cell, display.text.clone())
              .child(display.text)
```

Add `use crate::a11y::A11yExt as _;` to the file. (The placeholder em-dash branch at line 585 is left unannotated — it carries no real content.)

- [ ] **Step 2: Failing test — assert a known cell value renders**

Use the Gap-3 async harness pattern to import a tiny CSV (`a,b\n1,2\n`), bind the grid, prefetch page 0, then snapshot and assert the cell value "1" (and "2") are present as `Cell` nodes:

```rust
#[gpui::test]
#[serial]
fn grid_renders_cell_values_as_a11y_cells(cx: &mut TestAppContext) {
    // enter_async_harness, import CSV via handle_drop (block_test), bind grid,
    // prefetch_visible_rows, run_until_parked...
    let snap = support::A11ySnapshot::capture(cx);
    assert!(snap.has_label("1") || snap.state.query_by_value("1").is_some(),
        "cell value 1 must render");
}
```

(Cells use `set_value`, so query via `query_by_value` or `by().role(Role::Cell).value("1")`.)

- [ ] **Step 3: Run (fail → pass), teeth, commit**

Run the test; it fails before annotation, passes after. Teeth: assert a value NOT in the table, confirm absent. Commit:
```bash
git add crates/dat0-app/src/grid/mod.rs crates/dat0-app/tests/
git commit -s -m "test(a11y): grid cell values as AccessKit Cell nodes + content assertion (Gap 2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Inspector — content assertion on field rows

**Files:**
- Modify: `crates/dat0-app/src/inspector/panel.rs` (`render_inspector` line 26: title 36, overview 49, lineage 78/82/100; `column_card` line 161: header 185, stats 193, distinct 195, null 196)
- Modify/extend the content test file from Task 5.

- [ ] **Step 1: Annotate inspector text sites** with `.a11y_label(AccessRole::Label, <text>)` at each `SharedString::from(...)` child. Add `use crate::a11y::A11yExt as _;`. Example for overview (line 49): `.child(div().a11y_label(AccessRole::Label, overview.clone()).child(SharedString::from(overview)))`.

- [ ] **Step 2: Failing test** — open the inspector over a known table, snapshot, assert the overview line ("name — N rows · M cols") and a column header are present by `query_by_label`/`label_contains`. Use `label_contains` for the formatted strings.

- [ ] **Step 3: Run (fail → pass), teeth, commit** with message `test(a11y): inspector field content assertions (Gap 2)` + DCO/Co-Authored-By trailer.

---

### Task 7: SQL results — content assertion on result/status/timing text

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs` (`SqlConsole::render` ResultRegion: timing 906–908, status 781, error 793, cancelled 810, running 774)
- Modify/extend the content test file.

- [ ] **Step 1: Annotate** each region's text child with `.a11y_label(AccessRole::Label, <text>)` (the timing chip's `format!` string, the status/error/cancelled/running strings). Add `use crate::a11y::A11yExt as _;`. For result-grid cells, the grid annotation from Task 5 already covers them if the results pane reuses the grid delegate (verify; if it uses a separate delegate, annotate that cell fn too).

- [ ] **Step 2: Failing test** — run a trivial query (`SELECT 1 AS x`), drive to completion (async harness), snapshot, assert the timing chip text (`label_contains("ms")`) and the result cell value "1" render.

- [ ] **Step 3: Run (fail → pass), teeth, commit** with message `test(a11y): SQL console result/status/timing content assertions (Gap 2)` + trailer.

---

### Task 8: Error banner — content assertion on user-facing message

**Files:**
- Modify: `crates/dat0-app/src/error_ux/banner.rs` (`render_banner` line 196: title 235, body 237)
- Modify/extend the content test file.

- [ ] **Step 1: Annotate** the banner title (line 235 `.child(b.title.clone())`) as `AccessRole::Alert` and the body (line 237) as `AccessRole::Label`: e.g. `.a11y_label(AccessRole::Alert, b.title.clone()).child(b.title.clone())`. Add the import.

- [ ] **Step 2: Failing test** — trigger a banner (e.g., a forward-incompat / recover banner via the production path, or push a `Banner` into the shell in-test if a constructor is exposed), snapshot, assert the banner title text renders as an `Alert` node (`by().role(Role::Alert).label(<title>)`).

- [ ] **Step 3: Run (fail → pass), teeth, commit** with message `test(a11y): error banner content assertions (Gap 2)` + trailer.

---

### Task 9: Wrap-up — NOTICE, deferrals note, full gate, memory

**Files:**
- Modify: `NOTICE.md` (regen)
- Modify: `docs/deferrals.md` (D-015 note)
- Modify: `crates/dat0-app/src/a11y/mod.rs` (final doc-comment)

- [ ] **Step 1: Regenerate NOTICE and confirm the drift gate is satisfied.** New deps are optional/dev-only (off in the release binary), so cargo-about may not add entries — but the project has a NOTICE drift gate (`.github/workflows/notice.yml`) that has reddened CI before. Run the project's NOTICE regen (check `xtask`/`cargo about generate` per `docs/ci.md`), commit any diff. If cargo-about's config scans optional deps, ensure accesskit/accesskit_consumer/kittest license lines are present.

```bash
# whatever the repo uses, e.g.:
cargo about generate about.hbs -o NOTICE.md   # confirm exact command from docs/ci.md
```

- [ ] **Step 2: Annotate D-015 in `docs/deferrals.md`** — append a dated note to the D-015 entry: Gap 2 added a *test-only* AccessKit emitter (feature `a11y-capture`); D-015 (production screen-reader exposure) remains OPEN because there is still no OS platform adapter / gpui integration, but the dat0-side `.a11y` node annotations are a reusable down-payment if the gpui pin ever ships an AccessKit adapter.

- [ ] **Step 3: Full workspace gate (mirror CI locally).**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release        # feature OFF path compiles clean
.github/workflows i18n-check equivalent  # run the repo's i18n-check if locally runnable
```
Expected: all green. Fix any fmt/clippy drift (per the dev-workflow lesson: per-task gates omit `cargo fmt --check`; catch it here).

- [ ] **Step 4: Commit + update memory**

```bash
git add NOTICE.md docs/deferrals.md crates/dat0-app/src/a11y/mod.rs
git commit -s -m "docs(a11y): NOTICE regen + D-015 note + final gate (Gap 2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```
Update `memory/dat0-uat-automation-research.md`: Gap 2 status → built/merged, mechanism summary, D-015 still open.

---

## Self-Review

**1. Spec coverage:**
- Emit layer (`.a11y`/`.a11y_label`, collector, TreeUpdate) → Tasks 1–2. ✓
- Read layer (kittest State + queries) → Tasks 1–2 (support module). ✓
- Click layer (debug_bounds, label-located) → Tasks 1–2 (`A11ySnapshot::click`), exercised in 3–4. ✓
- One id keys both layers → `.a11y` chains `debug_selector` + stores click-id; verified Task 1 Step 6. ✓
- Five surfaces (dialogs/onboarding, grid, inspector, SQL, errors) → Tasks 3, 5, 6, 7, 8 (+ hero in 4). ✓
- Un-ignore Next/Back; convert (777,550) + (1700,40) → Tasks 3, 4. ✓
- Feature-gated, off in release, no fork, D-015 open → Task 1 (feature wiring), Task 9 (deferrals note); release-build check in 1/9. ✓
- T0 spike HARD GATE → Task 1. ✓
- Determinism / teeth → every surface task has a teeth step; Global Constraints. ✓
- NOTICE regen (CI-failure risk) → Task 9. ✓

**2. Placeholder scan:** Spike test Step 6 leaves the tour-open assertion as a documented option (capture+bounds proof is the gate); Tasks 6–8 give exact files/lines/annotation pattern and test intent but compress the boilerplate window-setup that Tasks 1–5 spell out in full — acceptable because the setup is identical and shown verbatim earlier (DRY), not omitted detail. No "TBD"/"handle errors"/"similar to" hand-waves on novel logic.

**3. Type consistency:** `AccessRole` (Task 1 → extended Task 2), `A11yExt::a11y`/`a11y_label`, `take_tree_update() -> A11yCapture{update, click_ids}`, `A11ySnapshot{state, click_ids}` with `has_label`/`click`/`query_by_*`, NodeId(i) ↔ `click_ids[i-1]` indexing — consistent across all tasks. kittest `By`/`by()`/`Queryable` usage matches the verified API. accesskit `Node::new`/`set_label`/`set_value`/`set_children`/`Tree::new`/`TreeUpdate{nodes,tree,focus}` matches accesskit 0.24.1.

**Open items deliberately deferred to implementation (flagged in-task, not placeholders):** exact frame-bracket (Task 1 Step 8 resolves empirically); whether gpui-component `Button` accepts `.a11y` or needs a wrapping div (Task 3 Step 1 note); exact NOTICE regen command (Task 9 Step 1 — read from `docs/ci.md`).
