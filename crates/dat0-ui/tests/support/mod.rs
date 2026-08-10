//! The headless component-test harness.
//!
//! Dioxus ships no official component-test harness, so this is the minimum one,
//! built against the documented [`WriteMutations`] seam. Everything in Phases
//! 3–6 is verified through it.
//!
//! The query/act surface deliberately mirrors the GPUI `A11ySnapshot` it
//! replaces — `has_label`, `query_by_role`, `click`, `press_tab`, `settle` —
//! so the suites that assert *content and keyboard behaviour* port by
//! substitution rather than rewrite:
//!
//! | GPUI | here |
//! |---|---|
//! | `cx.add_window_view(..)` + `A11ySnapshot::capture(cx)` | `Harness::new(Component, props)` |
//! | `snap.has_label("x")` | `h.has_label("x")` |
//! | `snap.query_by_role(Role::Button, "x")` | `h.query_by_role("button", "x")` |
//! | `snap.click(cx, "label")` | `h.click_label("label")` |
//! | `cx.simulate_keystrokes("cmd-k")` | `h.key(id, Key::Character("k"), Modifiers::META)` |
//! | `press_tab(cx)` | `h.press_tab()` |
//! | `run_until_parked()` | `h.settle()` |
//!
//! What it is *not*: a browser. There is no layout, so nothing here can answer
//! "is this visible" or "what are its pixel bounds". Assertions are about
//! structure, text, attributes and event wiring. Anything geometric is a
//! windowed check — see the design-conformance gate.

#![allow(dead_code)] // Each test binary uses a different slice of this surface.

pub mod dom;

use std::any::Any;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_core::{ComponentFunction, ElementId};
use dom::{Dom, NodeKey};

pub use dioxus::prelude::{Code, Key, Location, Modifiers};

/// A mounted component under test.
pub struct Harness {
    vdom: VirtualDom,
    dom: Dom,
    /// Keyboard focus, tracked here because there is no browser to own a
    /// `document.activeElement`. Modelling it as state a test can read is more
    /// honest than pretending the harness has a real focus ring.
    focus: Option<NodeKey>,
}

impl Harness {
    /// Mount `root` with `props` and run the initial build.
    pub fn new<P: Clone + 'static, M: 'static>(
        root: impl ComponentFunction<P, M>,
        props: P,
    ) -> Self {
        let mut vdom = VirtualDom::new_with_props(root, props);
        let mut dom = Dom::new();
        vdom.rebuild(&mut dom);
        Self {
            vdom,
            dom,
            focus: None,
        }
    }

    /// Drain pending work until the tree stops changing.
    ///
    /// Bounded at 64 passes: an effect that re-triggers itself is a bug, and a
    /// harness that spins forever on it reports a hang instead of a failure.
    pub fn settle(&mut self) {
        for _ in 0..64 {
            let before = self.dom.edit_count();
            self.vdom.render_immediate(&mut self.dom);
            if self.dom.edit_count() == before {
                return;
            }
        }
        panic!("the tree never settled after 64 render passes — a self-triggering effect?");
    }

    // ── queries ──────────────────────────────────────────────────────────────

    /// The node carrying `data-a11y-id`. The stable handle: unlike a label, it
    /// survives a copy change.
    pub fn by_a11y_id(&self, id: &str) -> Option<NodeKey> {
        self.find(|n| n.attr("data-a11y-id") == Some(id))
    }

    /// The node whose accessible name is exactly `label`.
    pub fn by_label(&self, label: &str) -> Option<NodeKey> {
        self.find(|n| n.attr("aria-label") == Some(label))
    }

    /// Every node with this ARIA role.
    pub fn by_role(&self, role: &str) -> Vec<NodeKey> {
        self.dom
            .walk()
            .into_iter()
            .filter(|k| self.dom.get(*k).attr("role") == Some(role))
            .collect()
    }

    /// Is a node with this accessible name present? (`A11ySnapshot::has_label`)
    pub fn has_label(&self, label: &str) -> bool {
        self.by_label(label).is_some()
    }

    /// Is there a node with this role *and* this name?
    /// (`A11ySnapshot::query_by_role`)
    pub fn query_by_role(&self, role: &str, label: &str) -> bool {
        self.by_role(role)
            .into_iter()
            .any(|k| self.dom.get(k).attr("aria-label") == Some(label))
    }

    /// How many nodes carry this accessible name? Catches the duplicate-mount
    /// class of bug, where a panel is rendered twice and both respond.
    pub fn count_label(&self, label: &str) -> usize {
        self.dom
            .walk()
            .into_iter()
            .filter(|k| self.dom.get(*k).attr("aria-label") == Some(label))
            .count()
    }

