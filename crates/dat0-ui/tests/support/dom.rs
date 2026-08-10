//! A queryable mirror of the VirtualDom, built from the mutation stream.
//!
//! Dioxus ships no component-test harness, but it does ship the seam a renderer
//! uses: [`WriteMutations`]. A renderer is just something that applies those
//! mutations to a tree; this is one that applies them to a `Vec<Node>` a test
//! can look at.
//!
//! The mutation protocol is stack-based and its semantics are not obvious, so
//! they are restated here — each is mirrored from
//! `dioxus-interpreter-js`'s `core.ts` / `native.ts`, which is the reference
//! implementation:
//!
//! | Mutation | Effect |
//! |---|---|
//! | `load_template(t, i, id)` | clone root `i` of `t`, **push**, bind `id` |
//! | `create_text_node` / `create_placeholder` | create, **push**, bind `id` |
//! | `push_root(id)` | **push** the node already bound to `id` |
//! | `assign_node_id(path, id)` | bind `id` to the node at `path` **under the top of the stack** |
//! | `append_children(id, m)` | **pop** `m`, append to `id` |
//! | `replace_node_with(id, m)` | **pop** `m`, put them where `id` was |
//! | `replace_placeholder_with_nodes(path, m)` | **pop** `m`, put them where the node at `path` under the stack top was |
//! | `insert_nodes_{after,before}(id, m)` | **pop** `m`, splice around `id` |
//!
//! The subtlety that costs an afternoon if missed: `load_template` materialises
//! a whole subtree but binds an `ElementId` only to its root. Every interior
//! node that Dioxus later wants to address gets its id afterwards, by *path*,
//! relative to whatever is on top of the stack. So the mirror needs its own
//! node handles independent of `ElementId`, with `ElementId` as a side index.

use std::collections::{BTreeMap, HashMap};

use dioxus_core::{AttributeValue, ElementId, Template, TemplateAttribute, TemplateNode};

/// A handle into the mirror's arena. Distinct from [`ElementId`], which Dioxus
/// assigns only to the nodes it needs to address later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKey(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Element {
        tag: String,
    },
    Text,
    /// Dioxus's re-entrance point for list diffing. Renders as nothing.
    Placeholder,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub attrs: BTreeMap<String, String>,
    pub text: String,
    pub children: Vec<NodeKey>,
    pub parent: Option<NodeKey>,
    /// Event names with a live listener, e.g. `"click"`, `"keydown"`.
    pub listeners: Vec<String>,
    /// Set when the node is detached, so stale [`NodeKey`]s in a caller's hands
    /// fail loudly rather than reading a resurrected slot.
    pub removed: bool,
}

impl Node {
    fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            attrs: BTreeMap::new(),
            text: String::new(),
            children: Vec::new(),
            parent: None,
            listeners: Vec::new(),
            removed: false,
        }
    }

    pub fn tag(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Element { tag } => Some(tag),
            _ => None,
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
}

/// The mirror.
#[derive(Debug, Default)]
pub struct Dom {
    arena: Vec<Node>,
    /// Top-level nodes, in order.
    pub roots: Vec<NodeKey>,
    /// The mutation stack.
    stack: Vec<NodeKey>,
    /// `ElementId` -> arena handle. `ElementId(0)` is the mount root, which has
    /// no mirror node; it is where `append_children` puts the initial tree.
    ids: HashMap<usize, NodeKey>,
    /// Mutations applied since construction.
    ///
    /// This is how the harness knows the tree has settled. The obvious
    /// alternative — call `render_immediate_to_vec` and check for an empty
    /// edit list — is a trap: that method *performs* the render and hands you
    /// the edits, so a following `render_immediate(&mut dom)` finds nothing
    /// dirty and the mirror silently never updates. Every assertion then reads
    /// the pre-event tree and the suite passes on stale data.
    edits: usize,
}

