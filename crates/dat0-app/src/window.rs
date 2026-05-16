//! GPUI window bootstrap for the dat0 desktop application.
//!
//! Composes the canonical `gpui` `Application::new().run(...)` entry point
//! (per `crates/gpui/examples/hello_world.rs` at the pinned 0.2.2 publish
//! commit) with the `gpui-component` requirements documented in
//! `docs/internal/gpui-api-notes.md` §0.2 (T0 spike): every window's first
//! layer must be a `gpui_component::Root`, and `gpui_component::init` must
//! run once before any window opens, otherwise dialogs / sheets /
//! notifications silently fail to render later (T17 depends on this).
//!
//! # Single-instance & multi-window (T12)
//!
//! `run_app` receives the `AppLock` singleton (already acquired in `main`)
//! and a list of CLI paths for the initial window. After the first window
//! opens, a tokio task is spawned to run the UDS server via
//! `AppLock::serve`. UDS-received `OpenWindowMessage`s are logged; the
//! visual cross-thread bridge to open a second GPUI window from a tokio task
//! is deferred (see `docs/deferrals.md` PD-010): `AsyncApp::update` is not
//! safe to call from a non-main thread (gpui uses `RefCell` for app state).
//!
//! Cmd-N triggers `menu_macos::NewWindow` action, handled via
//! `cx.on_action` in the GPUI main thread — fully wired, no deferral.

use anyhow::Result;
use dat0_i18n::t;
use gpui::{
    App, Application, Bounds, Context, ExternalPaths, IntoElement, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::Root;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_lock::{AppLock, OpenWindowMessage};
use crate::file_drop::{DropOutcome, handle_drop};
use crate::grid::GridDataSource;
use crate::session::Session;

/// Spawn a new workspace window.
///
/// Creates a fresh [`Session`] under `state_root`, wraps it in a
/// `WorkspaceShell`, and opens a GPUI window. Called both from Cmd-N
/// (synchronous main thread) and — once PD-010 is resolved — from the
/// UDS message handler.
fn spawn_window(cx: &mut App, state_root: &Path) {
    // Build tokio runtime handle; we are on the GPUI main thread, but
    // `Session::new` is async so we need to block on it.
    let rt = tokio::runtime::Handle::try_current();
    let session = match rt {
        Ok(handle) => handle.block_on(Session::new(state_root, 1024 * 1024 * 1024)),
        Err(_) => {
            tracing::warn!("spawn_window: no tokio runtime on calling thread — skipping");
            return;
        }
    };
    let session = match session {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            tracing::error!(error = %e, "spawn_window: Session::new failed");
            return;
        }
    };

    let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(t("app.name").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let view = cx.new(|_| WorkspaceShell::new(Arc::clone(&session)));
            cx.new(|cx| Root::new(view, window, cx))
        },
    )
    .expect("open window");
}

/// Launch the dat0 desktop application.
///
/// Blocks the calling thread on the platform event loop until the user
/// closes the last window (the standard GPUI shutdown path).
///
/// # Single-instance enforcement (T12)
///
/// `lock` is the `AppLock` acquired in `main`. After the first window is
/// open, a tokio task is spawned to run `lock.serve(handler)`, which listens
/// on `dat0.sock` for `OpenWindowMessage`s from subsequent launches.
///
/// # UDS → GPUI bridge (PD-010)
///
/// Opening a GPUI window from a tokio task requires calling
/// `AsyncApp::update`, which internally borrows a `RefCell<AppState>`. That
/// borrow is not safe to acquire from a non-main thread while the Cocoa
/// event loop may hold it. For now the UDS handler only logs the received
/// message; the visual cross-thread bridge is deferred to PD-010 (target
/// T17 / P3b). Single-instance enforcement (second launch forwards + exits)
/// is fully functional. Cmd-N spawns new windows synchronously.
///
/// # initial_paths
///
/// If non-empty on cold start, `handle_drop` is called against the first
/// window's session so CLI-supplied files are registered immediately.
pub fn run_app(lock: AppLock, initial_paths: Vec<PathBuf>) -> Result<()> {
    // Build a dedicated tokio runtime for session construction and future
    // async work (file registration, paged queries). main() is synchronous,
    // so Handle::current() would panic — we must create our own runtime here.
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

    let state_root = crate::platform::data_dir().expect("data dir");
    let budget = 1024 * 1024 * 1024; // 1 GB
    let session = runtime
        .block_on(Session::new(&state_root, budget))
        .expect("session");
    let session = Arc::new(Mutex::new(session));

    // Spawn UDS server on the tokio runtime. The handler closure logs each
    // received OpenWindowMessage. Visual window-open from the tokio context
    // is deferred — see module-level doc (PD-010).
    runtime.spawn(async move {
        let result = lock
            .serve(|msg: OpenWindowMessage| {
                tracing::info!(
                    paths = ?msg.paths,
                    "UDS: received open-window request from second instance \
                     (visual window spawn deferred — PD-010)"
                );
            })
            .await;
        if let Err(e) = result {
            tracing::error!(error = %e, "UDS server exited with error");
        }
    });

    let state_root_for_action = state_root.clone();
    Application::new().run(move |cx: &mut App| {
        // Required before opening any window: initialises the gpui-component
        // theme, global state, and (in debug builds) the inspector. Without
        // this, dialogs/sheets/notifications wired up in later tasks (T17)
        // will fail silently.
        gpui_component::init(cx);

        // Install the macOS native menu bar (P1.T14). On non-macOS targets
        // `build_menus` returns an empty Vec, so this is a no-op.
        // Per gpui v0.2.2 (`docs/internal/gpui-api-notes.md` §0.3),
        // `App::set_menus` is invoked inside the `Application::run` closure;
        // there is no `cx.activate_menu(...)` API. `set_menus` borrows `cx`
        // immutably while `build_menus` takes `&mut App`, so the call is
        // split into two statements to satisfy the borrow checker.
        #[cfg(target_os = "macos")]
        {
            let menus = crate::menu_macos::build_menus(cx);
            cx.set_menus(menus);
        }

        // Wire Cmd-N → NewWindow action (macOS only; Linux Cmd-N is P3b).
        // `cx.on_action` registers a global handler called on the GPUI main
        // thread whenever the action fires (keyboard shortcut or menu item).
        // `spawn_window` is synchronous and safe to call from the main thread.
        #[cfg(target_os = "macos")]
        {
            let state_root_for_new_window = state_root_for_action.clone();
            cx.on_action(
                move |_action: &crate::menu_macos::NewWindow, cx: &mut App| {
                    tracing::info!("Cmd-N: spawning new window");
                    spawn_window(cx, &state_root_for_new_window);
                },
            );
        }

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let session_for_window = Arc::clone(&session);
        let first_window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(t("app.name").into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|_| WorkspaceShell::new(Arc::clone(&session_for_window)));
                    // Per gpui-component v0.5.1, the window's first layer MUST be
                    // a Root: it provides the overlay layer used by Dialog,
                    // Sheet, notifications, etc.
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("open window");

        // If CLI paths were supplied on cold start, register them against the
        // first window's session. The WorkspaceShell owns the session, so we
        // spawn a task bound to that window entity.
        if !initial_paths.is_empty() {
            let paths = initial_paths.clone();
            let session_for_drop = Arc::clone(&session);
            let _ = first_window.update(cx, |_root, window, cx| {
                cx.spawn(async move |_weak: gpui::WeakEntity<Root>, _async_cx| {
                    let outcomes = handle_drop(paths, session_for_drop).await;
                    let n_registered = outcomes
                        .iter()
                        .filter(|o| matches!(o, DropOutcome::Registered { .. }))
                        .count();
                    tracing::info!(n_registered, "CLI paths processed on cold start");
                })
                .detach();
                let _ = window;
            });
        }

        // Bring the application to the foreground so the new window isn't
        // hidden behind whatever was focused at launch time (macOS).
        cx.activate(true);
    });

    // runtime drops here, after the event loop exits (last window closed).
    // AppLock is dropped inside the tokio task when `serve` returns (which
    // happens when the runtime is dropped), releasing the PID flock and
    // cleaning up dat0.sock + dat0.pid.
    drop(runtime);
    Ok(())
}

