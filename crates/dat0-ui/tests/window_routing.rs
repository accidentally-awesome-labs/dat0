//! A command acts on the window it was raised in (PD-027).
//!
//! There used to be one event bus per process, drained by whichever window
//! mounted first. A chord pressed in the second window toggled the first
//! window's sidebar, and closing the first window left the rest with nobody
//! draining the bus: menus, the palette, chords and a second launch's paths
//! all went nowhere.
//!
//! Two harnesses here share one [`Boot`] the way two windows share one process,
//! and each mounts the real `Shell` over `components::use_window_bus` — the
//! same bus `App` gives a desktop window. The drains are real `use_future`
//! tasks, polled by the harness's render passes; nothing is pumped by hand.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use support::{Harness, Key};

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvent;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::Surface;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;

#[derive(Clone, Props)]
struct WindowProps {
    boot: Boot,
    /// Where the window reports its id, so a test can focus it.
    id: Rc<Cell<Option<uuid::Uuid>>>,
}

impl PartialEq for WindowProps {
    fn eq(&self, _: &Self) -> bool {
        // Mounted once per harness and never handed new props.
        true
    }
}

/// One window: what `components::App` provides, minus the desktop-only parts
/// (the asset handler, the session, the menu handler).
#[component]
fn Window(props: WindowProps) -> Element {
    let theme = Theme::provide(None);
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    use_context_provider(|| props.boot.registry.clone());
    use_window_bus(props.boot.clone(), ws, surface);
    let id = props.id.clone();
    use_hook(move || id.set(Some(ws.window_id)));
    rsx! {
        div { "data-a11y-id": "theme", "data-theme": "{theme.tokens().id}" }
        Shell {}
    }
}

fn boot() -> Boot {
    let reg = ActionRegistry::new();
    register_all(&reg).expect("built-in actions register without conflict");
    Boot::new(reg, Vec::new())
}

/// Open a window on `boot`.
fn open(boot: &Boot) -> (Harness, uuid::Uuid) {
    let id = Rc::new(Cell::new(None));
    let mut h = Harness::new(
        Window,
        WindowProps {
            boot: boot.clone(),
            id: id.clone(),
        },
    );
    h.settle();
    (h, id.get().expect("the window reported its id"))
}

/// Let every window's tasks run until nothing changes.
fn settle(windows: &mut [&mut Harness]) {
    for _ in 0..3 {
        for h in windows.iter_mut() {
            h.settle();
        }
    }
}

fn sidebar_open(h: &Harness) -> bool {
    h.by_a11y_id("sidebar").is_some()
}

fn toggle_from_no_window() -> AppEvent {
    AppEvent::RunAction {
        id: ids::SIDEBAR_TOGGLE,
        window: None,
    }
}

/// Run `f` against a private config dir with the first-run tour answered, so
/// both windows render the plain shell. See `left_dock.rs`.
fn hermetic<R>(f: impl FnOnce() -> R) -> R {
    let tmp = TempDir::new().unwrap();
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock, and
    // nothing else in this binary reads the variable concurrently.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(tmp.path().join("settings.toml")),
        true,
    )
    .unwrap();
    let out = f();
    unsafe {
        match previous {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

#[test]
#[serial]
fn a_chord_acts_on_the_window_it_was_pressed_in() {
    hermetic(|| {
        let boot = boot();
        let (mut one, one_id) = open(&boot);
        let (mut two, _) = open(&boot);
        assert!(sidebar_open(&one) && sidebar_open(&two));

        // Whatever the registry last saw focused: a chord is the window's own.
        boot.windows.focused(one_id);
        two.key_at("window", Key::Character("b".into()), support::primary());
        settle(&mut [&mut one, &mut two]);
        assert!(!sidebar_open(&two), "the window the chord was pressed in");
        assert!(sidebar_open(&one), "and not the window that opened first");

        one.key_at("window", Key::Character("b".into()), support::primary());
        settle(&mut [&mut one, &mut two]);
        assert!(!sidebar_open(&one));
        assert!(!sidebar_open(&two), "untouched by the first window's chord");
    });
}

#[test]
#[serial]
fn a_command_from_no_window_acts_on_the_window_focused_last() {
    // The settings window's controls and the menu bar belong to no workbench
    // window. They reach the one the user was in.
    hermetic(|| {
        let boot = boot();
        let (mut one, one_id) = open(&boot);
        let (mut two, two_id) = open(&boot);

        boot.windows.focused(one_id);
        boot.events.send(toggle_from_no_window());
        settle(&mut [&mut one, &mut two]);
        assert!(!sidebar_open(&one), "the focused window");
        assert!(sidebar_open(&two), "and only that one");

        boot.windows.focused(two_id);
        boot.events.send(toggle_from_no_window());
        settle(&mut [&mut one, &mut two]);
        assert!(!sidebar_open(&two), "focus moved, and the command with it");
        assert!(!sidebar_open(&one), "one command, one window");
    });
}

#[test]
#[serial]
fn closing_the_first_window_leaves_the_rest_listening() {
    hermetic(|| {
        let boot = boot();
        let (one, one_id) = open(&boot);
        let (mut two, two_id) = open(&boot);

        // The first window holds the process bus, and has focus when it closes.
        boot.windows.focused(one_id);
        drop(one);
        assert_eq!(boot.windows.target(), Some(two_id));

        two.key_at("window", Key::Character("b".into()), support::primary());
        settle(&mut [&mut two]);
        assert!(
            !sidebar_open(&two),
            "chords still work in the window left open"
        );

        // The process bus passed to the second window, and so did the target.
        boot.events.send(toggle_from_no_window());
        settle(&mut [&mut two]);
        assert!(
            sidebar_open(&two),
            "a command from no window reaches the window left open"
        );
    });
}

#[test]
#[serial]
fn a_theme_chosen_in_settings_reaches_every_window() {
    // The settings window repaints itself and posts the theme "because a
    // theme is an application-wide choice" — and nothing drained it.
    hermetic(|| {
        let boot = boot();
        let (mut one, _) = open(&boot);
        let (mut two, _) = open(&boot);
        let theme = |h: &Harness| h.attr(h.by_a11y_id("theme").unwrap(), "data-theme");
        assert_eq!(theme(&one).as_deref(), Some("light"));

        boot.events
            .send(AppEvent::ThemeChanged { id: "dark".into() });
        settle(&mut [&mut one, &mut two]);
        assert_eq!(theme(&one).as_deref(), Some("dark"));
        assert_eq!(theme(&two).as_deref(), Some("dark"));
    });
}