impl Dom {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mutations applied since construction. See the field docs.
    pub fn edit_count(&self) -> usize {
        self.edits
    }

    pub fn get(&self, key: NodeKey) -> &Node {
        &self.arena[key.0]
    }

    pub fn by_element_id(&self, id: ElementId) -> Option<NodeKey> {
        self.ids.get(&id.0).copied()
    }

    /// The `ElementId` bound to a mirror node, if any. Needed to dispatch an
    /// event: `Runtime::handle_event` addresses nodes by `ElementId`.
    pub fn element_id_of(&self, key: NodeKey) -> Option<ElementId> {
        self.ids
            .iter()
            .find(|(_, k)| **k == key)
            .map(|(id, _)| ElementId(*id))
    }

    /// Every live node, in document order.
    pub fn walk(&self) -> Vec<NodeKey> {
        let mut out = Vec::new();
        for r in &self.roots {
            self.walk_into(*r, &mut out);
        }
        out
    }

    fn walk_into(&self, key: NodeKey, out: &mut Vec<NodeKey>) {
        if self.arena[key.0].removed {
            return;
        }
        out.push(key);
        for c in self.arena[key.0].children.clone() {
            self.walk_into(c, out);
        }
    }

    /// Text of a subtree — what a reader would announce, not what the markup
    /// happens to contain.
    ///
    /// Fragments from separate text nodes are joined with a space, and runs of
    /// whitespace collapse. `textContent` would give `"dat0Run1,048,576 rows"`
    /// for a heading, a button and a span; that string is technically the
    /// concatenation and useless to assert on.
    pub fn text_of(&self, key: NodeKey) -> String {
        let mut parts = Vec::new();
        self.text_into(key, &mut parts);
        parts
            .iter()
            .flat_map(|p| p.split_whitespace())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn text_into(&self, key: NodeKey, out: &mut Vec<String>) {
        let n = &self.arena[key.0];
        if n.removed {
            return;
        }
        if matches!(n.kind, NodeKind::Text) && !n.text.trim().is_empty() {
            out.push(n.text.clone());
        }
        for c in &n.children {
            self.text_into(*c, out);
        }
    }

    // ── internals ────────────────────────────────────────────────────────────

    fn push_node(&mut self, node: Node) -> NodeKey {
        self.arena.push(node);
        NodeKey(self.arena.len() - 1)
    }

    /// Materialise a template subtree. Returns its root handle.
    fn build(&mut self, node: &TemplateNode) -> NodeKey {
        match node {
            TemplateNode::Element {
                tag,
                attrs,
                children,
                ..
            } => {
                let key = self.push_node(Node::new(NodeKind::Element {
                    tag: tag.to_string(),
                }));
                for a in attrs.iter() {
                    if let TemplateAttribute::Static { name, value, .. } = a {
                        self.arena[key.0]
                            .attrs
                            .insert(name.to_string(), value.to_string());
                    }
                }
                for c in children.iter() {
                    let ck = self.build(c);
                    self.arena[ck.0].parent = Some(key);
                    self.arena[key.0].children.push(ck);
                }
                key
            }
            TemplateNode::Text { text } => {
                let key = self.push_node(Node::new(NodeKind::Text));
                self.arena[key.0].text = text.to_string();
                key
            }
            // A dynamic slot starts life as a placeholder; the diff replaces it.
            _ => self.push_node(Node::new(NodeKind::Placeholder)),
        }
    }

    /// `loadChild`: walk `path` as child indices from the top of the stack.
    fn load_child(&self, path: &[u8]) -> NodeKey {
        let mut node = *self.stack.last().expect("loadChild with an empty stack");
        for step in path {
            node = self.arena[node.0].children[*step as usize];
        }
        node
    }

    fn take(&mut self, m: usize) -> Vec<NodeKey> {
        let at = self.stack.len() - m;
        self.stack.split_off(at)
    }

    /// Unlink each node from wherever it currently sits.
    ///
    /// A DOM node exists in exactly one place: `parent.append(x)` and
    /// `ref.before(x)` **move** `x` rather than copying it. Dioxus relies on
    /// that when it reorders a keyed list — it pushes the existing nodes and
    /// re-inserts them — so a mirror that only inserts ends up with every
    /// reordered row twice.
    fn detach_all(&mut self, nodes: &[NodeKey]) {
        for n in nodes {
            if self.detach(*n).is_none() {
                self.roots.retain(|r| r != n);
                self.arena[n.0].parent = None;
            }
        }
    }

    fn detach(&mut self, key: NodeKey) -> Option<(NodeKey, usize)> {
        let parent = self.arena[key.0].parent?;
        let ix = self.arena[parent.0]
            .children
            .iter()
            .position(|c| *c == key)?;
        self.arena[parent.0].children.remove(ix);
        Some((parent, ix))
    }

    fn splice_at(&mut self, parent: Option<NodeKey>, ix: usize, nodes: &[NodeKey]) {
        match parent {
            Some(p) => {
                for (o, n) in nodes.iter().enumerate() {
                    self.arena[n.0].parent = Some(p);
                    self.arena[p.0].children.insert(ix + o, *n);
                }
            }
            None => {
                for (o, n) in nodes.iter().enumerate() {
                    self.arena[n.0].parent = None;
                    self.roots.insert(ix + o, *n);
                }
            }
        }
    }

    /// Position of a node among its siblings, and its parent (`None` = a root).
    fn locate(&self, key: NodeKey) -> (Option<NodeKey>, usize) {
        match self.arena[key.0].parent {
            Some(p) => (
                Some(p),
                self.arena[p.0]
                    .children
                    .iter()
                    .position(|c| *c == key)
                    .expect("a child knows its parent"),
            ),
            None => (
                None,
                self.roots
                    .iter()
                    .position(|c| *c == key)
                    .expect("a parentless node is a root"),
            ),
        }
    }

    fn mark_removed(&mut self, key: NodeKey) {
        self.arena[key.0].removed = true;
        for c in self.arena[key.0].children.clone() {
            self.mark_removed(c);
        }
    }
}

fn attr_text(value: &AttributeValue) -> Option<String> {
    match value {
        AttributeValue::Text(t) => Some(t.clone()),
        AttributeValue::Float(f) => Some(f.to_string()),
        AttributeValue::Int(i) => Some(i.to_string()),
        AttributeValue::Bool(b) => Some(b.to_string()),
        // `None` means "remove"; listeners and opaque values are not attributes.
        _ => None,
    }
}

impl dioxus_core::WriteMutations for Dom {
    fn append_children(&mut self, id: ElementId, m: usize) {
        self.edits += 1;
        let kids = self.take(m);
        self.detach_all(&kids);
        match self.ids.get(&id.0).copied() {
            Some(parent) => {
                for k in kids {
                    self.arena[k.0].parent = Some(parent);
                    self.arena[parent.0].children.push(k);
                }
            }
            // ElementId(0) is the mount point, which has no mirror node.
            None => {
                for k in kids {
                    self.arena[k.0].parent = None;
                    self.roots.push(k);
                }
            }
        }
    }

