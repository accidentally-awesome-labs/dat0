//! The component tree.
//!
//! [`App`] is the root every window mounts. It owns exactly three things —
//! the asset handler, the theme, and the event-bus drain — and delegates
//! everything visible to [`shell::Shell`].

pub mod about;
pub mod ai;
pub mod banner;
pub mod charts;
pub mod command_palette;
pub mod connections;
pub mod crash_report;
pub mod dock;
pub mod empty_state;
pub mod export_dialog;
pub mod filter_popover;
pub mod grid;
pub mod import_progress;
pub mod import_wizard;
pub mod inspector;
pub mod live_refresh;
pub mod modals;
pub mod name_prompt;
pub mod onboarding;
pub mod pane;
pub mod pipeline_bar;
pub mod query_library;
pub mod recovery;
pub mod saved_queries;
pub mod settings_ui;
pub mod shell;
pub mod sidebar;
pub mod sql_console;
pub mod update_ui;
pub mod workspace_in_use;

use dioxus::prelude::*;
use futures::StreamExt as _;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, AppEvents, Opening};

use crate::launch::Boot;
use crate::theme::{Theme, ThemeStyle};

/// The root of a window.
#[component]
pub fn App() -> Element {
    // Every asset — stylesheet, fonts, icons, the CodeMirror bundle — is served
    // from the binary. Registered per window because the handler registry is
    // per webview.
    dioxus::desktop::use_asset_handler("dat0", crate::protocol::serve);

    let boot = use_context::<Boot>();
    Theme::provide(None);
    let opening = use_hook(|| boot.take_opening());
    let ws = crate::state::Workspace::provide_for(&opening);
    // The shell installs its own command handler here once it mounts. Provided
    // by `App` rather than by the shell, because the bus drain is App's and a
    // child's context is invisible to its parent.
    let surface = use_context_provider(|| Signal::new(Option::<crate::router::Surface>::None));
    // The session opens after the first frame, on what this window was opened
    // for: the CLI's paths go to the first window only.
    crate::session_boot::use_session_on(ws, opening);
    use_context_provider(|| boot.registry.clone());
    let events = use_window_bus(boot.clone(), ws, surface);

    // Which window the menu bar acts on: the one focused last. dioxus hands a
    // handler only its own window's window events.
    {
        let windows = boot.windows.clone();
        let me = ws.window_id;
        dioxus::desktop::use_wry_event_handler(move |ev, _| {
            use dioxus::desktop::tao::event::{Event, WindowEvent};
            if let Event::WindowEvent {
                event: WindowEvent::Focused(true),
                ..
            } = ev
            {
                windows.focused(me);
            }
        });
    }

    // A menu click is `registry.dispatch(id, &events)` and nothing else,
    // because every item was created with its action id as its `muda` id.
    // There is no second table to keep in step — which is what let the GPUI
    // build ship menu items whose action existed nowhere.
    //
    // Every window registers this handler and every handler sees every click:
    // the menu bar is one per process on macOS, and dioxus hands each menu
    // event to every window's handler. Only the registry's target acts, or one
    // click would run the command once per open window (PD-027).
    {
        let registry = boot.registry.clone();
        let windows = boot.windows.clone();
        let me = ws.window_id;
        dioxus::desktop::use_muda_event_handler(move |ev| {
            if windows.target() != Some(me) {
                return;
            }
            let id = ev.id().0.clone();
            if registry.dispatch(&id, &events) {
                return;
            }
            // Not a registry action: window management and external links.
            menu_local(&id, &events);
        });
    }

    rsx! {
        ThemeStyle {}
        shell::Shell {}
    }
}

/// This window's command bus, and its share of the process bus.
///
/// Everything raised *in* a window — a palette row, a key chord, a banner
/// button, a click on its menu bar — is posted on the window's own bus,
/// provided here as the [`AppEvents`] context, and performed on this window's
/// state. There used to be one bus per process, drained by whichever window
/// mounted first, so a command raised in the second window acted on the
/// first, and closing the first silenced every menu, chord and palette row in
/// the rest (PD-027).
///
/// The process bus still exists for what belongs to no workbench window — a
/// second launch's paths, the settings window's controls. One window drains
/// it at a time through a [`crate::launch::ProcessBusLease`], which passes to
/// another window when the holder closes, and passes each event on to the
/// window it is for (see [`crate::launch::WindowRegistry`]).
///
/// Returns this window's bus. Separate from [`App`] so the headless harness can
/// mount two windows over one [`Boot`].
pub fn use_window_bus(
    boot: Boot,
    ws: crate::state::Workspace,
    surface: crate::router::SurfaceSlot,
) -> AppEvents {
    let (events, rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, std::rc::Rc::new(std::cell::RefCell::new(Some(rx))))
    });
    use_context_provider({
        let events = events.clone();
        move || events
    });

    // Listed for as long as the window is open, so what belongs to no window
    // can find it.
    {
        let windows = boot.windows.clone();
        let events = events.clone();
        let me = ws.window_id;
        use_hook(move || windows.opened(me, events));
        let windows = boot.windows.clone();
        use_drop(move || windows.closed(me));
    }

    // This window's bus, drained for as long as the window lives.
    {
        let boot = boot.clone();
        let events = events.clone();
        use_future(move || {
            let boot = boot.clone();
            let events = events.clone();
            let rx = rx.borrow_mut().take();
            async move {
                let Some(mut rx) = rx else { return };
                while let Some(ev) = rx.next().await {
                    handle(ev, &boot, ws, surface, &events).await;
                }
            }
        });
    }

    // The process bus, whenever this window holds it.
    use_future(move || {
        let boot = boot.clone();
        async move {
            loop {
                if let Some(mut lease) = crate::launch::ProcessBusLease::take(&boot) {
                    while let Some(ev) = lease.next().await {
                        pass_on(ev, &boot).await;
                    }
                    return;
                }
                boot.bus_free.notified().await;
            }
        }
    });

    events
}

