//! Opening the window's DuckDB session, and opening files into it.
//!
//! This is the whole of the plan's step 5.9 for the session path. The GPUI
//! build could not simply `await` a session: it built one on a tokio task and
//! posted the result back through `main_bridge::MainThreadDispatcher`, because
//! a `block_on` from inside a polled task aborts the process and gpui offered
//! no way to resume on the UI thread. Dioxus `spawn` runs the future on the
//! same thread as the component and lets it write a signal directly, so the
//! dispatcher, the `WeakEntity` upgrade dance and the "window stays Booting if
//! the dispatcher is missing" failure mode all disappear.
//!
//! What does NOT change is the discipline: DuckDB work never runs before the
//! first frame, and every result is applied through a monotonic guard so a slow
//! answer cannot overwrite a newer one.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use parking_lot::Mutex;

use dat0_core::file_drop::{DropOutcome, handle_drop};
use dat0_core::session::Session;
use dat0_core::session::dock_layout::DockLayout;
use dat0_core::session::slot::SessionSlot;

use crate::state::{TabView, Workspace};

/// How long a layout change waits before it reaches disk.
///
/// The GPUI shell debounced by the same 500 ms, for the same reason: a splitter
/// drag emits a size on every pointer move, and a write per move is sixty
/// `session.json` rewrites a second.
const LAYOUT_DEBOUNCE: Duration = Duration::from_millis(500);

/// Restore this window's dock layout from its session, then persist every
/// change back to it.
///
/// The whole of what `window/dock.rs::persist_dock_layout` did, minus the
/// polling. GPUI's `Dock` emitted nothing on resize, so the shell serialized
/// `DockArea::dump()` every frame and diffed it; here the layout *is* a signal,
/// so an effect that reads it runs exactly when it changes and never otherwise.
///
/// Two pieces of non-reactive state, deliberately:
///
/// * `known` is the last layout this window has adopted or written. Without it
///   the restore's own `ws.layout.set` would be seen as a user change and
///   written straight back, and an unchanged layout would rewrite the file on
///   every unrelated re-render.
/// * `generation` supersedes an in-flight debounce, so a drag that emits fifty
///   sizes performs one write, of the last one — not fifty staggered writes
///   racing to land out of order.
///
/// Both are `Rc` cells rather than signals: writing a signal here would
/// re-trigger the very effect that reads it.
pub fn use_layout_persistence(ws: Workspace) {
    let mut ws = ws;
    let known = use_hook(|| Rc::new(RefCell::new(Option::<DockLayout>::None)));
    let generation = use_hook(|| Rc::new(Cell::new(0u64)));

    use_effect(move || {
        // Read both signals unconditionally, before any early return: an
        // effect subscribes to what it reads, so a `layout` read behind a
        // `?`-style bail would leave later layout changes unobserved.
        let layout = ws.layout.read().clone();
        let slot = ws.session.read().clone();
        let Some(session) = slot.ready().cloned() else {
            return;
        };

        // First pass after the session lands: adopt what is on disk. A session
        // with no layout yields the default, which is what a fresh workspace
        // should show.
        if known.borrow().is_none() {
            let restored = restore_layout(&session);
            *known.borrow_mut() = Some(restored.clone());
            ws.layout.set(restored);
            return;
        }

        if known.borrow().as_ref() == Some(&layout) {
            return;
        }
        *known.borrow_mut() = Some(layout.clone());

        let epoch = generation.get() + 1;
        generation.set(epoch);
        let generation = generation.clone();
        spawn(async move {
            tokio::time::sleep(LAYOUT_DEBOUNCE).await;
            if generation.get() != epoch {
                return;
            }
            write_layout(&session, layout);
        });
    });
}

/// The layout a window should open with, given its session.
///
/// A session with no layout — every session before the user has touched a
/// dock — yields the default, which opens the sidebar. That is a deliberate
/// `unwrap_or_default` rather than a `None` the caller has to handle: there is
/// no third state, and a window with no layout at all is not renderable.
pub fn restore_layout(session: &Arc<Mutex<Session>>) -> DockLayout {
    session.lock().dock_layout().cloned().unwrap_or_default()
}

/// Write `layout` to the session file.
///
/// Logged rather than surfaced: a layout that fails to persist costs the user
/// a dock width, and a banner for it would be noise beside whatever real
/// failure — a full disk, a removed state root — actually caused it.
pub fn write_layout(session: &Arc<Mutex<Session>>, layout: DockLayout) {
    if let Err(e) = session.lock().set_dock_layout(Some(layout)) {
        tracing::warn!(error = %format!("{e:#}"), "dock layout persist failed");
    }
}

