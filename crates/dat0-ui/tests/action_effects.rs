//! Every command dat0 offers changes something.
//!
//! This file used to be `action_routing.rs`, and it asserted that the router
//! *claimed* every palette command. A handler that only logs claims its id, so
//! the gate stayed green while 22 of the 40 commands did nothing (PD-023). Now
//! each command the palette offers is performed inside a real shell and must
//! leave a trace: a changed DOM, an event on the bus, a new theme.
//!
//! The commands that cannot pass that yet are listed in `router::UNWIRED`,
//! which every surface consults before offering one — the palette hides them,
//! the menu bar and the grid's context menu show them disabled. That list is a
//! ratchet: [`the_unwired_list_only_shrinks`] fails if it grows, and wiring a
//! command means deleting its id there and giving it a row in [`SETUP`] if it
//! needs one.
//!
//! The routing runs **inside** the component tree, because that is where the
//! shell's command handler is installed — the shell owns the grid's selection
//! and the console's tabs, and a router that could be called from outside the
//! tree would be a router that could not reach them.

mod support;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::actions::builtin::ids;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::command_palette;
use dat0_core::events::AppEvents;
use dat0_core::session::slot::SessionSlot;
use dat0_ui::components::command_palette::offered;
use dat0_ui::components::shell::Shell;
use dat0_ui::router::{self, Surface, SurfaceSlot, UNWIRED};
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use serial_test::serial;

use support::Harness;

fn builtins() -> ActionRegistry {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
    reg
}

/// What a command needs in place before its effect can show.
#[derive(Clone, PartialEq, Debug)]
enum Setup {
    /// Route these first.
    Route(&'static [&'static str]),
    /// A window whose session failed to open: Retry does nothing otherwise.
    FailedSession,
    /// An import in flight: Cancel Import does nothing otherwise.
    ActiveImport,
    /// No dialog up. A fresh profile is a first run, and the first-run tour
    /// opens itself on mount, so Take a Tour would find it already open.
    NoModal,
}

/// Commands whose effect needs something in place first. Everything else is
/// performed on a freshly mounted shell.
const SETUP: &[(&str, Setup)] = &[
    // The tab strip is only drawn while the console is open.
    (ids::SQL_NEW_TAB, Setup::Route(&[ids::CONSOLE_TOGGLE])),
    // Closing the last tab is refused, so there must be a second.
    (
        ids::SQL_CLOSE_TAB,
        Setup::Route(&[ids::CONSOLE_TOGGLE, ids::SQL_NEW_TAB]),
    ),
    (ids::SESSION_RETRY, Setup::FailedSession),
    (ids::IMPORT_CANCEL, Setup::ActiveImport),
    (ids::ONBOARDING_TAKE_TOUR, Setup::NoModal),
];

/// Offered commands whose effect needs something the headless harness does
/// not have, and where each one is checked instead.
const NOT_HEADLESS: &[(&str, &str)] = &[
    (
        ids::FILE_OPEN,
        "opens the platform file picker, which needs a window system",
    ),
    (
        ids::SETTINGS_OPEN,
        "opens its own OS window; examples/settings_window_probe.rs drives it",
    ),
    (
        ids::SAMPLE_DATA_RETRY_TAXI,
        "downloads over the network; dat0-core's tests/sample_data_fetch.rs covers the fetch",
    ),
    (
        ids::SQL_CANCEL,
        "interrupts a running query, and a run needs a real session; \
         tests/console_run.rs cancels one",
    ),
    (
        ids::VIEW_UNDO,
        "undoes a view change, which needs a real session to make; \
         tests/grid_views.rs undoes one",
    ),
    (
        ids::VIEW_REDO,
        "redoes a view change, which needs a real session to make; \
         tests/grid_views.rs redoes one",
    ),
    (ids::VIEW_COPY, GRID_VERB),
    (ids::VIEW_CUT, GRID_VERB),
    (ids::VIEW_PASTE, GRID_VERB),
    (ids::VIEW_FILL_DOWN, GRID_VERB),
    (ids::VIEW_SET_NULL, GRID_VERB),
    (ids::VIEW_SET_VALUE, GRID_VERB),
    (ids::VIEW_DELETE_ROWS, GRID_VERB),
    (ids::VIEW_DELETE_COLUMN, GRID_VERB),
    (ids::VIEW_SAVE_AS_TABLE, GRID_VERB),
];

