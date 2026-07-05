//! Test-only AccessKit emission (UAT Gap 2). Gated behind `a11y-capture`; in
//! release builds the helpers are identity no-ops with zero cost. Does NOT
//! provide production accessibility (D-015 stays open).
//!
//! ## What this is
//! An element-wrapper helper `.a11y(id, role, label)` that, during a render
//! captured by the test harness, (1) pushes a node into a thread-local frame
//! collector and (2) chains the existing `.debug_selector(|| id.into())`. The
//! harness snapshots the collector into an `accesskit::TreeUpdate`, hands it to
//! `kittest::State::new`, and queries by label/role — then recovers the static
//! id and resolves geometry through `VisualTestContext::debug_bounds`. Content
//! (AccessKit node) and geometry (gpui hitbox) stay in lockstep because both are
//! keyed by the SAME `&'static str` id.
//!
//! ## Frame-reset bracket (resolved empirically by the T0 spike — Gap 2 Task 1)
//! The harness brackets a capture as:
//! `reset()` → `window.refresh()` → `run_until_parked()` → `take_tree_update()`.
//! We measured (see `tests/a11y_spike.rs`, gate 5) that this produces EXACTLY
//! one render of `WorkspaceShell` under `TestPlatform` — `debug_bounds` proves
//! the element-build + layout passes run, and the collector held the expected
//! node set with NO duplicate frames after `refresh()`. So a generation counter
//! (Step-8 fallback: bump-on-`begin_frame()`, keep only max-gen nodes) was NOT
//! needed for v1. If a future surface re-renders child views more than once per
//! forced frame and duplicates appear, reintroduce the generation counter.

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
        // The tab-stop/tab-index metadata lives on the FocusHandle, NOT the
        // element: when an explicit handle is tracked (`track_focus`), gpui's
        // paint pass does NOT copy the element-level `.tab_index()` onto it (that
        // copy only happens for an auto-created handle — div.rs:1584). A plain
        // `cx.focus_handle()` defaults to `tab_stop: false`, so it would be
        // painted but never reached by `focus_next`. Configure the handle the way
        // gpui-component's `Button` does (`fh.tab_index(..).tab_stop(true)`); the
        // builders mutate the shared `FocusRef` keyed by id, so the clone stays in
        // lockstep with the stored handle the oracle joins on.
        let fh = fh.clone().tab_index(tab_index).tab_stop(true);
        record_focus_id(&fh, id);
        self.track_focus(&fh)
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

#[cfg(feature = "a11y-capture")]
mod capture {
    use accesskit::{Node, NodeId, Role, Tree, TreeUpdate};
    use gpui::InteractiveElement;
    use std::cell::RefCell;

    /// dat0's small role vocabulary, mapped to accesskit roles. Extended in
    /// Gap 2 Task 2 to cover the surfaces Tasks 3-8 assert on (grid `Cell`/`Row`,
    /// `Dialog`/`Alert` overlays) alongside the Task-1 `Button`/`Label`.
    #[derive(Clone, Copy, Debug)]
    pub enum AccessRole {
        Button,
        Label,
        Cell,
        Row,
        Dialog,
        Alert,
    }
    impl AccessRole {
        /// Map to the accesskit role. `pub` so the shared test-support snapshot
        /// (in the integration-test crate — a different crate boundary) can build
        /// `By::new().role(..)` queries via this without having to name
        /// `accesskit::Role` directly (accesskit is not a dev-dependency).
        pub fn to_accesskit(self) -> Role {
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

    struct Captured {
        role: AccessRole,
        text: String,
        click_id: Option<&'static str>,
    }

    thread_local! {
        static FRAME: RefCell<Vec<Captured>> = const { RefCell::new(Vec::new()) };
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

    /// Clear the collector. Called by the harness before forcing a render.
    pub fn reset() {
        FRAME.with(|f| f.borrow_mut().clear());
        FOCUS.with(|f| f.borrow_mut().clear());
    }

    fn push(role: AccessRole, text: String, click_id: Option<&'static str>) {
        FRAME.with(|f| {
            f.borrow_mut().push(Captured {
                role,
                text,
                click_id,
            })
        });
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
            let child_ids: Vec<NodeId> = (0..frame.len()).map(|i| NodeId(i as u64 + 1)).collect();

            let mut root = Node::new(Role::Window);
            root.set_children(child_ids.clone());
            nodes.push((NodeId(0), root));

            for (i, c) in frame.iter().enumerate() {
                let mut n = Node::new(c.role.to_accesskit());
                // Value-vs-label rule (verified against kittest 0.3.0's
                // `By::matches`, filter.rs): a query built with `.label(x)` reads
                // `Node::value()` when `role == Role::Label` and `Node::label()`
                // for every other role. So to make ONE label-based query
                // (`has_label`) find every captured node uniformly, we store the
                // text where that role's matcher looks: `Role::Label` → `value`,
                // all other roles (Button/Cell/Row/Dialog/Alert) → `label`. This
                // is also the accesskit authoring convention. (Do NOT move
                // Cell/Row onto `set_value`: `By::matches` would then read their
                // `label()` and find nothing, silently breaking label queries.)
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

    /// Element-wrapper helpers for AccessKit capture.
    ///
    /// - [`A11yExt::a11y`] registers a **clickable** node: it records the static
    ///   `id` in the click-id side-map (so a label lookup can recover it and
    ///   resolve painted geometry via `VisualTestContext::debug_bounds`) AND
    ///   chains `debug_selector`. Use for buttons / interactive rows.
    /// - [`A11yExt::a11y_label`] registers a **content-only** node: no click id,
    ///   no `debug_selector`. Use for dynamic-id or non-interactive text (grid
    ///   cells, headings, tagline) that a test only needs to *find by content*.
    ///   `debug_bounds` takes a `&'static str`, so content nodes are not
    ///   clickable — only `.a11y(id, …)` nodes are.
    pub trait A11yExt: InteractiveElement + Sized {
        fn a11y(self, id: &'static str, role: AccessRole, label: impl Into<String>) -> Self {
            push(role, label.into(), Some(id));
            self.debug_selector(move || id.to_string())
        }

        /// Content-only capture: emits an AccessKit node with `text` under
        /// `role` but registers NO click id and chains no `debug_selector`.
        fn a11y_label(self, role: AccessRole, text: impl Into<String>) -> Self {
            push(role, text.into(), None);
            self
        }
    }
    impl<T: InteractiveElement + Sized> A11yExt for T {}
}

#[cfg(feature = "a11y-capture")]
pub use capture::{A11yCapture, A11yExt, AccessRole, focused_label, reset, take_tree_update};

// Release / no-capture stubs: identity helper, no accesskit, no debug_selector.
#[cfg(not(feature = "a11y-capture"))]
mod stub {
    // Must mirror the capture enum's full variant set: production render code
    // (empty_state.rs and the Tasks 3-8 surfaces) names `AccessRole::Cell` etc.
    // and must compile in release builds where only these stubs exist.
    #[derive(Clone, Copy, Debug)]
    pub enum AccessRole {
        Button,
        Label,
        Cell,
        Row,
        Dialog,
        Alert,
    }
    pub trait A11yExt: Sized {
        #[inline]
        fn a11y(self, _id: &'static str, _role: AccessRole, _label: impl Into<String>) -> Self {
            self
        }
        #[inline]
        fn a11y_label(self, _role: AccessRole, _text: impl Into<String>) -> Self {
            self
        }
    }
    impl<T: Sized> A11yExt for T {}
}

#[cfg(not(feature = "a11y-capture"))]
pub use stub::{A11yExt, AccessRole};