/// Where a set of paths goes, given the window's session slot.
///
/// Pure, and separate from [`open_paths`] for one reason: it is the whole of
/// the EN4 decision, and a `Signal` cannot be read outside a `VirtualDom`, so
/// the rule would otherwise only ever be checked by rendering a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRoute {
    /// The session is live: register now.
    Register,
    /// DuckDB is still opening. Hold the gesture — a drop the user already
    /// made must not evaporate because the engine was slow.
    Queue,
    /// The session failed. Holding the gesture would mean replaying it into a
    /// session that will never exist; the retry starts from a clean window.
    Discard,
}

pub fn route(slot: &SessionSlot) -> OpenRoute {
    match slot {
        SessionSlot::Ready(_) => OpenRoute::Register,
        SessionSlot::Booting => OpenRoute::Queue,
        SessionSlot::Failed(_) => OpenRoute::Discard,
    }
}

/// The banner a failed `Session::new` raises.
///
/// It is the ONLY way out of [`SessionSlot::Failed`] — the boot deliberately
/// never retries on its own, because the usual causes are a full disk or a
/// locked state root and a retry loop would hammer both. Hence
/// `dismissible: false`: a dismissed banner would strand the window with no
/// engine and no way to ask for one.
///
/// The action is routed by id rather than by calling [`retry`], so the button,
/// the command palette and the menu all reach the same handler in
/// `router::route`.
pub fn failure_banner(message: &str) -> dat0_core::error_ux::Banner {
    dat0_core::error_ux::Banner {
        dismissible: false,
        ..dat0_core::error_ux::Banner::error(dat0_i18n::t("session.failed"), message.to_string())
            .with_primary(
                dat0_i18n::t("session.retry"),
                dat0_core::actions::builtin::ids::SESSION_RETRY,
            )
    }
}

/// Apply a finished slot to the window: publish it, tell the status bar, then
/// either drain the queue or raise the failure banner.
///
/// One function, so the cold boot and the retry cannot disagree about what a
/// failure looks like — under GPUI they were two code paths and only one of
/// them cleared the queue.
async fn land(ws: Workspace, slot: SessionSlot) {
    let mut ws = ws;
    let failure = slot.failure().map(str::to_string);
    ws.session.set(Arc::new(slot));
    ws.status.write().engine_ok = failure.is_none();

    if let Some(message) = failure {
        ws.pending_open.write().clear();
        dat0_core::error_ux::push(failure_banner(&message));
        return;
    }

    // Front to back, in one call: `open_paths` binds the LAST file it
    // registers, so a drain that walked the queue backwards would leave the
    // first-dropped file active instead of the last.
    let queued = std::mem::take(&mut *ws.pending_open.write());
    if !queued.is_empty() {
        open_paths(ws, queued).await;
    }
}

/// Open this window's session, then drain anything queued while it booted.
///
/// Mount-once: `use_future` runs one task for the component's life.
pub fn use_session(ws: Workspace, cli_paths: Vec<PathBuf>) {
    let mut ws = ws;
    // Hangs off the same hook so a window cannot get a session without also
    // getting its layout: they are one lifecycle, not two.
    use_layout_persistence(ws);
    use_future(move || {
        let paths = cli_paths.clone();
        async move {
            // The launch arguments are simply the first entries in the queue.
            // Treating them as a separate channel is what let the GPUI build
            // open the CLI files and swallow a drop made while it did.
            if !paths.is_empty() {
                ws.pending_open.write().extend(paths);
            }

            let Some(state_root) = dat0_core::globals::state_root() else {
                // The state root is installed by `launch::main` before any
                // window exists. Missing means the process was started some
                // other way, and a session opened against a guessed directory
                // is worse than a visible failure.
                land(
                    ws,
                    SessionSlot::Failed("state root not installed".to_string()),
                )
                .await;
                return;
            };
            let budget = dat0_core::settings::budget::configured();
            let id = ws.window_id;

            let slot = match Session::new_with_id(state_root, budget, id).await {
                Ok(s) => SessionSlot::Ready(Arc::new(Mutex::new(s))),
                // `{e:#}` renders the whole anyhow chain — `Session::new`'s
                // context lines are the only diagnosis a user gets here.
                Err(e) => SessionSlot::Failed(format!("{e:#}")),
            };
            land(ws, slot).await;
        }
    });
}