    fn assign_node_id(&mut self, path: &'static [u8], id: ElementId) {
        self.edits += 1;
        let key = self.load_child(path);
        self.ids.insert(id.0, key);
    }

    fn create_placeholder(&mut self, id: ElementId) {
        self.edits += 1;
        let key = self.push_node(Node::new(NodeKind::Placeholder));
        self.ids.insert(id.0, key);
        self.stack.push(key);
    }

    fn create_text_node(&mut self, value: &str, id: ElementId) {
        self.edits += 1;
        let key = self.push_node(Node::new(NodeKind::Text));
        self.arena[key.0].text = value.to_string();
        self.ids.insert(id.0, key);
        self.stack.push(key);
    }

    fn load_template(&mut self, template: Template, index: usize, id: ElementId) {
        self.edits += 1;
        let key = self.build(&template.roots[index]);
        self.ids.insert(id.0, key);
        self.stack.push(key);
    }

    fn replace_node_with(&mut self, id: ElementId, m: usize) {
        self.edits += 1;
        let nodes = self.take(m);
        self.detach_all(&nodes);
        let Some(target) = self.ids.get(&id.0).copied() else {
            return;
        };
        let (parent, ix) = self.locate(target);
        match parent {
            Some(_) => {
                self.detach(target);
            }
            None => {
                self.roots.retain(|r| *r != target);
            }
        }
        self.splice_at(parent, ix, &nodes);
        self.mark_removed(target);
    }

