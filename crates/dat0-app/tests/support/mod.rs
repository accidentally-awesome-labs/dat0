//! Shared harness support for AccessKit content-assertion tests (UAT Gap 2).
//!
//! Included by test binaries via `mod support;`. Centralizes the capture
//! snapshot + kittest query/click combinators so every surface test
//! (Tasks 3-8) reuses ONE copy instead of re-deriving them. Hoisted here from
//! the Task-1 `a11y_spike.rs` (review Minor M2) so there is a single source.
//!
//! ## Why the `KNode` newtype (kittest 0.3.0)
//!
//! kittest 0.3.0 is the newest release compatible with the workspace's declared
//! `rust-version = "1.85"` MSRV — kittest 0.4.0 raises the floor to rustc 1.92.
//! (The installed toolchain is already 1.95; the ceiling here is the *declared
//! MSRV*, not the compiler in the box.) On 0.3.0 the querying trait `Queryable`
//! is implemented for any [`NodeT`], while [`State`] exposes only
//! `root() -> AccessKitNode` and is NOT itself `Queryable` (that is the 0.4.0
//! API the design's code sample assumed). So an integration ships a tiny
//! `NodeT` newtype over `State::root()` to get the `get_by_*` / `query_by_*`
//! helpers. `KNode` is that newtype. When the crate MSRV rises past 1.92 and
//! kittest can bump to 0.4, `State` becomes queryable and this wrapper can be
//! dropped.
//!
//! `#![allow(dead_code)]`: this is a *shared* helper library — a given test
//! binary that does `mod support;` will exercise only the combinators it needs,
//! so per-binary unused-helper warnings are expected and intentional.
#![allow(dead_code)]

use gpui::VisualTestContext;
use kittest::{AccessKitNode, NodeT, Queryable as _, State};

use dat0_app::a11y::AccessRole;

/// Minimal kittest [`NodeT`] adapter over `accesskit_consumer`'s `Node`
/// (re-exported as [`kittest::AccessKitNode`]). `State::root()` is `Copy` but
/// not `Debug`, so this newtype supplies the `Debug` + `NodeT` impls kittest's
/// blanket `Queryable` requires.
#[derive(Clone)]
pub struct KNode<'tree>(pub AccessKitNode<'tree>);

impl std::fmt::Debug for KNode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KNode")
            .field("id", &self.0.id().0)
            .field("role", &self.0.role())
            .field("label", &self.0.label())
            .field("value", &self.0.value())
            .finish()
    }
}

impl<'tree> NodeT<'tree> for KNode<'tree> {
    fn accesskit_node(&self) -> AccessKitNode<'tree> {
        self.0
    }
    fn new_related(&self, child: AccessKitNode<'tree>) -> Self {
        KNode(child)
    }
}

/// A captured AccessKit frame: the queryable [`State`] plus the
/// `NodeId → static-id` side-map that turns a label lookup back into a
/// `debug_bounds`-resolvable id.
pub struct A11ySnapshot {
    pub state: State,
    /// `click_ids[i]` is the static `.a11y(id, …)` id for `NodeId(i as u64 + 1)`,
    /// or `None` for a content-only `.a11y_label(…)` node.
    pub click_ids: Vec<Option<&'static str>>,
}

impl A11ySnapshot {
    /// Reset the collector, force ONE full re-render (`window.refresh()` bypasses
    /// gpui's per-view render cache so the view's `render` runs again), drain the
    /// frame with `run_until_parked`, and snapshot the emitted tree. This is the
    /// frame-reset bracket the Task-1 spike proved yields exactly one clean frame
    /// under `TestPlatform`.
    pub fn capture(cx: &mut VisualTestContext) -> Self {
        dat0_app::a11y::reset();
        cx.update(|window, _app| window.refresh());
        cx.run_until_parked();
        let cap = dat0_app::a11y::take_tree_update();
        Self {
            state: State::new(cap.update),
            click_ids: cap.click_ids,
        }
    }