/// Register `paths` as tables and append a tab for each.
///
/// Shared by the drop handler, the file picker and the CLI paths, so "open" and
/// "drop" cannot drift — this is the one function that turns a path into a tab.
pub async fn open_paths(ws: Workspace, paths: Vec<PathBuf>) {
    let mut ws = ws;
    // Scoped: the read guard must be gone before the queue is written.
    let (where_to, ready) = {
        let slot = ws.session.read();
        (route(&slot), slot.ready().cloned())
    };
    let session = match where_to {
        OpenRoute::Register => match ready {
            Some(s) => s,
            // `route` said Ready, so this is unreachable; degrade rather than
            // unwrap, because a panic here is a dead window.
            None => return,
        },
        OpenRoute::Queue => {
            // Dropped onto a window whose session is still opening. APPEND:
            // two drops during one boot are two gestures and must open as two
            // tabs, in the order they were made.
            tracing::debug!(count = paths.len(), "open_paths: queued while booting");
            ws.pending_open.write().extend(paths);
            return;
        }
        OpenRoute::Discard => {
            tracing::info!(
                count = paths.len(),
                "open_paths: session failed; drop ignored"
            );
            return;
        }
    };

    for outcome in handle_drop(paths, session).await {
        match outcome {
            DropOutcome::Registered {
                table_name,
                source_path,
            } => {
                ws.tabs.write().push(TabView {
                    table: table_name,
                    path: Some(source_path),
                });
                let last = ws.tabs.read().len() - 1;
                ws.active.set(Some(last));
            }
            DropOutcome::Unsupported { path, extension } => {
                let what = extension.unwrap_or_default();
                tracing::info!(?path, extension = %what, "unsupported file dropped");
                dat0_core::error_ux::push(dat0_core::error_ux::Banner::warning(dat0_i18n::t(
                    "drop.unsupported",
                )));
            }
            DropOutcome::EngineError { path, error } => {
                tracing::warn!(?path, %error, "register failed");
                dat0_core::error_ux::push(dat0_core::error_ux::Banner::error(
                    dat0_i18n::t("drop.register_failed"),
                    error,
                ));
            }
            other => {
                // The import wizard's ambiguous-sniff outcome. Routed by the
                // wizard surface, which owns the mapping UI.
                tracing::info!(?other, "drop needs the import wizard");
            }
        }
    }
}

/// Rebuild the window's session after `Session::new` failed.
///
/// Deliberately user-driven, and deliberately not automatic: the usual causes
/// are a full disk or a locked state root, and a retry loop would hammer both.
/// The `SessionSlot::Failed` banner is the only way to reach it.
pub fn retry(ws: Workspace) {
    let mut ws = ws;
    if ws.session.read().failure().is_none() {
        return;
    }
    ws.session.set(Arc::new(SessionSlot::Booting));
    spawn(async move {
        let Some(state_root) = dat0_core::globals::state_root() else {
            land(
                ws,
                SessionSlot::Failed("state root not installed".to_string()),
            )
            .await;
            return;
        };
        let budget = dat0_core::settings::budget::configured();
        let slot = match Session::new_with_id(state_root, budget, ws.window_id).await {
            Ok(s) => SessionSlot::Ready(Arc::new(Mutex::new(s))),
            Err(e) => SessionSlot::Failed(format!("{e:#}")),
        };
        land(ws, slot).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_core::session::dock_layout::DockLayout;

    const BUDGET: u64 = 128 * 1024 * 1024;

    async fn session_in(dir: &std::path::Path) -> Arc<Mutex<Session>> {
        Arc::new(Mutex::new(
            Session::new(dir, BUDGET).await.expect("Session::new"),
        ))
    }

    fn touched() -> DockLayout {
        DockLayout {
            console_open: true,
            charts_visible: true,
            sidebar_open: false,
            sidebar_size: Some(291),
            sections_collapsed: ["packages".to_string()].into_iter().collect(),
            ..DockLayout::default()
        }
    }

    #[tokio::test]
    async fn a_window_opens_on_the_layout_its_session_persisted() {
        let tmp = tempfile::tempdir().unwrap();
        let session = session_in(tmp.path()).await;
        session
            .lock()
            .set_dock_layout(Some(touched()))
            .expect("seed the session");

        assert_eq!(
            restore_layout(&session),
            touched(),
            "a window adopts the layout its session is carrying"
        );
    }

    #[tokio::test]
    async fn a_session_with_no_layout_opens_the_sidebar() {
        // The default is not "everything closed": a workbench whose catalog is
        // hidden on first launch looks broken. This is the branch every new
        // window takes.
        let tmp = tempfile::tempdir().unwrap();
        let session = session_in(tmp.path()).await;
        assert!(session.lock().dock_layout().is_none(), "precondition");

        let opened = restore_layout(&session);
        assert_eq!(opened, DockLayout::default());
        assert!(opened.sidebar_open);
        assert!(!opened.console_open);
        assert!(!opened.right_open());
    }

    #[tokio::test]
    async fn a_written_layout_reaches_the_session_file() {
        // Through the file, not the live session: re-reading the value just
        // written would prove only that assignment works. The disk-format half
        // is `dat0-core/tests/dock_layout_persist.rs`; this proves the UI's
        // own write path reaches it.
        let tmp = tempfile::tempdir().unwrap();
        let session = session_in(tmp.path()).await;

        write_layout(&session, touched());

        let path = session.lock().home.root_dir().join("session.json");
        let raw = std::fs::read_to_string(path).expect("session.json exists");
        let on_disk = dat0_core::session::migrate::load_str(&raw)
            .expect("session.json parses")
            .dock_layout;
        assert_eq!(on_disk, Some(touched()));
    }
}