/// Session-backed workspace shell rendered inside `gpui_component::Root`.
///
/// Owns the session for this window and an optional data source (set once the
/// user drops a file or opens a table). When no data source is present,
/// renders a "Drop a file here" placeholder. When a data source is present,
/// renders a grid placeholder pending T11+ TableDelegate wiring.
pub struct WorkspaceShell {
    session: Arc<Mutex<Session>>,
    data_source: Option<Arc<GridDataSource>>,
}

impl WorkspaceShell {
    pub fn new(session: Arc<Mutex<Session>>) -> Self {
        Self {
            session,
            data_source: None,
        }
    }

    pub fn set_data_source(&mut self, ds: Arc<GridDataSource>) {
        self.data_source = Some(ds);
    }
}

impl Render for WorkspaceShell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = Arc::clone(&self.session);

        let drop_listener = cx.listener(move |_view, paths: &ExternalPaths, _window, cx| {
            let paths_vec: Vec<std::path::PathBuf> = paths.paths().to_vec();
            let session = Arc::clone(&session);
            cx.spawn(
                async move |weak_shell: gpui::WeakEntity<WorkspaceShell>, async_cx| {
                    let outcomes = handle_drop(paths_vec, session).await;

                    // Per spec §3.5: the last successfully-registered file
                    // becomes the active tab. Iterate all outcomes and keep
                    // the last Registered one.
                    let last_registered = outcomes.into_iter().rev().find_map(|o| match o {
                        DropOutcome::Registered { table_name, .. } => Some(table_name),
                        _ => None,
                    });

                    if let Some(table_name) = last_registered {
                        // Obtain the engine Arc from the session via a sync update call.
                        let engine = async_cx
                            .update(|app_cx| {
                                weak_shell
                                    .update(app_cx, |view, _cx| view.session.lock().engine.clone())
                                    .ok()
                            })
                            .ok()
                            .flatten();

                        if let Some(engine) = engine {
                            match GridDataSource::new(engine, table_name).await {
                                Ok(ds) => {
                                    let _ = async_cx.update(|app_cx| {
                                        let _ = weak_shell.update(app_cx, |view, cx| {
                                            view.set_data_source(Arc::new(ds));
                                            cx.notify();
                                        });
                                    });
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "WorkspaceShell: GridDataSource::new failed: {e}"
                                    );
                                }
                            }
                        }
                    }
                },
            )
            .detach();
        });

        match self.data_source.as_ref() {
            Some(_ds) => {
                // TODO(T11+): mount gpui-component Table widget with
                // a TableDelegate impl over _ds (paged Arrow batch adapter).
                div()
                    .size_full()
                    .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
                    .on_drop::<ExternalPaths>(drop_listener)
                    .child("Grid placeholder — wired in T11+")
            }
            None => div()
                .size_full()
                .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
                .on_drop::<ExternalPaths>(drop_listener)
                .child("Drop a file here"),
        }
    }
}