const GRID_VERB: &str = "acts on a selection over a table's rows, and a table needs a real \
     session; tests/grid_edits.rs performs each grid verb there";

/// The `UNWIRED` list as of the review that introduced it (2026-09-25).
/// Frozen here so the list can shrink but never grow; see
/// [`the_unwired_list_only_shrinks`].
const UNWIRED_AT_MOST: &[&str] = &[
    ids::LIVE_REFRESH,
    ids::WORKSPACE_OPEN,
    ids::WORKSPACE_SAVE,
    ids::PERF_HUD_TOGGLE,
];

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    setup: Option<Setup>,
    /// Routed when `go` is clicked.
    id: String,
}

/// The real shell, plus two buttons that route commands from inside the tree
/// and a readout of what the shell cannot show on its own.
#[component]
fn Host(props: HostProps) -> Element {
    Workspace::provide();
    let theme = Theme::provide(None);
    use_context_provider(|| Signal::new(Option::<Surface>::None));
    use_context_provider(builtins);
    let (events, rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, Rc::new(RefCell::new(rx)))
    });
    use_context_provider({
        let events = events.clone();
        move || events
    });

    let ws = Workspace::use_current();
    let slot = use_context::<SurfaceSlot>();
    let mut posted = use_signal(|| 0usize);
    let mut refused = use_signal(String::new);

    // Route `ids`, counting what lands on the bus: `window.new` is performed
    // by the window that drains it, so an event posted IS its effect here.
    let route = {
        let events = events.clone();
        move |ids: Vec<String>| {
            for id in ids {
                if !router::route(ws, &events, slot, &id) {
                    refused.write().push_str(&id);
                }
                while rx.borrow_mut().try_recv().is_ok() {
                    *posted.write() += 1;
                }
            }
        }
    };

    let setup = props.setup.clone();
    let on_setup = {
        let mut route = route.clone();
        move |_| match &setup {
            Some(Setup::Route(ids)) => route(ids.iter().map(|s| s.to_string()).collect()),
            Some(Setup::FailedSession) => {
                let mut session = ws.session;
                session.set(Arc::new(SessionSlot::Failed("injected".to_string())));
            }
            Some(Setup::ActiveImport) => {
                dat0_core::import_progress::set_active(
                    dat0_core::import_progress::ImportProgress::new(10),
                );
            }
            Some(Setup::NoModal) => {
                let mut modal = ws.modal;
                modal.set(None);
            }
            None => {}
        }
    };
    let id = props.id.clone();
    let mut route = route;
    let on_go = move |_| route(vec![id.clone()]);
    let theme_id = theme.tokens().id;

    rsx! {
        Shell {}
        button { "data-a11y-id": "setup", onclick: on_setup }
        button { "data-a11y-id": "go", onclick: on_go }
        div { "data-a11y-id": "posted", "{posted}" }
        div { "data-a11y-id": "refused", "{refused}" }
        div { "data-a11y-id": "theme", "{theme_id}" }
    }
}

/// Everything a command could have changed.
fn observe(h: &Harness) -> String {
    let posted = h.text_of(h.by_a11y_id("posted").expect("readout"));
    let theme = h.text_of(h.by_a11y_id("theme").expect("readout"));
    format!("posted={posted} theme={theme}\n{}", h.html())
}

/// Perform `id` on a freshly mounted shell, after its [`SETUP`] row if it has
/// one. `Err` names what went wrong.
fn perform(id: &str) -> Result<(), String> {
    let setup = SETUP.iter().find(|(s, _)| *s == id).map(|(_, s)| s.clone());
    perform_with(id, setup)
}

fn perform_with(id: &str, setup: Option<Setup>) -> Result<(), String> {
    let mut h = Harness::new(
        Host,
        HostProps {
            setup: setup.clone(),
            id: id.to_string(),
        },
    );
    h.settle();
    if setup.is_some() {
        h.click("setup");
    }
    let before = observe(&h);
    // A shell still changing on its own would make any command look like it
    // did something.
    h.settle();
    if observe(&h) != before {
        return Err("the shell kept changing with nothing routed".to_string());
    }
    h.click("go");
    h.settle();
    let refused = h.text_of(h.by_a11y_id("refused").expect("readout"));
    if !refused.is_empty() {
        return Err(format!("refused: {refused}"));
    }
    if observe(&h) == before {
        return Err("routed, and nothing changed".to_string());
    }
    Ok(())
}

