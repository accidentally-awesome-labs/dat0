//! The token gallery renders from the token table, not from a copy of it.
//!
//! The gallery's whole value is being the one window a human looks at to judge
//! the design system. That only works if it cannot drift: a hand-kept swatch
//! list would be wrong within a week, and a stale gallery is worse than none —
//! it is a design system that lies. These tests pin the derivation.

#![cfg(feature = "gallery")]

mod support;

use dioxus::prelude::*;

use dat0_core::theme::tokens::CSS_NAMES;
use dat0_ui::gallery::Gallery;
use dat0_ui::theme::Theme;
use support::Harness;

/// The gallery reads the theme from context, which in the app comes from
/// `components::App`. Mounting it bare would panic before rendering a thing.
#[component]
fn Host() -> Element {
    Theme::provide(None);
    rsx! { Gallery {} }
}

#[test]
fn every_token_in_the_table_gets_a_swatch() {
    let h = Harness::new(Host, ());
    let html = h.html();
    for (name, _) in CSS_NAMES.iter() {
        assert!(
            html.contains(&format!("var({name})")),
            "{name} is in the token table but has no swatch — the gallery is \
             not deriving its grid from CSS_NAMES"
        );
    }
}

#[test]
fn every_embedded_icon_gets_a_cell() {
    let h = Harness::new(Host, ());
    let html = h.html();
    let icons = dat0_ui::protocol::icon_names();
    assert!(!icons.is_empty(), "no icons are embedded at all");
    for name in icons {
        assert!(
            html.contains(&format!("dat0://icons/{name}")),
            "{name} is embedded but the gallery does not show it"
        );
    }
}

#[test]
fn switching_theme_repaints_the_swatches() {
    // The swatch chips read `var(--d0-…)`, so the repaint is the stylesheet's
    // job — what this proves is that the button actually changes the theme the
    // page is rendered under, which is the half a CSS variable cannot do.
    let mut h = Harness::new(Host, ());
    assert!(h.html().contains("data-a11y-id=\"theme-dark\""));

    h.click("theme-dark");
    h.settle();
    let html = h.html();
    // The active theme's button is the primary one.
    let dark = html
        .find("data-a11y-id=\"theme-dark\"")
        .expect("the dark button is still there");
    let before = &html[html[..dark].rfind("<button").unwrap()..dark];
    assert!(
        before.contains("is-primary"),
        "after clicking `dark` the dark button should read as the active theme"
    );
}
