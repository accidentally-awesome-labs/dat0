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
use dat0_core::events::{AppEvent, AppEvents};

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
    let ws = crate::state::Workspace::provide();
    // The shell installs its own command handler here once it mounts. Provided
    // by `App` rather than by the shell, because the bus drain below is App's
    // and a child's context is invisible to its parent.
    let surface = use_context_provider(|| Signal::new(Option::<crate::router::Surface>::None));
    // The session opens after the first frame; the CLI's paths are opened into
    // it when it lands. Only the first window takes them.
    crate::session_boot::use_session(ws, boot.take_cli_paths());
    use_context_provider(|| boot.registry.clone());
    use_context_provider(|| boot.events.clone());

    // A menu click is `registry.dispatch(id, &events)` and nothing else,
    // because every item was created with its action id as its `muda` id.
    // There is no second table to keep in step — which is what let the GPUI
    // build ship menu items whose action existed nowhere.
    {
        let registry = boot.registry.clone();
        let events = boot.events.clone();
        dioxus::desktop::use_muda_event_handler(move |ev| {
            let id = ev.id().0.clone();
            if registry.dispatch(&id, &events) {
                return;
            }
            // Not a registry action: window management and external links.
            menu_local(&id, &events);
        });
    }

    // Drain the event bus. Exactly one window takes the receiver — the first to
    // mount — because there is one bus per process, not per window.
    let drain_boot = boot.clone();
    use_future(move || {
        let boot = drain_boot.clone();
        async move {
            let Some(mut rx) = boot.rx.lock().take() else {
                return;
            };
            while let Some(ev) = rx.next().await {
                handle(ev, &boot, ws, surface).await;
            }
        }
    });

    rsx! {
        ThemeStyle {}
        shell::Shell {}
    }
}

/// Perform one bus event.
async fn handle(
    ev: AppEvent,
    boot: &Boot,
    ws: crate::state::Workspace,
    surface: crate::router::SurfaceSlot,
) {
    match ev {
        AppEvent::OpenWindow { paths } => {
            tracing::info!(count = paths.len(), "opening a window");
            let _ = crate::launch::open_window(boot.clone(), paths).await;
        }
        AppEvent::RunAction { id, .. } => {
            if !crate::router::route(ws, &boot.events, surface, id) {
                // Loud, because it can only mean a descriptor was registered
                // with no handler — the failure mode the router exists to make
                // impossible to ship silently.
                tracing::warn!(action = %id, "action has no handler");
            }
        }
        other => tracing::debug!(?other, "unhandled app event"),
    }
}

/// Menu items that are not registry actions: external links and the package
/// verbs, which Phase 5 gives real handlers.
fn menu_local(id: &str, events: &AppEvents) {
    use crate::menu::menu_ids;
    match id {
        menu_ids::DOCS => open_url("https://dat0.dev/docs"),
        menu_ids::DISCORD => open_url("https://dat0.dev/discord"),
        other if other.starts_with(menu_ids::RECENT_PREFIX) => {
            let Some(ix) = other[menu_ids::RECENT_PREFIX.len()..].parse::<usize>().ok() else {
                return;
            };
            let recents = dat0_core::globals::recents_snapshot();
            if let Some(path) = recents.get(ix) {
                events.send(AppEvent::OpenWindow {
                    paths: vec![path.clone()],
                });
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
