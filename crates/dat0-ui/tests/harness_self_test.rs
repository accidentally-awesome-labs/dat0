//! The harness testing itself.
//!
//! Everything in Phases 3–6 is asserted through `support::Harness`, so a bug
//! in the mirror is a bug that makes every other suite lie — most dangerously
//! by *passing*. These cases pin the parts of the mutation protocol that are
//! easy to get subtly wrong: keyed list reordering, conditional subtrees,
//! attribute removal, and the fact that a stale node really leaves the tree.

mod support;

use dioxus::prelude::*;
use support::{Harness, Key, Modifiers};

#[component]
fn Static() -> Element {
    rsx! {
        div { "data-a11y-id": "root", role: "group",
            h1 { "aria-label": "Title", class: "d0-h1", "dat0" }
            button { "data-a11y-id": "go", role: "button", "aria-label": "Run", tabindex: "0",
                "Run"
            }
            span { class: "d0-mono", "1,048,576 rows" }
        }
    }
}

#[test]
fn a_static_tree_is_queryable_by_id_role_label_and_text() {
    let h = Harness::new(Static, ());

    assert!(h.by_a11y_id("root").is_some());
    assert!(h.has_label("Title"));
    assert!(h.query_by_role("button", "Run"));
    assert!(
        !h.query_by_role("button", "Title"),
        "role and name must both match"
    );

    let root = h.by_a11y_id("root").unwrap();
    assert_eq!(h.text_of(root), "dat0 Run 1,048,576 rows");

    let go = h.by_a11y_id("go").unwrap();
    assert_eq!(h.attr(go, "tabindex").as_deref(), Some("0"));
    assert_eq!(h.attr(go, "class"), None, "the button carries no class");
}

#[component]
fn Counter() -> Element {
    let mut n = use_signal(|| 0);
    rsx! {
        div {
            button {
                "data-a11y-id": "inc",
                role: "button",
                "aria-label": "Increment",
                tabindex: "0",
                onclick: move |_| n += 1,
                "+"
            }
            span { "data-a11y-id": "count", "{n}" }
        }
    }
}

#[test]
fn a_click_runs_the_handler_and_the_mirror_sees_the_new_text() {
    let mut h = Harness::new(Counter, ());
    let count = h.by_a11y_id("count").unwrap();
    assert_eq!(h.text_of(count), "0");

    h.click("inc");
    assert_eq!(h.text_of(h.by_a11y_id("count").unwrap()), "1");

    h.click("inc");
    h.click("inc");
    assert_eq!(h.text_of(h.by_a11y_id("count").unwrap()), "3");
}

#[test]
fn a_listener_is_visible_as_wiring_not_just_as_an_effect() {
    // Proves a handler is attached even where clicking it would be a no-op —
    // the difference between "does nothing" and "is not connected".
    let h = Harness::new(Counter, ());
    let inc = h.by_a11y_id("inc").unwrap();
    assert!(h.has_listener(inc, "click"));
    assert!(!h.has_listener(inc, "keydown"));
}

#[component]
fn Keys() -> Element {
    let mut last = use_signal(String::new);
    rsx! {
        div {
            "data-a11y-id": "surface",
            tabindex: "0",
            onkeydown: move |e| {
                let m = e.modifiers();
                last.set(format!(
                    "{}{}",
                    if m.meta() { "cmd-" } else { "" },
                    e.key()
                ));
            },
            span { "data-a11y-id": "last", "{last}" }
        }
    }
}

#[test]
fn a_keystroke_carries_its_key_and_modifiers() {
    let mut h = Harness::new(Keys, ());
    h.key_at("surface", Key::Character("k".into()), Modifiers::META);
    assert_eq!(h.text_of(h.by_a11y_id("last").unwrap()), "cmd-k");

    h.key_at("surface", Key::Escape, Modifiers::empty());
    assert_eq!(h.text_of(h.by_a11y_id("last").unwrap()), "Escape");
}

#[derive(Props, Clone, PartialEq)]
struct ListProps {
    initial: Vec<u32>,
}

#[component]
fn List(props: ListProps) -> Element {
    let mut items = use_signal(|| props.initial.clone());
    rsx! {
        div {
            button {
                "data-a11y-id": "reverse",
                role: "button",
                "aria-label": "Reverse",
                onclick: move |_| items.write().reverse(),
                "reverse"
            }
            button {
                "data-a11y-id": "drop-first",
                role: "button",
                "aria-label": "Drop first",
                onclick: move |_| { items.write().remove(0); },
                "drop"
            }
            ul { "data-a11y-id": "list",
                for i in items() {
                    li { key: "{i}", "data-a11y-id": "item-{i}", role: "row", "{i}" }
                }
            }
        }
    }
}

