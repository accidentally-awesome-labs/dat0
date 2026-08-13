//! Pane chrome (S4) — what a dock surface's header and body are made of.
//!
//! Ported from `dat0-app/tests/dock_chrome_spike.rs` (B6 T0 / B8 T0). That file
//! was archaeology on `gpui-component`: does a `TabPanel`'s
//! `overflow_y_scroll` + `.cached(..)` + `.tab_group()` wrapper change what a
//! single-frame a11y capture sees? Is a focus stop inside one still reachable
//! by Tab? Why has no dat0 panel ever grown a ✕, given `Panel::closable`
//! defaults to `true`?
//!
//! None of that markup exists any more — `pane.rs` opens by saying so: dat0
//! was already picking `DockItem::panel` to dodge `TabPanel`'s 30px title bar,
//! and a pane is ~40 lines, so the workaround became the implementation. What
//! ports is the *question* the spike answered, asked of [`Pane`]:
//!
//! | B6/B8 asked of `TabPanel` | asked here of `Pane` |
//! |---|---|
//! | does the chrome duplicate or swallow the panel's node? | one header, one body, exactly once |
//! | does a sibling dock disturb the centre's capture? | sibling panes do not disturb each other |
//! | is `toggle_dock` observable with no settle frame? | a header click flips `aria-expanded` in one settle |
//! | is a stop inside the chrome Tab-reachable? | the header is a real `button` with a click listener |
//! | can a docked panel be closed irrecoverably? | the header is the *only* control, and it reopens |
//!
//! **Deleted, with reason:** `a_closed_bottom_dock_is_still_rendered` pinned an
//! upstream quirk — `Dock::render` returns an empty div for a closed left or
//! right dock but keeps a closed BOTTOM one at `h(px(29.))` so its title bar
//! stays clickable. There is no `Dock`, no placement branch and no 29px bar;
//! the console's closed behaviour is asserted directly in
//! `tests/bottom_dock.rs::a_closed_console_leaves_no_console_nodes_in_the_tree`,
//! which is the same claim without the upstream caveat.

mod support;

use dioxus::prelude::*;

use dat0_ui::components::pane::Pane;
use support::Harness;
use support::dom::NodeKey;

// ── host ─────────────────────────────────────────────────────────────────────

/// Two panes side by side, each with its own body marker.
///
/// Two, deliberately, and with DIFFERENT ids — B6's probes carried distinct
/// labels so a count could never be misattributed between placements, and the
/// same trap applies to a component rendered twice in one tree.
#[derive(Clone, PartialEq, Props)]
struct HostProps {
    first_open: bool,
    second_open: bool,
    /// Render the second pane at all. The single-pane case is the control.
    second: bool,
}

impl Default for HostProps {
    fn default() -> Self {
        Self {
            first_open: true,
            second_open: true,
            second: false,
        }
    }
}

#[component]
fn Host(props: HostProps) -> Element {
    let mut first = use_signal(|| props.first_open);
    let mut second = use_signal(|| props.second_open);
    let mut toggles = use_signal(|| 0usize);

    rsx! {
        Pane {
            id: "alpha".to_string(),
            title: "Alpha Pane".to_string(),
            meta: "⌘⏎ run".to_string(),
            open: first(),
            on_toggle: move |_| {
                let v = first();
                first.set(!v);
                toggles += 1;
            },
            div { "data-a11y-id": "alpha-body", "alpha body" }
        }

        if props.second {
            Pane {
                id: "beta".to_string(),
                title: "Beta Pane".to_string(),
                meta: "price · DOUBLE".to_string(),
                open: second(),
                on_toggle: move |_| {
                    let v = second();
                    second.set(!v);
                },
                div { "data-a11y-id": "beta-body", "beta body" }
            }
        }

        div { "data-a11y-id": "rb-toggles", "{toggles}" }
    }
}

fn mount(props: HostProps) -> Harness {
    let mut h = Harness::new(Host, props);
    h.settle();
    h
}

/// One open pane — the control, B6's "bare centre panel".
fn one() -> HostProps {
    HostProps::default()
}

