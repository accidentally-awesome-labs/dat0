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
use dat0_core::events::{AppEvent, AppEvents, Opening};

use crate::state::{Modal, Workspace};

/// Registered actions that do nothing in this build.
///
/// Each has a descriptor, so the palette, the menu bar, the grid's context
/// menu and the keymap all know it, but its handler only logs, opens a dialog
/// whose reply is thrown away, or hands its input to a path that refuses it
/// (PD-023). Offering them is the failure this router exists to prevent, one
/// level down: the id is claimed and nothing happens.
///
/// So every surface that offers a command asks [`is_wired`] first. The list is
/// a ratchet: `tests/action_effects.rs` fails if it grows, and an id leaves it
/// in the same change as the test that shows its effect.
pub const UNWIRED: &[&str] = &[];

/// Whether `id` does something in this build. See [`UNWIRED`].
pub fn is_wired(id: &str) -> bool {
    !UNWIRED.contains(&id)
}

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
        ids::WINDOW_NEW => events.send(AppEvent::OpenWindow(Opening::files(Vec::new()))),
        ids::SIDEBAR_TOGGLE => ws.toggle_sidebar(),
        ids::INSPECTOR_TOGGLE => {
            let open = ws.layout.read().inspector_visible;
            ws.layout.write().inspector_visible = !open;
        }
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
            let theme = crate::theme::Theme::current();
            let next = if theme.tokens().id == "light" {
                "dark"
            } else {
                "light"
            };
            // Every window, and the next launch too.
            crate::theme::choose(next);
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
                    crate::workspace_open::open(ws, &events, folder);
                }
            });
        }
        ids::WORKSPACE_SAVE => {
            if !crate::launch::has_desktop() {
                tracing::debug!("workspace.save: no window system, nothing to show");
                return true;
            }
            spawn(async move {
                if let Some(folder) = crate::files::pick_folder_to_save().await {
                    crate::workspace_save::save(ws, folder).await;
                }
            });
        }

        // ── Modals ─────────────────────────────────────────────────────────
        ids::ONBOARDING_TAKE_TOUR => ws.modal.set(Some(Modal::Onboarding)),
        ids::REPORT_BUG => crate::crash_flow::report_bug(ws),
        ids::SETTINGS_OPEN => {
            // Its own OS window, not the modal slot: settings is a nine-section
            // surface a user keeps open beside the workbench, and the slot
            // holds one dialog at a time.
            if !crate::launch::has_desktop() {
                tracing::debug!("settings.open: no window system, nothing to open");
                return true;
            }
            // The settings window belongs to no workbench window, so its
            // controls post on the process bus and reach whichever window was
            // focused last by then — not this one, which may have closed.
            let events = crate::launch::process_bus().unwrap_or_else(|| events.clone());
            spawn(async move {
                crate::components::settings_ui::open_settings_window(events).await;
            });
        }

        // ── Session ────────────────────────────────────────────────────────
        ids::SESSION_RETRY => crate::session_boot::retry(ws),
        // What windows that are gone left behind. Open brings a session back
        // in a window of its own, with its tabs, their views and its SQL. The
        // new window belongs to no workbench window, so it is asked for on the
        // process bus, and still opens if this one closes first. Resume
        // finishes a Save Workspace that was cut short, found among the recent
        // workspaces, and opens the workspace.
        ids::RECOVERY_REVIEW => {
            use crate::components::modals::ModalOutcome;
            let process = crate::launch::process_bus().unwrap_or_else(|| events.clone());
            let events = events.clone();
            ws.modal.set(Some(Modal::Recovery {
                scratch_root: dat0_core::globals::state_root()
                    .map(|p| p.join("scratch"))
                    .unwrap_or_default(),
                recent_roots: dat0_core::globals::recents_snapshot(),
                reply: crate::components::modals::ModalReply::new(move |outcome| match outcome {
                    ModalOutcome::RecoveryOpen(dir) => {
                        process.send(AppEvent::OpenWindow(Opening::Recover { dir }))
                    }
                    ModalOutcome::RecoveryResume(root) => {
                        crate::workspace_open::resume(ws, &events, root)
                    }
                    _ => {}
                }),
            }));
        }

        // ── Modals the shell can open from workspace state alone ───────────
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
