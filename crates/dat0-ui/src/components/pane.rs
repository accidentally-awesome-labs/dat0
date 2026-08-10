//! The pane (S4).
//!
//! Every dock surface — console, inspector, charts — is one of these: a 32px
//! header button over a body, collapsible, with the design's open and closed
//! treatments.
//!
//! This replaces `gpui-component`'s `TabPanel` chrome, which dat0 was already
//! working around: `window/dock.rs:341-345` picked `DockItem::panel`
//! specifically to avoid the 30px title bar `TabPanel` forced on it. A pane is
//! ~40 lines of markup, so the workaround becomes the implementation.

use dioxus::prelude::*;

/// A collapsible pane.
#[derive(Clone, PartialEq, Props)]
pub struct PaneProps {
    /// Stable id, used for `data-a11y-id` and as the header's small label.
    pub id: String,
    /// Header title.
    pub title: String,
    /// Right-aligned header meta: `⌘⏎ run`, `{column} · {type}`, the chart kind.
    #[props(default)]
    pub meta: String,
    /// Whether the body is showing.
    pub open: bool,
    /// The header was clicked.
    pub on_toggle: EventHandler<()>,
    /// Body content. Rendered even when closed, so a pane keeps its state and
    /// its scroll position across a collapse — the body is hidden by CSS
    /// (`opacity: 0`, zero flex-basis), not unmounted.
    pub children: Element,
}

#[component]
pub fn Pane(props: PaneProps) -> Element {
    let open = props.open;
    rsx! {
        section {
            class: if open { "d0-pane" } else { "d0-pane is-collapsed" },
            "data-a11y-id": "pane-{props.id}",

            button {
                class: "d0-pane-head",
                "data-a11y-id": "pane-head-{props.id}",
                "aria-expanded": if open { "true" } else { "false" },
                onclick: move |_| props.on_toggle.call(()),

                span { class: "d0-chevron", "▾" }
                span { class: "d0-label", "{props.id}" }
                span { class: "d0-head-title", "{props.title}" }
                span { class: "d0-pane-meta d0-label", "{props.meta}" }
            }

            div { class: "d0-pane-body", "data-a11y-id": "pane-body-{props.id}",
                {props.children}
            }
        }
    }
}
