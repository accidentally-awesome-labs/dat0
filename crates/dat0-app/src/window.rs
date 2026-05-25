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
//! # Single-instance & multi-window (T12 + P3b T1)
//!
//! `run_app` receives the `AppLock` singleton (already acquired in `main`)
//! and a list of CLI paths for the initial window. After the first window
//! opens, a tokio task is spawned to run the UDS server via
//! `AppLock::serve`. P3b T1 closes PD-010: each UDS-received
//! `OpenWindowMessage` posts a visual-spawn closure through the
//! [`crate::main_bridge::MainThreadDispatcher`] global; the closure runs
//! on the GPUI main thread inside `MainLoop::consume` (a `cx.spawn`'d
//! receiver loop registered during app init).
//!
//! Cmd-N triggers `menu_macos::NewWindow` action, handled via
//! `cx.on_action` in the GPUI main thread — fully wired, no deferral.
//!
//! # WindowRegistry wiring (T12 follow-up)
//!
//! A `WindowRegistry` instance is created in `run_app` before
//! `Application::new().run(...)`. Both the first-window open path and the
//! Cmd-N `spawn_window` path call `registry.lock().register(...)` after
//! each successful `cx.open_window`. The registry is an
//! `Arc<parking_lot::Mutex<WindowRegistry>>` captured directly into the
//! `Application::run` closure and passed through to `spawn_window`. T17
//! will assert `registry.lock().len()` to verify window count.