    /// Is there a node whose accessible name *contains* `needle`? For names
    /// that interpolate a value.
    pub fn has_label_contains(&self, needle: &str) -> bool {
        self.dom.walk().into_iter().any(|k| {
            self.dom
                .get(k)
                .attr("aria-label")
                .is_some_and(|l| l.contains(needle))
        })
    }

    /// Visible text of a subtree, whitespace-collapsed.
    pub fn text_of(&self, key: NodeKey) -> String {
        self.dom.text_of(key)
    }

    /// Visible text of the whole tree. The blunt instrument; prefer a scoped
    /// `text_of` so an assertion cannot pass on a coincidence elsewhere.
    pub fn text(&self) -> String {
        self.dom
            .roots
            .iter()
            .map(|r| self.dom.text_of(*r))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// An attribute of a node.
    pub fn attr(&self, key: NodeKey, name: &str) -> Option<String> {
        self.dom.get(key).attr(name).map(str::to_string)
    }

    /// Does this node have a live listener for `event`? Proves a handler is
    /// *wired*, which a text assertion cannot.
    pub fn has_listener(&self, key: NodeKey, event: &str) -> bool {
        self.dom.get(key).listeners.iter().any(|l| l == event)
    }

    /// Nodes in tab order: those with `tabindex="0"`, in document order.
    pub fn tab_order(&self) -> Vec<NodeKey> {
        self.dom
            .walk()
            .into_iter()
            .filter(|k| self.dom.get(*k).attr("tabindex") == Some("0"))
            .collect()
    }

    /// The rendered HTML, for `insta` snapshots.
    pub fn html(&self) -> String {
        dioxus_ssr::render(&self.vdom)
    }

    /// The mirror, for assertions this surface does not cover.
    pub fn dom(&self) -> &Dom {
        &self.dom
    }

    fn find(&self, pred: impl Fn(&dom::Node) -> bool) -> Option<NodeKey> {
        self.dom.walk().into_iter().find(|k| pred(self.dom.get(*k)))
    }

    // ── acts ─────────────────────────────────────────────────────────────────

    /// Dispatch an event at a node, then settle.
    ///
    /// Panics on a node with no `ElementId`: Dioxus only assigns one to nodes
    /// it can address, so a missing id means the element has no listeners at
    /// all — which is the bug the test is trying to find, and a silent no-op
    /// would hide it.
    pub fn dispatch(&mut self, key: NodeKey, name: &str, data: impl Any + 'static) {
        let id = self.dom.element_id_of(key).unwrap_or_else(|| {
            panic!(
                "node has no ElementId, so it has no listeners: {:?}",
                self.dom.get(key).kind
            )
        });
        self.dispatch_to(id, name, data);
    }

    /// `data` must be the **serialized** form of the event
    /// (`SerializedMouseData`, `SerializedKeyboardData`, …), not the public
    /// one.
    ///
    /// An `onclick` listener is registered as `Event<PlatformEventData>` and
    /// converts to `MouseData` through the process-wide `HtmlEventConverter`
    /// (`dioxus-html/src/events/mod.rs`), which every renderer installs. Handing
    /// it a bare `MouseData` fails the downcast *inside* dioxus with a bare
    /// `Any { .. }` and no hint as to why.
    fn dispatch_to(&mut self, id: ElementId, name: &str, data: impl Any + 'static) {
        install_event_converter();
        let platform = dioxus::html::PlatformEventData::new(Box::new(data));
        let event = Event::new(Rc::new(platform) as Rc<dyn Any>, true);
        self.vdom.runtime().handle_event(name, event, id);
        self.settle();
    }

    /// Click the node with this `data-a11y-id`.
    pub fn click(&mut self, a11y_id: &str) {
        let key = self
            .by_a11y_id(a11y_id)
            .unwrap_or_else(|| panic!("no element with data-a11y-id={a11y_id:?}"));
        self.dispatch(key, "click", mouse(Modifiers::empty()));
    }

    /// Click the node with this accessible name. (`A11ySnapshot::click`)
    pub fn click_label(&mut self, label: &str) {
        let key = self
            .by_label(label)
            .unwrap_or_else(|| panic!("no element with aria-label={label:?}"));
        self.dispatch(key, "click", mouse(Modifiers::empty()));
    }

    /// Press a key at a node.
    pub fn key(&mut self, key: NodeKey, k: Key, mods: Modifiers) {
        self.dispatch(key, "keydown", keyboard(k, mods));
    }

    /// Press a key at the node with this `data-a11y-id`.
    pub fn key_at(&mut self, a11y_id: &str, k: Key, mods: Modifiers) {
        let key = self
            .by_a11y_id(a11y_id)
            .unwrap_or_else(|| panic!("no element with data-a11y-id={a11y_id:?}"));
        self.key(key, k, mods);
    }

    /// Press a key at the first root — the shell's key cascade entry point.
    pub fn key_global(&mut self, k: Key, mods: Modifiers) {
        let root = *self.dom.roots.first().expect("the tree has a root");
        self.key(root, k, mods);
    }

    /// Advance the harness's notion of keyboard focus. (`press_tab`)
    ///
    /// Focus is the harness's own cursor over [`tab_order`]: there is no
    /// browser here to own a `document.activeElement`, and modelling it as
    /// state a test can read is more useful than pretending otherwise.
    ///
    /// [`tab_order`]: Self::tab_order
    pub fn press_tab(&mut self) -> Option<NodeKey> {
        self.step_focus(1)
    }

    pub fn press_shift_tab(&mut self) -> Option<NodeKey> {
        self.step_focus(-1)
    }

    fn step_focus(&mut self, delta: isize) -> Option<NodeKey> {
        let order = self.tab_order();
        if order.is_empty() {
            return None;
        }
        let current = self.focus.and_then(|f| order.iter().position(|k| *k == f));
        let next = match current {
            Some(i) => (i as isize + delta).rem_euclid(order.len() as isize) as usize,
            None if delta > 0 => 0,
            None => order.len() - 1,
        };
        self.focus = Some(order[next]);
        self.focus
    }

    /// The accessible name of the focused node. (`A11ySnapshot::focused_label`)
    pub fn focused_label(&self) -> Option<String> {
        self.focus
            .and_then(|k| self.dom.get(k).attr("aria-label").map(str::to_string))
    }

    /// The `data-a11y-id` of the focused node.
    pub fn focused_id(&self) -> Option<String> {
        self.focus
            .and_then(|k| self.dom.get(k).attr("data-a11y-id").map(str::to_string))
    }
}

/// Install the serialized-event converter once per process.
///
/// Every renderer installs one at start-up; a harness is a renderer, so it must
/// too. Without it the first event dispatch panics inside dioxus-html on an
/// `unwrap` of `None`.
fn install_event_converter() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    });
}