    fn replace_placeholder_with_nodes(&mut self, path: &'static [u8], m: usize) {
        self.edits += 1;
        let nodes = self.take(m);
        let target = self.load_child(path);
        self.detach_all(&nodes);
        let (parent, ix) = self.locate(target);
        match parent {
            Some(_) => {
                self.detach(target);
            }
            None => {
                self.roots.retain(|r| *r != target);
            }
        }
        self.splice_at(parent, ix, &nodes);
        self.mark_removed(target);
    }

    fn insert_nodes_after(&mut self, id: ElementId, m: usize) {
        self.edits += 1;
        let nodes = self.take(m);
        let Some(target) = self.ids.get(&id.0).copied() else {
            return;
        };
        // Detach first, then locate: removing a node that sat before the
        // target shifts the target's index.
        self.detach_all(&nodes);
        let (parent, ix) = self.locate(target);
        self.splice_at(parent, ix + 1, &nodes);
    }

    fn insert_nodes_before(&mut self, id: ElementId, m: usize) {
        self.edits += 1;
        let nodes = self.take(m);
        let Some(target) = self.ids.get(&id.0).copied() else {
            return;
        };
        self.detach_all(&nodes);
        let (parent, ix) = self.locate(target);
        self.splice_at(parent, ix, &nodes);
    }

    fn set_attribute(
        &mut self,
        name: &'static str,
        _ns: Option<&'static str>,
        value: &AttributeValue,
        id: ElementId,
    ) {
        self.edits += 1;
        let Some(key) = self.ids.get(&id.0).copied() else {
            return;
        };
        match attr_text(value) {
            Some(v) => {
                self.arena[key.0].attrs.insert(name.to_string(), v);
            }
            None => {
                self.arena[key.0].attrs.remove(name);
            }
        }
    }

    fn set_node_text(&mut self, value: &str, id: ElementId) {
        self.edits += 1;
        if let Some(key) = self.ids.get(&id.0).copied() {
            self.arena[key.0].text = value.to_string();
        }
    }

    fn create_event_listener(&mut self, name: &'static str, id: ElementId) {
        self.edits += 1;
        if let Some(key) = self.ids.get(&id.0).copied() {
            let l = &mut self.arena[key.0].listeners;
            if !l.iter().any(|n| n == name) {
                l.push(name.to_string());
            }
        }
    }

    fn remove_event_listener(&mut self, name: &'static str, id: ElementId) {
        self.edits += 1;
        if let Some(key) = self.ids.get(&id.0).copied() {
            self.arena[key.0].listeners.retain(|n| n != name);
        }
    }

    fn remove_node(&mut self, id: ElementId) {
        self.edits += 1;
        let Some(key) = self.ids.get(&id.0).copied() else {
            return;
        };
        if self.detach(key).is_none() {
            self.roots.retain(|r| *r != key);
        }
        self.mark_removed(key);
    }

    fn push_root(&mut self, id: ElementId) {
        self.edits += 1;
        if let Some(key) = self.ids.get(&id.0).copied() {
            self.stack.push(key);
        }
    }
}