/// Two open panes — B6's "centre plus a right dock".
fn two() -> HostProps {
    HostProps {
        second: true,
        ..HostProps::default()
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn count_id(h: &Harness, id: &str) -> usize {
    h.dom()
        .walk()
        .into_iter()
        .filter(|k| h.dom().get(*k).attr("data-a11y-id") == Some(id))
        .count()
}

fn id_node(h: &Harness, id: &str) -> NodeKey {
    h.by_a11y_id(id)
        .unwrap_or_else(|| panic!("no element with data-a11y-id={id:?}"))
}

/// The classes on a node, as a set of words.
fn classes(h: &Harness, key: NodeKey) -> Vec<String> {
    h.attr(key, "class")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// The direct children of a node, in order, as `(tag, class)`.
fn child_shape(h: &Harness, key: NodeKey) -> Vec<(String, String)> {
    h.dom()
        .get(key)
        .children
        .iter()
        .filter(|c| !h.dom().get(**c).removed)
        .filter_map(|c| {
            let n = h.dom().get(*c);
            n.tag().map(|t| {
                (
                    t.to_string(),
                    n.attr("class").unwrap_or_default().to_string(),
                )
            })
        })
        .collect()
}

// ── header structure ─────────────────────────────────────────────────────────

/// S4's header, element by element: chevron, `.d0-label` id, `.d0-head-title`
/// title, right-aligned `.d0-label` meta — in that order.
///
/// The order is the assertion. Each span read on its own would pass on a
/// header that rendered them in any arrangement, and the design's header is a
/// left-to-right reading: what kind of pane, what it is showing, what it can
/// do.
#[test]
fn a_pane_header_carries_a_chevron_an_id_a_title_and_a_meta_in_that_order() {
    let h = mount(one());
    let head = id_node(&h, "pane-head-alpha");

    assert_eq!(
        h.dom().get(head).tag(),
        Some("button"),
        "the header is the pane's control, so it is a button"
    );
    assert_eq!(
        child_shape(&h, head),
        vec![
            ("span".to_string(), "d0-chevron".to_string()),
            ("span".to_string(), "d0-label".to_string()),
            ("span".to_string(), "d0-head-title".to_string()),
            ("span".to_string(), "d0-pane-meta d0-label".to_string()),
        ],
        "the header's four parts, in the design's reading order"
    );

    let text = h.text_of(head);
    assert!(text.contains('▾'), "the chevron: {text:?}");
    assert!(text.contains("alpha"), "the pane's id: {text:?}");
    assert!(text.contains("Alpha Pane"), "its title: {text:?}");
    assert!(text.contains("⌘⏎ run"), "and its meta: {text:?}");
}

/// B6's core measurement, on the surface that replaced the thing it measured.
///
/// The numbers meant: 1 = intact, 2 = the chrome double-renders, 0 = the
/// `.cached(..)` wrapper swallowed the frame. The generation counter B6 had
/// ready would have fixed duplicates and would NOT have fixed omissions, which
/// is why it had to be measured rather than assumed. A pane has no cache, but
/// "exactly one" is still the only reading that means the chrome is honest.
#[test]
fn a_pane_emits_its_chrome_and_its_body_exactly_once() {
    let h = mount(one());

    for id in ["pane-alpha", "pane-head-alpha", "pane-body-alpha"] {
        assert_eq!(count_id(&h, id), 1, "{id} must appear exactly once");
    }
    assert_eq!(
        count_id(&h, "alpha-body"),
        1,
        "and the caller's own body content is passed through once — 2 would \
         mean the pane renders its children twice, 0 that it swallowed them"
    );
    assert_eq!(h.text_of(id_node(&h, "pane-body-alpha")), "alpha body");
}

/// B6: "adding a right dock must not disturb the centre's own capture."
#[test]
fn a_sibling_pane_does_not_disturb_the_first_ones_chrome() {
    let h = mount(two());

    for id in [
        "pane-alpha",
        "pane-head-alpha",
        "pane-body-alpha",
        "alpha-body",
        "pane-beta",
        "pane-head-beta",
        "pane-body-beta",
        "beta-body",
    ] {
        assert_eq!(
            count_id(&h, id),
            1,
            "{id} must appear exactly once with two panes mounted"
        );
    }
    assert_eq!(h.text_of(id_node(&h, "pane-body-alpha")), "alpha body");
    assert_eq!(
        h.text_of(id_node(&h, "pane-body-beta")),
        "beta body",
        "and neither pane is showing the other's children"
    );
}

// ── collapse ─────────────────────────────────────────────────────────────────

/// The collapsed treatment: header kept, `is-collapsed` on, `aria-expanded`
/// off — and the body still mounted.
///
/// The last part is the load-bearing one and is stated in `pane.rs`: the body
/// is hidden by CSS, not unmounted, so a pane keeps its state and its scroll
/// position across a collapse. Unmounting it would silently reset the
/// inspector's profile and the console's document on every chevron click.
#[test]
fn a_collapsed_pane_keeps_its_header_and_its_body_and_says_it_is_shut() {
    let h = mount(HostProps {
        first_open: false,
        ..one()
    });

    let pane = id_node(&h, "pane-alpha");
    assert!(
        classes(&h, pane).contains(&"is-collapsed".to_string()),
        "the collapsed treatment is on the pane: {:?}",
        h.attr(pane, "class")
    );
    assert_eq!(
        h.attr(id_node(&h, "pane-head-alpha"), "aria-expanded"),
        Some("false".to_string()),
        "and it announces itself to a screen reader"
    );
    assert!(
        h.text_of(id_node(&h, "pane-head-alpha"))
            .contains("Alpha Pane"),
        "a collapsed pane still shows its header — that header is the only \
         thing that can reopen it"
    );
    assert_eq!(
        count_id(&h, "alpha-body"),
        1,
        "the body stays MOUNTED and is hidden by CSS: unmounting it would \
         throw away the pane's state on every collapse"
    );
}

/// B8 T0 (d), restated: `is_dock_open` had to be observable immediately after
/// `toggle_dock`, or the shell's refresh-on-show fired on the wrong edge.
/// A signal write is observable after one settle, and the harness's `click`
/// settles — so what is worth pinning is that a click is *sufficient*, with no
/// extra frame, and that two clicks return to the start.
#[test]
fn a_header_click_flips_the_pane_and_two_clicks_return_it() {
    let mut h = mount(one());
    assert_eq!(
        h.attr(id_node(&h, "pane-head-alpha"), "aria-expanded"),
        Some("true".to_string()),
        "seed: open"
    );

    h.click("pane-head-alpha");
    assert_eq!(
        h.attr(id_node(&h, "pane-head-alpha"), "aria-expanded"),
        Some("false".to_string()),
        "one click closes it, with no further settle"
    );

    h.click("pane-head-alpha");
    assert_eq!(
        h.attr(id_node(&h, "pane-head-alpha"), "aria-expanded"),
        Some("true".to_string()),
        "and two clicks return to the starting state"
    );
    assert_eq!(
        h.text_of(id_node(&h, "rb-toggles")),
        "2",
        "each click raised exactly one toggle — a header that fired twice \
         would look inert because the second call undoes the first"
    );
}

#[test]
fn collapsing_a_pane_leaves_its_sibling_open() {
    let mut h = mount(two());

    h.click("pane-head-alpha");

    assert_eq!(
        h.attr(id_node(&h, "pane-head-alpha"), "aria-expanded"),
        Some("false".to_string())
    );
    assert_eq!(
        h.attr(id_node(&h, "pane-head-beta"), "aria-expanded"),
        Some("true".to_string()),
        "panes are a stack of independent collapsibles, not a split whose \
         halves move together"
    );
}

// ── no close button ──────────────────────────────────────────────────────────

/// B8's `a_docked_panel_is_not_closable_and_the_lock_is_not_why`.
///
/// That test was three-deep archaeology: `TabPanel::closable` short-circuits on
/// `!draggable`, `draggable = !is_locked && !is_last_panel`, and `is_locked`
/// ends with `stack_panel.is_none()` — so a dat0 dock panel was un-closable for
/// three independent reasons, none of them the `closable` override a reader
/// would look for. Its practical conclusion was that closing a dat0 dock panel
/// would be UNRECOVERABLE, because nothing could re-add one.
///
/// A pane makes that structural: the header is the only control it renders, it
/// collapses rather than closes, and the same control reopens it. This asserts
/// exactly that — one control, and the round trip.
#[test]
fn a_pane_can_only_be_collapsed_and_the_same_control_reopens_it() {
    let mut h = mount(one());
    let pane = id_node(&h, "pane-alpha");

    let controls: Vec<_> = h
        .dom()
        .walk()
        .into_iter()
        .filter(|k| h.dom().get(*k).tag() == Some("button"))
        .filter(|k| {
            // Inside this pane's subtree only; the readback div is not a
            // button, but a second pane's header would be.
            let mut cur = Some(*k);
            while let Some(n) = cur {
                if n == pane {
                    return true;
                }
                cur = h.dom().get(n).parent;
            }
            false
        })
        .collect();
    assert_eq!(
        controls.len(),
        1,
        "a pane renders exactly one control — no ✕, so a pane cannot be put \
         into a state nothing can undo"
    );
    assert_eq!(
        h.attr(controls[0], "data-a11y-id"),
        Some("pane-head-alpha".to_string()),
        "and that one control is the header"
    );

    h.click("pane-head-alpha");
    assert_eq!(count_id(&h, "pane-alpha"), 1, "collapsed, not removed");
    h.click("pane-head-alpha");
    assert_eq!(
        h.attr(id_node(&h, "pane-head-alpha"), "aria-expanded"),
        Some("true".to_string()),
        "the round trip closes: whatever a user can do to a pane, they can undo"
    );
}
