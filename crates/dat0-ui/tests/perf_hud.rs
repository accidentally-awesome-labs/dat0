//! The performance HUD (MX1, step 5.8).
//!
//! `perf.hud.toggle` flipped a flag the shell never rendered, so it was the
//! last command in `router::UNWIRED`. Toggled from inside a real shell, the
//! HUD now shows its four lines, and toggled again it is gone. Headless, no
//! webview paints, so every rate reads as a dash, never `0`.

mod support;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::ids;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_ui::components::shell::Shell;
use dat0_ui::router::{self, Surface, SurfaceSlot};
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;

use support::Harness;

fn builtins() -> ActionRegistry {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
    reg
}

/// The real shell, and a button that routes the toggle from inside the tree.
#[component]
fn Host() -> Element {
    Workspace::provide();
    Theme::provide(None);
    use_context_provider(|| Signal::new(Option::<Surface>::None));
    use_context_provider(builtins);
    let events = use_hook(|| AppEvents::channel().0);
    use_context_provider({
        let events = events.clone();
        move || events
    });
    let ws = Workspace::use_current();
    let slot = use_context::<SurfaceSlot>();
    rsx! {
        Shell {}
        button {
            "data-a11y-id": "toggle",
            onclick: move |_| assert!(router::route(ws, &events, slot, ids::PERF_HUD_TOGGLE)),
        }
    }
}

fn line(h: &Harness, id: &str) -> String {
    h.text_of(h.by_a11y_id(id).unwrap_or_else(|| panic!("no {id}")))
}

#[test]
#[serial]
fn the_hud_toggles_on_with_its_four_lines_and_off_again() {
    let mut h = Harness::new(Host, ());
    h.settle();
    assert!(h.by_a11y_id("perf-hud").is_none(), "closed until asked for");

    h.click("toggle");
    h.settle();
    assert!(h.by_a11y_id("perf-hud").is_some());
    assert_eq!(line(&h, "perf-hud-fps"), "\u{2014} fps", "a dash, never 0");
    assert_eq!(
        line(&h, "perf-hud-frame-ms"),
        "p50 \u{2014} / p95 \u{2014} / p99 \u{2014} ms"
    );
    let rss = line(&h, "perf-hud-rss");
    assert!(rss.starts_with("rss ") && rss.ends_with('B'), "{rss}");
    assert_eq!(
        line(&h, "perf-hud-pages"),
        "pages \u{2014}",
        "no grid showing"
    );
    let html = h.html();
    let hud = &html[html.find("d0-perf-hud").expect("the HUD's markup")..];
    let hud = &hud[..hud.find("</div></div>").expect("the HUD's end")];
    assert!(!hud.contains("tabindex"), "not a tab stop: {hud}");
    assert!(html.contains(r#"aria-hidden="true""#));

    h.click("toggle");
    h.settle();
    assert!(h.by_a11y_id("perf-hud").is_none(), "gone when toggled off");
}
