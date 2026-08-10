//! Every command the palette offers actually does something.
//!
//! This is the gate the GPUI build had as `WINDOW_ROUTED` and lost in the
//! migration. Its absence was not theoretical: at the point this file was
//! written the registry held 40 actions and the router claimed 11, so 29
//! palette rows posted `AppEvent::RunAction`, hit no arm, and logged "action
//! has no handler" — a command list where three quarters of the entries were
//! decorative.
//!
//! A registered descriptor is a promise: it appears in the palette, it can
//! carry a chord, and a menu item can point at it. This suite makes breaking
//! that promise a build failure rather than a log line nobody reads.
//!
//! The routing runs **inside** the component tree, because that is where the
//! shell's command handler is installed — the shell owns the grid's selection
//! and the console's tabs, and a router that could be called from outside the
//! tree would be a router that could not reach them.

mod support;

use dioxus::prelude::*;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::command_palette;
use dat0_core::events::AppEvents;
use dat0_ui::components::shell::Shell;
use dat0_ui::router::{Surface, SurfaceSlot};
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;

use support::Harness;

fn builtins() -> ActionRegistry {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
    reg
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    ids: Vec<String>,
}

/// Mounts the real shell — which installs the command handler — then routes
/// every id and reports the refusals in a node the test can read.
#[component]
fn Host(props: HostProps) -> Element {
    Workspace::provide();
    Theme::provide(None);
    use_context_provider(|| Signal::new(Option::<Surface>::None));
    use_context_provider(builtins);
    use_context_provider(|| AppEvents::channel().0);

    let ws = Workspace::use_current();
    let events = use_context::<AppEvents>();
    let slot = use_context::<SurfaceSlot>();
    let ids = props.ids.clone();

    // An effect, not a hook: the shell installs its handler during its own
    // first render, so routing has to happen after the tree is built.
    //
    // Guarded to run exactly once. Routing a command CHANGES the state the
    // effect read — that is what a command is — so an unguarded effect would
    // re-run itself forever.
    let refused = use_signal(String::new);
    let done = use_hook(|| std::rc::Rc::new(std::cell::Cell::new(false)));
    {
        let mut refused = refused;
        use_effect(move || {
            if done.replace(true) {
                return;
            }
            let out: Vec<&str> = ids
                .iter()
                .filter(|id| !dat0_ui::router::route(ws, &events, slot, id))
                .map(String::as_str)
                .collect();
            refused.set(out.join(" "));
        });
    }

    rsx! {
        Shell {}
        div { "data-a11y-id": "refused", "{refused}" }
    }
}

fn refusals(ids: Vec<String>) -> Vec<String> {
    let mut h = Harness::new(Host, HostProps { ids });
    h.settle();
    let node = h.by_a11y_id("refused").expect("the readback node");
    h.text_of(node)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[test]
fn every_command_the_palette_offers_is_routed() {
    let reg = builtins();
    let offered: Vec<String> = command_palette::visible_items(&reg, "")
        .into_iter()
        .map(|d| d.id.to_string())
        .collect();
    assert!(
        offered.len() > 20,
        "the palette offers only {} commands — this gate is measuring nothing",
        offered.len()
    );

    let refused = refusals(offered);
    assert!(
        refused.is_empty(),
        "these commands are listed in the palette and do nothing: {refused:?}"
    );
}

#[test]
fn an_unregistered_id_is_refused_rather_than_swallowed() {
    // The other half: if `route` returned true for everything, the gate above
    // would pass while routing nothing.
    assert_eq!(
        refusals(vec!["nope.not.an.action".to_string()]),
        vec!["nope.not.an.action".to_string()]
    );
}

#[test]
fn every_hidden_id_is_still_a_real_command() {
    // `HIDDEN` suppresses palette rows for actions reachable another way. A
    // stale id there hides nothing while reading as if it does.
    let reg = builtins();
    for id in command_palette::HIDDEN {
        assert!(
            reg.iter().any(|d| d.id.as_str() == *id),
            "{id} is in HIDDEN but is not a registered action"
        );
    }
}