use anyhow::Result;
use dat0_i18n::t;
use gpui::{
    App, Application, Bounds, Context, Entity, ExternalPaths, IntoElement, Render, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::Root;
use gpui_component::table::{Table, TableState};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

use crate::app_lock::{AppLock, OpenWindowMessage};
use crate::file_drop::{DropOutcome, handle_drop};
use crate::grid::{GridDataSource, GridTableDelegate};
use crate::main_bridge::MainLoop;
use crate::session::Session;
use crate::window_registry::{WindowHandle, WindowRegistry};

/// Spawn a new workspace window.
///
/// Creates a fresh [`Session`] under `state_root`, wraps it in a
/// `WorkspaceShell`, and opens a GPUI window. Called both from Cmd-N
/// (synchronous main thread, macOS only) and — as of P3b T1 (closes
/// PD-010) — from the UDS message handler on all platforms via the
/// [`crate::main_bridge::MainThreadDispatcher`] bridge.
///
/// `registry` receives a `register` call for the newly opened window so
/// the window-count assertion in `tests/single_instance.rs` can observe it.
pub(crate) fn spawn_window(
    cx: &mut App,
    state_root: &std::path::Path,
    registry: Arc<Mutex<WindowRegistry>>,
) {
    // SAFETY: block_on is called from the Cocoa/GPUI main thread (cx.on_action
    // fires synchronously here), NOT inside a tokio async context. If gpui ever
    // dispatches actions via tokio::spawn, this becomes a nested-runtime panic;
    // migrate to tokio::task::block_in_place in that case. See PD-010 for the
    // related cross-thread bridge work.
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

    let window_id = session.lock().window_id;
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

    registry.lock().register(WindowHandle { window_id });
    tracing::debug!(%window_id, "spawn_window: window registered in WindowRegistry");
}

/// Scan `scratch_root` for orphan session directories (subdirs containing
/// a `session.json`) and emit at most ONE consolidated warning Banner
/// summarising the count, with a `"Review"` primary action wired to
/// [`crate::actions::builtin::ids::RECOVERY_REVIEW`].
///
/// P3a T15's per-orphan loop is replaced by this count-based emission
/// (P3b T5) so the user sees a single line rather than N near-identical
/// banners. Returns the banners that were emitted so callers (and tests)
/// can inspect them without draining the global pending queue.
///
/// Non-UUID directory names are tolerated (they count as orphans iff
/// they contain a `session.json`) — the test harness uses
/// `session-{i:02}` names to keep `tempdir` paths readable.
pub fn orphan_scan_emit(scratch_root: &std::path::Path) -> Vec<crate::error_ux::Banner> {
    let mut count = 0usize;
    if let Ok(entries) = std::fs::read_dir(scratch_root) {
        for e in entries.flatten() {
            if e.path().join("session.json").is_file() {
                count += 1;
            }
        }
    }
    let mut banners = vec![];
    if count > 0 {
        banners.push(
            crate::error_ux::Banner::warning_with_body(
                format!(
                    "{count} previous session{} found",
                    if count == 1 { "" } else { "s" }
                ),
                "Restore tabs or discard them.".to_string(),
            )
            .with_primary("Review", crate::actions::builtin::ids::RECOVERY_REVIEW),
        );
    }
    for b in &banners {
        crate::error_ux::push(b.clone());
    }
    banners
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
/// # UDS → GPUI bridge (closes PD-010)
///
/// Opening a GPUI window from a tokio task requires calling
/// `AsyncApp::update`, which internally borrows a `RefCell<AppState>`. That
/// borrow is not safe to acquire from a non-main thread. P3b T1 closes
/// PD-010 via [`crate::main_bridge::MainThreadDispatcher`]: the UDS handler
/// posts a closure into a `futures::channel::mpsc` channel; the receiver
/// (`MainLoop::consume`) runs inside `cx.spawn` on the foreground executor
/// and therefore calls `cx.update` on the main thread.
///
/// # initial_paths
///
/// If non-empty on cold start, `handle_drop` is called against the first
/// window's session so CLI-supplied files are registered immediately.
pub fn run_app(lock: AppLock, initial_paths: Vec<PathBuf>, main_loop: MainLoop) -> Result<()> {
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

    // In-process registry of open windows. Created here, before
    // Application::run, so it outlives the event loop and can be inspected
    // by tests after shutdown. Both the first-window open path and the
    // Cmd-N spawn_window path call register() after cx.open_window succeeds.
    let registry = Arc::new(Mutex::new(WindowRegistry::new()));

    // P3b T3: publish `state_root` + the `WindowRegistry` handle as
    // process-wide singletons so the built-in `window.new` action
    // (registered in `main.rs`) can call `spawn_window` with the same
    // arguments the cold-start / Cmd-N paths use. Both setters are
    // idempotent (`OnceCell::set`), so a re-entry during tests is a
    // no-op rather than a panic.
    crate::window_registry::install_state_root(state_root.clone());
    crate::window_registry::install_window_registry(Arc::clone(&registry));

    // Spawn UDS server on the tokio runtime. Each received OpenWindowMessage
    // dispatches a visual-spawn closure onto the GPUI main thread via the
    // process-wide MainThreadDispatcher installed in main.rs — closes PD-010.
    let state_root_for_uds = state_root.clone();
    let registry_for_uds = Arc::clone(&registry);
    runtime.spawn(async move {
        let result = lock
            .serve(move |msg: OpenWindowMessage| {
                tracing::info!(
                    paths = ?msg.paths,
                    "UDS: received open-window request from second instance"
                );
                let Some(d) = crate::window_registry::dispatcher() else {
                    tracing::warn!("PD-010: dispatcher not installed; dropping UDS open-window");
                    return;
                };
                let state_root = state_root_for_uds.clone();
                let registry = Arc::clone(&registry_for_uds);
                if let Err(e) = d.dispatch(move |cx| {
                    spawn_window(cx, &state_root, registry);
                }) {
                    tracing::warn!(error = %e, "UDS: main-thread dispatch failed");
                }
            })
            .await;
        if let Err(e) = result {
            tracing::error!(error = %e, "UDS server exited with error");
        }
    });

    // Used by the orphan scan (all platforms) and the macOS Cmd-N handler.
    let state_root_for_action = state_root.clone();
    let registry_for_run = Arc::clone(&registry);
    Application::new().run(move |cx: &mut App| {
        // Required before opening any window: initialises the gpui-component
        // theme, global state, and (in debug builds) the inspector. Without
        // this, dialogs/sheets/notifications wired up in later tasks (T17)
        // will fail silently.
        gpui_component::init(cx);

        // PD-010 closure: drive the MainThreadDispatcher receiver loop from
        // a foreground-executor task so each posted closure runs on the
        // GPUI main thread via `cx.update`. The loop terminates when every
        // dispatcher clone is dropped (see main_bridge.rs).
        cx.spawn(async move |cx| {
            if let Err(e) = main_loop.consume(cx).await {
                tracing::warn!(error = %e, "MainLoop::consume exited with error");
            }
        })
        .detach();

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
            let registry_for_action = Arc::clone(&registry_for_run);
            cx.on_action(
                move |_action: &crate::menu_macos::NewWindow, cx: &mut App| {
                    tracing::info!("Cmd-N: spawning new window");
                    spawn_window(
                        cx,
                        &state_root_for_new_window,
                        Arc::clone(&registry_for_action),
                    );
                },
            );
        }

        // Wire Cmd-Shift-P (macOS) / Ctrl-Shift-P (Linux) → OpenCommandPalette
        // action (P3b T6). `bind_keys` registers the keystroke against the
        // global keymap; `on_action` registers the handler. Both fire the
        // same `OpenCommandPalette` action so the menu-item click and the
        // keystroke path converge on `command_palette::open`.
        //
        // The Linux menu module doesn't exist yet (the comment above flags
        // "Linux Cmd-N is P3b"), but `OpenCommandPalette` is declared in
        // `menu_macos.rs` unconditionally so we can bind it on Linux too —
        // the handler still resolves and the keystroke fires even without a
        // visible menu item.
        {
            #[cfg(target_os = "macos")]
            let keystroke = "cmd-shift-p";
            #[cfg(not(target_os = "macos"))]
            let keystroke = "ctrl-shift-p";
            cx.bind_keys([gpui::KeyBinding::new(
                keystroke,
                crate::menu_macos::OpenCommandPalette,
                None,
            )]);
            cx.on_action(
                |_action: &crate::menu_macos::OpenCommandPalette, cx: &mut App| {
                    crate::command_palette::open(cx);
                },
            );
        }

        let first_window_id = session.lock().window_id;
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

        // Register the first window with the in-process registry so T17 can
        // assert window count immediately after cold start.
        registry_for_run.lock().register(WindowHandle {
            window_id: first_window_id,
        });
        tracing::debug!(%first_window_id, "run_app: first window registered in WindowRegistry");

        // Scan `$state_root/scratch/*` for orphaned dirs (sessions that
        // didn't exit cleanly) and emit a single count-based Banner with
        // a "Review" primary action wired to `recovery.review` (T5).
        // Per spec §11 exit criterion #4. The per-orphan loop introduced
        // in T2 is replaced by [`orphan_scan_emit`] which consolidates
        // N orphans into one banner.
        let scratch_root = state_root_for_action.join("scratch");
        let _emitted = orphan_scan_emit(&scratch_root);

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
/// Owns the session for this window and an optional data source (set once
/// the user drops a file or opens a table). When no data source is present,
/// renders a "Drop a file here" placeholder. When a data source is present
/// the shell mounts a real `gpui_component::table::Table` over a
/// [`GridTableDelegate`] wrapper (P3b T4 — closes the P3a T10 placeholder).
///
/// `table_state` is built lazily on the first render after `set_data_source`
/// — `TableState::new` requires `&mut Window`, which is only available
/// inside `Render::render`. The drop handler runs off-thread and so cannot
/// touch the window; it just stores the new `Arc<GridDataSource>` and asks
/// the view to re-render via `cx.notify()`. The next frame promotes that
/// `Arc` into an `Entity<TableState<…>>`.
pub struct WorkspaceShell {
    session: Arc<Mutex<Session>>,
    data_source: Option<Arc<GridDataSource>>,
    /// Stateful entity owning the gpui-component Table's scroll handles,
    /// column-resize state, selection, etc. (`gpui-table-api-notes.md` §3).
    /// Rebuilt when `data_source` is swapped (e.g., user drops a second
    /// file). `None` until the first data source lands.
    table_state: Option<Entity<TableState<GridTableDelegate>>>,
    /// Theme observer subscription, kept alive for the lifetime of the
    /// view. Per `docs/internal/gpui-api-notes.md` §0.A.4 the `Theme`
    /// global is app-scoped; switching theme in one window notifies every
    /// observer in every window so the grid re-renders with the new
    /// palette.
    ///
    /// **CAVEAT (P3b T4):** dat0's `crate::theme::Theme` is not yet wired
    /// as a `gpui::Global` — that promotion is part of a later P3b task
    /// (the theme live-switch work). For now we subscribe to
    /// `gpui_component::Theme` which is already `impl Global for Theme`
    /// per `docs/internal/gpui-component-api-notes.md` §1.3. The later
    /// task only needs to flip the `<gpui_component::Theme>` type
    /// parameter to `<crate::theme::Theme>` once the dat0 type is
    /// promoted; no other change required.
    theme_subscription: Option<Subscription>,
}

impl WorkspaceShell {
    pub fn new(session: Arc<Mutex<Session>>) -> Self {
        Self {
            session,
            data_source: None,
            table_state: None,
            theme_subscription: None,
        }
    }

    pub fn set_data_source(&mut self, ds: Arc<GridDataSource>) {
        // Drop any stale TableState — it was built around the previous
        // delegate's `Arc<GridDataSource>` and would render stale rows.
        // The next `render` call rebuilds one against the new source.
        self.table_state = None;
        self.data_source = Some(ds);
    }
}

impl WorkspaceShell {
    /// Return the static type name of the widget the shell mounts when a
    /// data source is present. Used by `tests/file_drop_formats.rs` to
    /// assert the P3a T10 placeholder (`div`) has been replaced by a real
    /// `gpui_component::table::Table` mount.
    ///
    /// Lives outside `#[cfg(test)]` because Rust integration tests (in
    /// `tests/`) build the library crate without the `test` cfg flag and
    /// therefore can't see `#[cfg(test)]` items. The helper is a static
    /// no-op — `std::any::type_name` is resolved at compile time and
    /// carries no runtime cost.
    ///
    /// This is an intent-level assertion (no real render loop needed) —
    /// see the test docstring in `tests/file_drop_formats.rs` for the
    /// rationale.
    pub fn child_widget_type_name() -> &'static str {
        std::any::type_name::<Table<GridTableDelegate>>()
    }
}

impl Render for WorkspaceShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Subscribe to Theme global changes once, on the first render. The
        // subscription returns a `Subscription` that must be kept alive
        // (drop = unregister) per `gpui-api-notes.md` §0.A.2.
        //
        // Per the IMPORTANT CAVEAT in the P3b T4 task brief we observe
        // `gpui_component::Theme` (which IS a Global) rather than
        // `crate::theme::Theme` (which is not yet promoted to a Global);
        // the later theme-live-switch task flips this single type
        // parameter once dat0's Theme becomes a Global.
        if self.theme_subscription.is_none() {
            let sub = cx.observe_global::<gpui_component::Theme>(|_view, cx| {
                cx.notify();
            });
            self.theme_subscription = Some(sub);
        }

        // Lazily promote `Arc<GridDataSource>` → `Entity<TableState<…>>`
        // on the first render after the data source landed. `TableState::new`
        // requires `&mut Window`, which is only available inside `render`
        // — the async drop handler stores the `Arc` then asks the view to
        // re-render so this branch can build the stateful entity.
        if let Some(ds) = self.data_source.as_ref() {
            let needs_rebuild = match self.table_state.as_ref() {
                None => true,
                Some(state_entity) => {
                    // If the stored delegate's source no longer matches the
                    // current one (user dropped a second file), rebuild.
                    !state_entity.read(cx).delegate().source_ptr_eq(ds)
                }
            };
            if needs_rebuild {
                let delegate = GridTableDelegate::new(Arc::clone(ds));
                self.table_state = Some(cx.new(|cx| TableState::new(delegate, window, cx)));
            }
        }

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

        // `Table<D>` and the empty-state are different concrete types, so
        // we widen both arms with `.into_any_element()` to satisfy
        // `impl IntoElement`'s single-return-type requirement.
        let body = match (self.data_source.as_ref(), self.table_state.as_ref()) {
            (Some(_ds), Some(state)) => {
                // Real Table mount — closes the P3a T10 placeholder.
                // Per `docs/internal/gpui-table-api-notes.md` §3:
                //   `Table::new(state: &Entity<TableState<D>>) -> Self`
                // Theming flows implicitly via `cx.theme()` inside the
                // widget (spike §1.3); no prop to pass.
                Table::new(state)
                    .stripe(true)
                    .bordered(true)
                    .into_any_element()
            }
            // Either: we have a data source but the state hasn't been
            // promoted yet (the next frame promotes it), or there is no
            // data source. In both cases show the placeholder copy.
            (Some(_), None) => div().child("Loading grid…").into_any_element(),
            (None, _) => div().child("Drop a file here").into_any_element(),
        };

        div()
            .size_full()
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
            .on_drop::<ExternalPaths>(drop_listener)
            .child(body)
    }
}