/// A primary-button click at the origin.
///
/// Coordinates are zeroed rather than plausible: this harness has no layout, so
/// any non-zero value would be a fiction a test could accidentally assert on.
/// Pointer geometry is a windowed concern.
fn mouse(mods: Modifiers) -> dioxus::html::SerializedMouseData {
    use dioxus::html::SerializedMouseData;
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;

    let at = Coordinates::new(
        ScreenPoint::new(0.0, 0.0),
        ClientPoint::new(0.0, 0.0),
        ElementPoint::new(0.0, 0.0),
        PagePoint::new(0.0, 0.0),
    );
    SerializedMouseData::new(
        Some(MouseButton::Primary),
        MouseButton::Primary.into(),
        at,
        mods,
    )
}

fn keyboard(key: Key, mods: Modifiers) -> dioxus::html::SerializedKeyboardData {
    use dioxus::html::SerializedKeyboardData;
    let code = code_for(&key);
    SerializedKeyboardData::new(
        key,
        code,
        Location::Standard,
        /* is_auto_repeating */ false,
        mods,
        /* is_composing */ false,
    )
}

/// A plausible `code` for a `key`. Handlers should read `key`, not `code`, but
/// the event carries both and a nonsense value would be a trap.
fn code_for(key: &Key) -> Code {
    match key {
        Key::Enter => Code::Enter,
        Key::Escape => Code::Escape,
        Key::Tab => Code::Tab,
        Key::ArrowUp => Code::ArrowUp,
        Key::ArrowDown => Code::ArrowDown,
        Key::ArrowLeft => Code::ArrowLeft,
        Key::ArrowRight => Code::ArrowRight,
        Key::Backspace => Code::Backspace,
        Key::Delete => Code::Delete,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Character(c) => match c.to_ascii_lowercase().as_str() {
            "a" => Code::KeyA,
            "b" => Code::KeyB,
            "c" => Code::KeyC,
            "d" => Code::KeyD,
            "k" => Code::KeyK,
            "n" => Code::KeyN,
            "v" => Code::KeyV,
            "x" => Code::KeyX,
            "z" => Code::KeyZ,
            _ => Code::Unidentified,
        },
        _ => Code::Unidentified,
    }
}
