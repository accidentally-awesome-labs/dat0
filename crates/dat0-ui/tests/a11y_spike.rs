//! The accessibility contract itself: annotated nodes are findable by name,
//! round-trip to their stable id, and reach the handler behind them.
//!
//! Ported from `crates/dat0-app/tests/a11y_spike.rs`. That file was the go/no-go
//! for the whole GPUI capture mechanism: it proved an AccessKit tree came back
//! from a rendered window, that a label could be resolved to a `debug_selector`
//! id, that geometry resolved from the id, and that a click at those bounds hit
//! the real widget. Three of those four steps were scaffolding around a
//! test-only capture. Here they are the DOM.
//!
//! # What is genuinely different
//!
//! Under GPUI, `A11yExt::a11y` emitted a node **only** under the `a11y-capture`
//! feature — release builds got an identity no-op, which is why deferral D-015
//! ("no production accessibility") stayed open. Every attribute this file
//! asserts ships in the release binary and is read by the platform AX API, so
//! the assertions are now about the product rather than about the harness.
//!
//! # The count bracket, ported
//!
//! The original asserted `17..=18` captured nodes as a *double-render proof*:
//! GPUI's frame collector accumulated, so a re-rendered frame doubled every
//! node and the label lookups would then panic on "found two or more". A
//! VirtualDom mirror has no accumulator, so the number itself carries nothing —
//! but the bug it was watching for (a surface mounted twice, where both copies
//! answer) is real in any tree. `every_annotated_node_is_mounted_once` states
//! that directly, over the whole shell, which is strictly stronger than a
//! magic-number range that had to be recounted by hand seven times.

mod support;

use std::rc::Rc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_ui::components::shell::Shell;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

// ── mounting ─────────────────────────────────────────────────────────────────

/// A fresh config dir with `first_run_done` unset, so the shell renders the
/// enriched first-run hero — where the "Take a tour" button and the headline
/// both live. Mirrors the original's bare `set_var`, minus the leak.
fn with_first_run_config<R>(f: impl FnOnce() -> R) -> R {
    let tmp = TempDir::new().unwrap();
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock, and
    // no other thread in this binary reads the variable.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    let out = f();
    unsafe {
        match previous {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

/// The real `Shell` under the four contexts a window provides, plus a readback
/// of the modal slot and a driver that empties it.
///
/// The driver matters: the first run may open the tour by itself, and a test
/// that asserts "clicking Take a tour opens the tour" has to start from a known
/// empty slot or it proves nothing.
#[component]
fn Host() -> Element {
    let mut ws = Workspace::provide();
    Theme::provide(None);
    use_context_provider(ActionRegistry::new);
    let (events, _rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, Rc::new(std::cell::RefCell::new(rx)))
    });
    use_context_provider(|| events.clone());

    let modal = ws
        .modal
        .read()
        .as_ref()
        .map(dat0_ui::components::modals::slug)
        .unwrap_or("-")
        .to_string();

    rsx! {
        Shell {}
        div { "data-a11y-id": "rb-modal", "{modal}" }
        button {
            "data-a11y-id": "drive-clear-modal",
            onclick: move |_| ws.modal.set(None),
            "clear"
        }
    }
}

fn mount() -> Harness {
    let mut h = Harness::new(Host, ());
    h.settle();
    h
}

fn take_tour() -> String {
    dat0_i18n::t("hero.take_tour")
}

// ── the contract ─────────────────────────────────────────────────────────────

#[test]
#[serial]
fn a_rendered_name_locates_a_control_and_round_trips_to_its_stable_id() {
    with_first_run_config(|| {
        let h = mount();
        let label = take_tour();

        // (a) findable by the exact string the render used …
        assert!(
            h.has_label(&label),
            "the hero's tour button must be findable by its rendered label"
        );
        // … and by role together with it.
        assert!(
            h.query_by_role("button", &label),
            "it must be findable as a button with that label"
        );

        // (b) the label resolves to the static id. Under GPUI this round trip
        // went through a side-map from the AccessKit node to the
        // `debug_selector`, and the point was that the two could not drift. The
        // id and the name are now attributes of one element, so the round trip
        // is the element — but a surface that annotated a name without an id
        // would still be unqueryable by every later suite, which is what this
        // catches.
        let key = h.by_label(&label).unwrap();
        assert_eq!(
            h.attr(key, "data-a11y-id").as_deref(),
            Some("hero-take-tour"),
            "a label lookup must land on the stable id the suites query by"
        );
        assert_eq!(
            h.attr(key, "tabindex").as_deref(),
            Some("0"),
            "a control a reader can name must also be one a keyboard can reach"
        );
    });
}

#[test]
#[serial]
fn a_name_located_click_reaches_the_live_handler() {
    with_first_run_config(|| {
        let mut h = mount();

        // The first run may have opened the tour already; start from empty so
        // the transition below is the click's doing and nothing else's.
        h.click("drive-clear-modal");
        assert_eq!(h.text_of(h.by_a11y_id("rb-modal").unwrap()), "-");

        h.click_label(&take_tour());

        assert_eq!(
            h.text_of(h.by_a11y_id("rb-modal").unwrap()),
            "tour",
            "clicking the button found by name must open the tour — a node \
             that announces itself but is wired to nothing is worse than an \
             unannotated one, because a reader promises it works"
        );
    });
}

#[test]
#[serial]
fn static_text_is_announced_without_pretending_to_be_a_control() {
    with_first_run_config(|| {
        let h = mount();
        let title = dat0_i18n::t("hero.title");

        // Content-only: the headline is findable by its exact rendered text …
        assert!(
            h.has_label(&title),
            "the hero headline must be findable by its rendered text"
        );
        // … as a note, which is what `AccessRole::Label` maps to (ARIA has no
        // `label` role, and `note` is what a reader announces as ancillary
        // content).
        assert!(
            h.query_by_role("note", &title),
            "the headline must be a note, not a control"
        );

        let key = h.by_label(&title).unwrap();
        assert_eq!(
            h.attr(key, "tabindex"),
            None,
            "content-only means not focusable: `TabStop::No` omits the \
             attribute rather than writing -1"
        );
        assert!(
            !h.has_listener(key, "click"),
            "content-only means no handler — the GPUI equivalent was `no click \
             id in the side-map`"
        );
    });
}

#[test]
#[serial]
fn every_annotated_node_is_mounted_once() {
    with_first_run_config(|| {
        let h = mount();

        // A duplicate id is the signature of a surface mounted twice — the
        // exact class of bug the original's node-count bracket existed to
        // catch, stated over every annotated element instead of a hand-counted
        // total.
        let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
        for key in h.dom().walk() {
            if let Some(id) = h.attr(key, "data-a11y-id") {
                *seen.entry(id).or_default() += 1;
            }
        }
        assert!(
            !seen.is_empty(),
            "the shell must annotate something, or this test is vacuous"
        );
        let dupes: Vec<(&String, &usize)> = seen.iter().filter(|(_, n)| **n > 1).collect();
        assert!(
            dupes.is_empty(),
            "these ids are mounted more than once, so a query by id is \
             ambiguous and a click by id is a coin flip: {dupes:?}"
        );

        // And the shell's own landmarks are each there exactly once — the
        // shape every other suite in this crate assumes.
        for id in ["window", "titlebar", "tabstrip", "statusbar", "pane-stack"] {
            assert_eq!(
                seen.get(id).copied(),
                Some(1),
                "the shell must render exactly one {id:?}"
            );
        }
    });
}