    /// The synthetic root wrapped as a queryable [`KNode`]. Queries descend from
    /// here recursively; the captured nodes are its direct children. Escape
    /// hatch for low-level lookups when the convenience methods are not enough.
    pub fn root(&self) -> KNode<'_> {
        KNode(self.state.root())
    }

    /// Content assertion (role-agnostic): is there a captured node whose text
    /// exactly equals `label`? Matches `Role::Label` nodes by their `value` and
    /// every other role by its `label` (the value-vs-label rule), so a single
    /// call finds any `.a11y` / `.a11y_label` text regardless of role.
    ///
    /// # Panics
    /// - if **two or more** nodes match `label` (kittest's unique-match `query`
    ///   panics on duplicates). Use [`Self::has_label_any`] /
    ///   [`Self::count_label`] instead whenever the same text can appear on more
    ///   than one node (e.g. repeated grid cell values — categories, booleans,
    ///   small ints).
    pub fn has_label(&self, label: &str) -> bool {
        self.root().query_by_label(label).is_some()
    }

    /// Content assertion scoped to a role: is there a captured node with this
    /// `role` whose text exactly equals `label`? Disambiguates when the same
    /// string appears under different roles.
    ///
    /// # Panics
    /// - if **two or more** nodes match `role` + `label` (kittest's unique-match
    ///   `query` panics on duplicates). Use [`Self::count_label`] (or a
    ///   role-scoped `query_all_by_role_and_label` on [`Self::root`]) when a
    ///   role+label pair can repeat.
    pub fn query_by_role(&self, role: AccessRole, label: &str) -> bool {
        self.root()
            .query_by_role_and_label(role.to_accesskit(), label)
            .is_some()
    }

    /// Duplicate-tolerant content assertion: is there **at least one** captured
    /// node whose text exactly equals `label`? Wraps kittest's
    /// `query_all_by_label` (an iterator — never panics on duplicates), unlike
    /// [`Self::has_label`] which wraps the unique-match `query`. This is the
    /// method to use for grid cell values, which repeat across rows/columns.
    pub fn has_label_any(&self, label: &str) -> bool {
        self.count_label(label) > 0
    }

    /// How many captured nodes have text exactly equal to `label`. Wraps
    /// `query_all_by_label().count()`, so it is safe for repeated values where
    /// [`Self::has_label`] would panic. `0` means the value is absent (the
    /// teeth-check form: assert a value NOT in the fixture yields `0`).
    pub fn count_label(&self, label: &str) -> usize {
        self.root().query_all_by_label(label).count()
    }

    /// Recover the static `.a11y` id for the single node matching `label`, or
    /// `None` if that node is content-only (`.a11y_label`, no click id).
    ///
    /// # Panics
    /// - if zero or more than one node matches `label`.
    pub fn click_id_for_label(&self, label: &str) -> Option<&'static str> {
        let node = self.root().get_by_label(label);
        let nid = node.accesskit_node().id(); // NodeId(i), i >= 1 (root is 0)
        self.click_ids[(nid.0 - 1) as usize]
    }

    /// Locate a clickable node by its label, recover its static id, resolve
    /// painted geometry via `debug_bounds`, and fire a real `simulate_click` at
    /// its center. Proves the AccessKit node and the gpui hitbox stay in
    /// lockstep (same id) — no hand-tuned pixel constant.
    ///
    /// # Panics
    /// - if zero or more than one node matches `label`;
    /// - if the matched node is content-only (`.a11y_label`, not clickable);
    /// - if the id has no painted bounds.
    pub fn click(&self, cx: &mut VisualTestContext, label: &str) {
        let id = self
            .click_id_for_label(label)
            .expect("clicked node must be a clickable `.a11y(id, …)` node, not `.a11y_label`");
        let bounds = cx
            .debug_bounds(id)
            .expect("clicked node must have painted bounds resolvable by its id");
        cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    }
}