/// Perform one process-bus event: open a window, or pass the event to the
/// workbench window it is for — the one it names, or else the registry's
/// target. A theme goes to every window, because it is an application-wide
/// choice.
async fn pass_on(ev: AppEvent, boot: &Boot) {
    let window = match ev {
        AppEvent::OpenWindow(opening) => {
            open_window(boot, opening).await;
            return;
        }
        AppEvent::ThemeChanged { id } => {
            boot.windows
                .broadcast(|| AppEvent::ThemeChanged { id: id.clone() });
            return;
        }
        AppEvent::RunAction { window, .. } => window,
        AppEvent::OpenPaths { window, .. } => Some(window),
        _ => None,
    };
    if !boot.windows.send(window, ev) {
        tracing::debug!(?window, "process event for a window that is not open");
    }
}

async fn open_window(boot: &Boot, opening: Opening) {
    if !crate::launch::has_desktop() {
        tracing::debug!("open window: no window system");
        return;
    }
    tracing::info!(?opening, "opening a window");
    let _ = crate::launch::open_window(boot.clone(), opening).await;
}

/// Perform one bus event on this window. `events` is the window's own bus, so
/// a command a routed action raises in turn stays in this window too.
async fn handle(
    ev: AppEvent,
    boot: &Boot,
    ws: crate::state::Workspace,
    surface: crate::router::SurfaceSlot,
    events: &AppEvents,
) {
    match ev {
        AppEvent::OpenWindow(opening) => open_window(boot, opening).await,
        AppEvent::ThemeChanged { id } => Theme::current().set(&id),
        // Raised off the UI thread, by a file watcher. One of each: a burst of
        // saves is one change to act on.
        AppEvent::Banner(banner) => {
            if !ws.banners.peek().iter().any(|b| b.title == banner.title) {
                ws.push_banner(banner);
            }
        }
        AppEvent::RunAction { id, .. } => {
            if !crate::router::route(ws, events, surface, id) {
                // Loud, because it can only mean a descriptor was registered
                // with no handler — the failure mode the router exists to make
                // impossible to ship silently.
                tracing::warn!(action = %id, "action has no handler");
            }
        }
        other => tracing::debug!(?other, "unhandled app event"),
    }
}

/// Menu items that are not registry actions: external links and recents. The
/// package verbs and the update check have no arm yet; `menu::UNWIRED_LOCAL`
/// builds them disabled.
fn menu_local(id: &str, events: &AppEvents) {
    use crate::menu::menu_ids;
    match id {
        // dat0.app is the project's domain; dat0.dev is not, and there is no
        // Discord yet — the site links the repository instead.
        menu_ids::DOCS => open_url("https://dat0.app/docs"),
        menu_ids::GITHUB => open_url("https://github.com/accidentally-awesome-labs/dat0"),
        other if other.starts_with(menu_ids::RECENT_PREFIX) => {
            let Some(ix) = other[menu_ids::RECENT_PREFIX.len()..].parse::<usize>().ok() else {
                return;
            };
            let recents = dat0_core::globals::recents_snapshot();
            if let Some(path) = recents.get(ix) {
                events.send(AppEvent::OpenWindow(Opening::files(vec![path.clone()])));
            }
        }
        other => tracing::info!(menu_id = other, "menu item has no handler yet"),
    }
}

fn open_url(url: &str) {
    if let Err(e) = dat0_core::platform::open_url(url) {
        tracing::warn!("could not open {url}: {e}");
    }
}

/// The registry, for any component that needs to look up or list actions.
pub fn registry() -> ActionRegistry {
    use_context()
}

/// The event bus, for any component that needs to post one.
pub fn events() -> AppEvents {
    use_context()
}
