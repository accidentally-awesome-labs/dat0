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
pub use capture::{A11yCapture, A11yExt, AccessRole, reset, take_tree_update};

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