#[test]
#[serial]
fn every_command_the_palette_offers_changes_something() {
    let reg = builtins();
    let offered = offered(&reg, "");
    assert!(
        offered.len() > 15,
        "the palette offers only {} commands — this gate is measuring nothing",
        offered.len()
    );

    let mut failures = Vec::new();
    for id in &offered {
        if NOT_HEADLESS.iter().any(|(n, _)| n == id) {
            continue;
        }
        if let Err(why) = perform(id) {
            failures.push(format!("{id}: {why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "these commands are offered and did nothing — wire them, or add them to \
         router::UNWIRED so no surface offers them:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
#[serial]
fn an_unregistered_id_is_refused_rather_than_swallowed() {
    // The other half: if `route` returned true for everything, a command that
    // did nothing would still read as claimed.
    assert_eq!(
        perform("nope.not.an.action"),
        Err("refused: nope.not.an.action".to_string())
    );
}

#[test]
#[serial]
fn a_command_that_does_nothing_is_caught() {
    // The gate's own control. Cancel Import with no import running is a no-op
    // by design, so if this passed, the comparison above would be passing
    // everything.
    assert_eq!(
        perform_with(ids::IMPORT_CANCEL, None),
        Err("routed, and nothing changed".to_string())
    );
}

#[test]
fn the_unwired_list_only_shrinks() {
    for id in UNWIRED {
        assert!(
            UNWIRED_AT_MOST.contains(id),
            "{id} was added to router::UNWIRED. That list only shrinks: wire the \
             command instead of hiding it"
        );
    }
    assert_eq!(
        UNWIRED.len(),
        UNWIRED_AT_MOST.len(),
        "an id left router::UNWIRED — delete it from UNWIRED_AT_MOST here too, so \
         it cannot quietly come back"
    );
}

#[test]
fn every_unwired_id_is_a_real_command() {
    let reg = builtins();
    for id in UNWIRED {
        assert!(
            reg.contains(id),
            "{id} is in router::UNWIRED but is not a registered action"
        );
    }
}

#[test]
fn the_palette_offers_nothing_unwired() {
    let reg = builtins();
    for id in offered(&reg, "") {
        assert!(
            router::is_wired(&id),
            "the palette offers {id}, which does nothing"
        );
    }
}

#[test]
fn the_exemptions_are_offered_commands() {
    // A stale entry exempts nothing while reading as if it does.
    let reg = builtins();
    let offered = offered(&reg, "");
    for (id, _) in NOT_HEADLESS {
        assert!(
            offered.contains(&id.to_string()),
            "{id} is exempt but not offered"
        );
    }
    for (id, _) in SETUP {
        assert!(
            offered.contains(&id.to_string()),
            "{id} has a setup but is not offered"
        );
    }
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

/// A modal whose reply is `ModalReply::new(|_| {})` looks like a feature and
/// throws the user's answer away — how History, Load, Save, Export and Live
/// Refresh shipped. The count may only go down. `src/visual/` is exempt: its
/// scenes are fixtures that render a modal and never submit it.
#[test]
fn discarded_modal_replies_only_decrease() {
    const AT_MOST: usize = 2;
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    collect(&src, &mut |path, text| {
        if path.components().any(|c| c.as_os_str() == "visual") {
            return;
        }
        for (n, line) in text.lines().enumerate() {
            if line.contains("ModalReply::new(|_| {})") {
                found.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    });
    assert!(
        found.len() <= AT_MOST,
        "{} modals discard their reply (at most {AT_MOST}); a new one is a feature \
         that looks finished and is not:\n  {}",
        found.len(),
        found.join("\n  ")
    );
    assert_eq!(
        found.len(),
        AT_MOST,
        "fewer discarded replies than recorded — lower AT_MOST to {} so they stay gone",
        found.len()
    );
}

fn collect(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path, &str)) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("read source");
            f(&path, &text);
        }
    }
}
