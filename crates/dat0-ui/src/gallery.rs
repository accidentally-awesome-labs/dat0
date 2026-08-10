//! Token gallery — the dat0 design system rendered as one scrollable page.
//!
//! Dev-only, behind the `gallery` feature that only the self-dev-dependency
//! turns on, so none of it reaches the shipped binary. Boot it with
//! `cargo run --features gallery --example gallery`.
//!
//! This is the manual-UAT vehicle: palette feel, high-contrast legibility, the
//! focus ring, elevation, the icon set and the type scale, all in one window
//! instead of booting the whole app once per theme.
//!
//! **It renders from the source of truth, not a copy.** The swatch grid is
//! built by iterating [`dat0_core::theme::tokens::CSS_NAMES`] and the icon grid
//! by iterating the embedded asset list, so a token or icon added anywhere
//! appears here with no edit. A gallery that had to be updated by hand would be
//! wrong within a week, and a stale gallery is worse than none — it is a
//! design system that lies.

use dioxus::prelude::*;

use dat0_core::theme::tokens::CSS_NAMES;

use crate::theme::{Theme, ThemeStyle};

/// Every builtin theme, in switch order.
const THEMES: [&str; 3] = ["light", "dark", "high-contrast"];

#[component]
pub fn Gallery() -> Element {
    let mut theme = Theme::use_current();
    let tokens = theme.tokens();
    let current = tokens.id.clone();

    rsx! {
        ThemeStyle {}
        div { class: "d0-gallery", "data-a11y-id": "gallery",
            header { class: "d0-gallery-head",
                span { class: "d0-wordmark", "dat" span { class: "d0-logomark" } }
                span { class: "d0-head-title", "token gallery" }
                div { class: "d0-gallery-themes",
                    for id in THEMES {
                        button {
                            key: "{id}",
                            class: if current == id { "d0-btn is-primary" } else { "d0-btn" },
                            "data-a11y-id": "theme-{id}",
                            onclick: move |_| theme.set(id),
                            "{id}"
                        }
                    }
                }
            }

            section { class: "d0-gallery-section",
                h2 { class: "d0-label", "colour" }
                div { class: "d0-swatch-grid",
                    // Straight from the token table: a new token shows up here
                    // with no edit to this file.
                    for (name, get) in CSS_NAMES.iter() {
                        div { key: "{name}", class: "d0-swatch-cell",
                            div {
                                class: "d0-swatch-chip",
                                style: "background: var({name})",
                            }
                            span { class: "d0-mono", "{name}" }
                            span { class: "d0-mono d0-swatch-value", "{get(&tokens)}" }
                        }
                    }
                }
            }

            section { class: "d0-gallery-section",
                h2 { class: "d0-label", "type" }
                div { class: "d0-gallery-type",
                    div { class: "d0-head-title", "Head title — Geist 15/1.2" }
                    div { class: "d0-label", "label — uppercase micro" }
                    div { class: "d0-mono", "mono 12.5 — SELECT * FROM sales WHERE qty > 10" }
                    div { class: "d0-num", "1,048,576" }
                }
            }

            section { class: "d0-gallery-section",
                h2 { class: "d0-label", "controls" }
                div { class: "d0-gallery-row",
                    button { class: "d0-btn", "default" }
                    button { class: "d0-btn is-primary", "primary" }
                    button { class: "d0-btn is-ghost", "ghost" }
                    button { class: "d0-btn", disabled: true, "disabled" }
                    input { class: "d0-field", placeholder: "sample input" }
                }
                div { class: "d0-gallery-row",
                    for kind in ["csv", "parquet", "sqlite", "json", "dat0", "other"] {
                        span { key: "{kind}", class: "d0-chip",
                            span { class: "d0-swatch sw-{kind}" }
                            "{kind}"
                        }
                    }
                }
            }

            section { class: "d0-gallery-section",
                h2 { class: "d0-label", "icons" }
                div { class: "d0-icon-grid",
                    for name in crate::protocol::icon_names() {
                        div { key: "{name}", class: "d0-icon-cell",
                            img { src: "dat0://icons/{name}", width: "16", height: "16" }
                            span { class: "d0-mono", "{name}" }
                        }
                    }
                }
            }
        }
    }
}