#[test]
fn a_keyed_list_reorders_without_losing_or_duplicating_nodes() {
    // Keyed reordering drives `insert_nodes_before` / `push_root`, the two
    // mutations most likely to leave a mirror subtly wrong — and a grid that
    // scrolls is nothing but keyed reordering.
    let mut h = Harness::new(
        List,
        ListProps {
            initial: vec![1, 2, 3],
        },
    );
    let list = h.by_a11y_id("list").unwrap();
    assert_eq!(h.text_of(list), "1 2 3");
    assert_eq!(h.by_role("row").len(), 3);

    h.click("reverse");
    assert_eq!(h.text_of(h.by_a11y_id("list").unwrap()), "3 2 1");
    assert_eq!(h.by_role("row").len(), 3, "reordering must not duplicate");
}

#[test]
fn a_removed_item_leaves_the_tree() {
    let mut h = Harness::new(
        List,
        ListProps {
            initial: vec![1, 2, 3],
        },
    );
    h.click("drop-first");

    assert_eq!(h.text_of(h.by_a11y_id("list").unwrap()), "2 3");
    assert_eq!(h.by_role("row").len(), 2);
    assert!(
        h.by_a11y_id("item-1").is_none(),
        "a removed node must not still answer queries"
    );
}

#[component]
fn Conditional() -> Element {
    let mut open = use_signal(|| false);
    rsx! {
        div {
            button {
                "data-a11y-id": "toggle",
                role: "button",
                "aria-label": "Toggle",
                onclick: move |_| open.toggle(),
                "toggle"
            }
            if open() {
                div { "data-a11y-id": "panel", role: "dialog", "aria-label": "Panel", "body" }
            }
        }
    }
}

#[test]
fn a_conditional_subtree_appears_and_disappears() {
    // `if` in rsx compiles to a placeholder that is replaced and re-created —
    // `replace_placeholder_with_nodes` and `replace_node_with`, the pair a
    // naive mirror gets wrong by leaving the placeholder in the tree.
    let mut h = Harness::new(Conditional, ());
    assert!(h.by_a11y_id("panel").is_none());
    assert!(h.by_role("dialog").is_empty());

    h.click("toggle");
    assert!(h.by_a11y_id("panel").is_some());
    assert_eq!(h.by_role("dialog").len(), 1);
    assert!(h.query_by_role("dialog", "Panel"));

    h.click("toggle");
    assert!(h.by_a11y_id("panel").is_none());
    assert!(
        h.by_role("dialog").is_empty(),
        "the panel must really leave"
    );
}

#[component]
fn Attrs() -> Element {
    let mut on = use_signal(|| true);
    rsx! {
        div {
            button {
                "data-a11y-id": "flip",
                role: "button",
                "aria-label": "Flip",
                onclick: move |_| on.toggle(),
                "flip"
            }
            div {
                "data-a11y-id": "target",
                class: if on() { "d0-cell is-selected" } else { "d0-cell" },
                style: if on() { "left: 100px" } else { "left: 200px" },
            }
        }
    }
}

#[test]
fn dynamic_attributes_track_state() {
    let mut h = Harness::new(Attrs, ());
    let t = h.by_a11y_id("target").unwrap();
    assert_eq!(h.attr(t, "class").as_deref(), Some("d0-cell is-selected"));
    assert_eq!(h.attr(t, "style").as_deref(), Some("left: 100px"));

    h.click("flip");
    let t = h.by_a11y_id("target").unwrap();
    assert_eq!(h.attr(t, "class").as_deref(), Some("d0-cell"));
    assert_eq!(h.attr(t, "style").as_deref(), Some("left: 200px"));
}

#[component]
fn Focusables() -> Element {
    rsx! {
        div {
            button { "data-a11y-id": "a", "aria-label": "A", tabindex: "0", "a" }
            span { "data-a11y-id": "skip", "aria-label": "Skip", tabindex: "-1", "s" }
            button { "data-a11y-id": "b", "aria-label": "B", tabindex: "0", "b" }
        }
    }
}

#[test]
fn tab_visits_only_real_tab_stops_and_wraps() {
    let mut h = Harness::new(Focusables, ());
    assert_eq!(h.tab_order().len(), 2, "tabindex=-1 is not a tab stop");

    h.press_tab();
    assert_eq!(h.focused_label().as_deref(), Some("A"));
    h.press_tab();
    assert_eq!(h.focused_label().as_deref(), Some("B"));
    h.press_tab();
    assert_eq!(h.focused_label().as_deref(), Some("A"), "Tab wraps");

    h.press_shift_tab();
    assert_eq!(
        h.focused_label().as_deref(),
        Some("B"),
        "Shift-Tab wraps back"
    );
}

#[test]
fn count_label_catches_a_double_mount() {
    let h = Harness::new(Static, ());
    assert_eq!(h.count_label("Run"), 1);
    assert_eq!(h.count_label("Nothing"), 0);
}

#[test]
fn html_renders_for_snapshotting() {
    let h = Harness::new(Static, ());
    let html = h.html();
    assert!(html.contains("data-a11y-id=\"root\""), "{html}");
    assert!(html.contains("1,048,576 rows"), "{html}");
}
