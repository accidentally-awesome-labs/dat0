//! The action router: what an action id actually does.
//!
//! Every entry point — menu item, key chord, command palette row, banner
//! button — resolves to an id and posts `AppEvent::RunAction`. This is the one
//! place that turns an id into behaviour, which is what makes those four
//! surfaces impossible to desynchronise: a menu item whose action does not
//! exist cannot ship, because there is exactly one table and
//! `menu::tests::local_ids_cannot_collide_with_action_ids` fails the build.
//!
//! The GPUI build had no such place. Its actions were dispatched through gpui's
//! own `Action` tree, so a handler could live anywhere a `cx.listener` could be
//! attached, and several ids ended up with no handler at all — silently, since
//! an unhandled `Action` is a no-op.

use std::rc::Rc;

use dioxus::prelude::*;

use dat0_core::actions::builtin::ids;
use dat0_core::events::{AppEvent, AppEvents};

use crate::state::{Modal, Workspace};

/// A shell-installed handler for the actions whose state the shell owns.
///
/// Most commands are window state and this module performs them directly. The
/// rest — the grid's edit verbs, the console's tab verbs, chart export — need
/// signals that belong to a surface, and hoisting those into [`Workspace`] just
/// to reach them here would make every surface's private state public so one
/// `match` could see it.
///
/// So the shell installs a closure over its own signals and the router falls
/// through to it. One indirection, and the state stays where it is used.
#[derive(Clone)]
pub struct Surface(Rc<dyn Fn(&str) -> bool>);

impl Surface {
    pub fn new(f: impl Fn(&str) -> bool + 'static) -> Self {
        Self(Rc::new(f))
    }

    fn call(&self, id: &str) -> bool {
        (self.0)(id)
    }
}

impl PartialEq for Surface {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// The slot the shell writes its handler into.
///
/// Provided by `App` rather than by the shell, because the bus drain that calls
/// [`route`] lives in `App` and a child's context is not visible to its parent.
pub type SurfaceSlot = Signal<Option<Surface>>;

/// Perform `id`. Returns false when nothing claims it, which can only mean a
/// descriptor was registered with no handler — the failure mode this module
/// exists to make impossible to ship silently.
pub fn route(ws: Workspace, events: &AppEvents, surface: SurfaceSlot, id: &str) -> bool {
    let mut ws = ws;
    match id {
        // ── Window and shell ───────────────────────────────────────────────
        ids::WINDOW_NEW => events.send(AppEvent::OpenWindow { paths: Vec::new() }),
        ids::SIDEBAR_TOGGLE => ws.toggle_sidebar(),
        ids::CONSOLE_TOGGLE => {
            let open = ws.layout.read().console_open;
            ws.layout.write().console_open = !open;
        }
        ids::CHART_VISUALIZE => {
            let open = ws.layout.read().charts_visible;
            ws.layout.write().charts_visible = !open;
        }
        ids::THEME_TOGGLE => {
            // Cycles light → dark → light. High contrast is deliberately not
            // in the cycle: it is an accessibility choice made once in
            // settings, not something to land on by pressing a key twice.
            let mut theme = crate::theme::Theme::use_current();
            let next = if theme.tokens().id == "light" {
                "dark"
            } else {
                "light"
            };
            theme.set(next);
        }
        ids::RECENTS_SHOW => ws.palette.set(true),

        // ── Files ──────────────────────────────────────────────────────────
        ids::FILE_OPEN => {
            if !crate::launch::has_desktop() {
                tracing::debug!("file.open: no window system, nothing to show");
                return true;
            }
            spawn(async move {
                let picked = crate::files::pick_data_files().await;
                if !picked.is_empty() {
                    crate::session_boot::open_paths(ws, picked).await;
                }
            });
        }
        ids::WORKSPACE_OPEN => {
            if !crate::launch::has_desktop() {
                tracing::debug!("workspace.open: no window system, nothing to show");
                return true;
            }
            let events = events.clone();
            spawn(async move {
                if let Some(folder) = crate::files::pick_folder().await {
                    events.send(AppEvent::OpenWindow {
                        paths: vec![folder],
                    });
                }
            });
        }

        // ── Modals ─────────────────────────────────────────────────────────
        ids::ONBOARDING_TAKE_TOUR => ws.modal.set(Some(Modal::Onboarding)),
        ids::SETTINGS_OPEN => {
            // Its own OS window, not the modal slot: settings is a nine-section
            // surface a user keeps open beside the workbench, and the slot
            // holds one dialog at a time.
            if !crate::launch::has_desktop() {
                tracing::debug!("settings.open: no window system, nothing to open");
                return true;
            }
            let events = events.clone();
            spawn(async move {
                crate::components::settings_ui::open_settings_window(events).await;
            });
        }

        // ── Session ────────────────────────────────────────────────────────
        ids::SESSION_RETRY => crate::session_boot::retry(ws),

        // ── Modals the shell can open from workspace state alone ───────────
        ids::LIVE_REFRESH => ws.modal.set(Some(Modal::LiveRefresh {
            dropped_edits: 0,
            dropped_deletes: 0,
            reply: crate::components::modals::ModalReply::new(|_| {}),
        })),
        ids::IMPORT_CANCEL => {
            // Idempotent by design: cancelling an import that already finished
            // is a no-op, not an error, because the user cannot know which.
            dat0_core::import_progress::cancel_active();
        }
        ids::SAMPLE_DATA_RETRY_TAXI => {
            // The one sample that can fail: it is fetched, not embedded. Retry
            // is the banner's action, and it takes the same path the card does.
            if let Some(entry) = dat0_core::sample_data::entries()
                .into_iter()
                .find(|e| matches!(e.kind, dat0_core::sample_data::SampleKind::Remote { .. }))
            {
                crate::components::shell::open_sample(ws, entry.kind);
            }
        }

        // Everything else belongs to a surface.
        other => {
            return surface.peek().as_ref().is_some_and(|s| s.call(other));
        }
    }
    true
}
