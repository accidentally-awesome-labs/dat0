//! GPUI window bootstrap for the dat0 desktop application.
//!
//! Composes the canonical `gpui` `Application::new().run(...)` entry point
//! (per `crates/gpui/examples/hello_world.rs` at the pinned 0.2.2 publish
//! commit) with the `gpui-component` requirements documented in
//! `docs/internal/gpui-api-notes.md` §0.2 (T0 spike): every window's first
//! layer must be a `gpui_component::Root`, and `gpui_component::init` must
//! run once before any window opens, otherwise dialogs / sheets /
//! notifications silently fail to render later (T17 depends on this).

use anyhow::Result;
use dat0_i18n::t;
use gpui::{
    App, Application, Bounds, Context, ExternalPaths, IntoElement, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::Root;
use parking_lot::Mutex;
use std::sync::Arc;

use crate::file_drop::{DropOutcome, handle_drop};
use crate::grid::GridDataSource;
use crate::session::Session;

/// Launch the dat0 desktop application.
///
/// Blocks the calling thread on the platform event loop until the user
/// closes the last window (the standard GPUI shutdown path).
///
/// A dedicated `tokio::runtime::Runtime` is created here because `main` is
/// synchronous. `run_app` is called before any async runtime is active, so
/// `Handle::current()` would panic. The runtime is kept alive for the full
/// duration of the app by holding it in a local binding; it is dropped only
/// after `Application::run` returns (i.e. after the last window closes).
///
/// Currently panics via `.expect("open window")` if the platform refuses
/// to open a window — treated as a fatal startup error in P1. Graceful
/// handling (propagating through the `Result` return) lands at T17/T21.
pub fn run_app() -> Result<()> {
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

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let session_for_window = Arc::clone(&session);
        cx.open_window(
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

        // Bring the application to the foreground so the new window isn't
        // hidden behind whatever was focused at launch time (macOS).
        cx.activate(true);
    });

    // runtime drops here, after the event loop exits (last window closed).
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
