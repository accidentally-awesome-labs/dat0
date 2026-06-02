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
    App, Application, Bounds, Context, Entity, ExternalPaths, FocusHandle, IntoElement,
    KeyDownEvent, Render, Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};
use gpui_component::Root;
use gpui_component::h_flex;
use gpui_component::table::{Table, TableState};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

use crate::app_lock::{AppLock, OpenWindowMessage};
use crate::empty_state::EmptyState;
use crate::file_drop::{DropOutcome, handle_drop};
use crate::grid::{GridDataSource, GridTableDelegate};
use crate::main_bridge::MainLoop;
use crate::recents::Recents;
use crate::session::Session;
use crate::view::ViewModel;
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
            let view = cx.new(|cx| WorkspaceShell::new(Arc::clone(&session), cx));
            // T13: register this workspace as the focused one so that
            // view.undo / view.redo dispatch closures can reach it.
            crate::window_registry::install_focused_workspace(view.downgrade().into());
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
    // Enter the tokio runtime for the lifetime of the GPUI event loop. GPUI's
    // foreground executor (`cx.spawn`) runs its tasks on THIS main thread, and
    // the app's async engine work invoked from GPUI handlers uses tokio
    // primitives — `handle_drop`'s `spawn_blocking` (cold-start CLI file load +
    // drag-drop) and `spawn_view_change` / prefetch's `tokio::spawn`. Without an
    // active runtime context on the main thread those panic ("must be called
    // from the context of a Tokio runtime"). The guard drops before `runtime`
    // (declared earlier), so teardown order is correct.
    let _rt_guard = runtime.enter();
    Application::new().run(move |cx: &mut App| {
        // Required before opening any window: initialises the gpui-component
        // theme, global state, and (in debug builds) the inspector. Without
        // this, dialogs/sheets/notifications wired up in later tasks (T17)
        // will fail silently.
        gpui_component::init(cx);

        // P3b T12 (D-002 closure): promote dat0's own `crate::theme::Theme`
        // to a `gpui::Global` for the lifetime of the app. The initial id
        // is read from `theme.id` in the persisted settings file (the same
        // path AppContext::boot writes), with `"dark"` as the fallback for
        // missing / unknown ids. Subscribers register via
        // `cx.observe_global::<crate::theme::Theme>` (see
        // `WorkspaceShell::render`); the Settings theme dropdown's
        // `on_theme_change` calls `Theme::switch` to fan out to every
        // subscriber on the next tick.
        if let Ok(cfg_dir) = crate::platform::config_dir() {
            let settings_path = cfg_dir.join("settings.toml");
            let store = crate::settings::store::SettingsStore::with_path(settings_path);
            crate::theme::Theme::install(cx, &store);
        } else {
            // Without a config dir we still want subscribers to find a
            // global (`cx.global::<Theme>` panics otherwise). Install the
            // built-in default directly — same shape as the fallback path
            // in `Theme::install`.
            cx.set_global(crate::theme::Theme::load_builtin_or_default("dark"));
        }

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

        // Wire Cmd-Z / Ctrl-Z → Undo, Cmd-Shift-Z / Ctrl-Shift-Z → Redo (P4a T7).
        // `Undo` and `Redo` are gpui action stubs declared in `menu_macos.rs`
        // (unconditional, so they resolve on Linux too). Handlers dispatch
        // through the ActionRegistry so the same closure drives menu-click,
        // keybind, and command-palette paths.
        {
            #[cfg(target_os = "macos")]
            let (undo_ks, redo_ks) = ("cmd-z", "cmd-shift-z");
            #[cfg(not(target_os = "macos"))]
            let (undo_ks, redo_ks) = ("ctrl-z", "ctrl-shift-z");
            cx.bind_keys([
                gpui::KeyBinding::new(undo_ks, crate::menu_macos::Undo, None),
                gpui::KeyBinding::new(redo_ks, crate::menu_macos::Redo, None),
            ]);
            cx.on_action(|_action: &crate::menu_macos::Undo, cx: &mut App| {
                if let Some(reg) = crate::window_registry::action_registry() {
                    if let Some(desc) = reg.get(&crate::actions::ActionId::from(
                        crate::actions::builtin::ids::VIEW_UNDO,
                    )) {
                        (desc.dispatch)(cx);
                    }
                }
            });
            cx.on_action(|_action: &crate::menu_macos::Redo, cx: &mut App| {
                if let Some(reg) = crate::window_registry::action_registry() {
                    if let Some(desc) = reg.get(&crate::actions::ActionId::from(
                        crate::actions::builtin::ids::VIEW_REDO,
                    )) {
                        (desc.dispatch)(cx);
                    }
                }
            });
        }

        // Wire Cmd-E / Ctrl-E → Export (P4c T11). `Export` is a gpui action stub
        // declared in `menu_macos.rs` (unconditional, so it resolves on Linux
        // too). The handler dispatches through the ActionRegistry so the
        // menu-click, keybind, and command-palette paths converge on
        // `view.export` → `WorkspaceShell::open_export_dialog`.
        {
            #[cfg(target_os = "macos")]
            let export_ks = "cmd-e";
            #[cfg(not(target_os = "macos"))]
            let export_ks = "ctrl-e";
            cx.bind_keys([gpui::KeyBinding::new(
                export_ks,
                crate::menu_macos::Export,
                None,
            )]);
            cx.on_action(|_action: &crate::menu_macos::Export, cx: &mut App| {
                if let Some(reg) = crate::window_registry::action_registry() {
                    if let Some(desc) = reg.get(&crate::actions::ActionId::from(
                        crate::actions::builtin::ids::VIEW_EXPORT,
                    )) {
                        (desc.dispatch)(cx);
                    }
                }
            });
        }

        // Register the SQL grammar for the P5 console editor (runtime-registered,
        // single grammar — see query::highlight). T0 spike confirmed the runtime
        // path; decision-7 fallback NOT triggered.
        crate::query::highlight::register_sql_language();

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
                    let view =
                        cx.new(|cx| WorkspaceShell::new(Arc::clone(&session_for_window), cx));
                    // T13: register this workspace as the focused one so that
                    // view.undo / view.redo dispatch closures can reach it.
                    crate::window_registry::install_focused_workspace(view.downgrade().into());
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
    pub(crate) data_source: Option<Arc<GridDataSource>>,
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
    /// As of P3b T12 (D-002 closure) we subscribe to
    /// `crate::theme::Theme` — dat0's own theme type was promoted to a
    /// `gpui::Global` in `crates/dat0-app/src/theme/mod.rs`, replacing
    /// the T4 placeholder subscription against `gpui_component::Theme`.
    theme_subscription: Option<Subscription>,
    /// Per-tab view model (T13). Owns the active Transformation stack,
    /// undo cursor, and view name. Initialized when a table is first
    /// registered (file drop). `None` until the first table lands.
    ///
    /// T13 note: P4a is single-tab per window; multi-tab (one ViewModel
    /// per tab) is P4b. The field is `Option` so it can be None before
    /// any file is dropped.
    pub(crate) view_model: Option<ViewModel>,
    /// Currently-mounted filter popover (T0 / PD-016 funnel-click wiring).
    /// `Some` while a popover is open for some column; cleared when its
    /// `Outcome` is routed (apply / clear / cancel). Rendered as an overlay
    /// child in `render` when present.
    pub(crate) active_popover:
        Option<Entity<crate::view::filter_popover_entity::FilterPopoverEntity>>,
    /// Subscription to the active popover's `FilterPopoverEvent`. Stored so
    /// the callback stays registered — a dropped `Subscription` deregisters
    /// silently (P4a T10b post-review lesson). Cleared alongside
    /// `active_popover`.
    popover_sub: Option<Subscription>,
    /// Ephemeral grid selection (T4 pure-logic model). `None` until a data
    /// source is mounted; `SelectionModel::new` requires non-empty grid
    /// dimensions, so it is constructed lazily on the first render after a
    /// source lands (see `render`). T11 wires keyboard movers to it; T6 reads
    /// `selection.active()` to locate the cell being edited.
    pub(crate) selection: Option<crate::grid::selection::SelectionModel>,
    /// Currently-mounted inline cell editor (T6). `Some` while editing the
    /// active cell; cleared on commit / cancel. Rendered as an overlay child
    /// in `render` when present.
    pub(crate) cell_editor: Option<Entity<crate::grid::cell_editor::CellEditor>>,
    /// Subscription to the active cell editor's `CellEditorEvent`. Stored so
    /// the commit/cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside
    /// `cell_editor`.
    pub(crate) cell_editor_sub: Option<Subscription>,
    /// Marching-ants range set by the most recent copy/cut (T7). Stored in
    /// screen-space; T7 only records the range. T11/polish will render the
    /// animated dashed border and clear this on the next selection change —
    /// until then it persists after a copy/cut.
    // T11/polish: render marching-ants from this stored range + clear on selection change.
    pub(crate) copied_range: Option<crate::grid::selection::CellRange>,
    /// Currently-mounted inline header-rename editor (P4c T7). `Some` while the
    /// user is renaming a column; cleared on commit / cancel. The `usize` is the
    /// screen column index. Rendered in-place inside `render_th` when `Some` for
    /// that column.
    pub(crate) header_rename: Option<(usize, Entity<crate::grid::cell_editor::HeaderRenameEditor>)>,
    /// Subscription to the active header-rename editor's [`HeaderRenameEvent`].
    /// Stored so the commit/cancel callback stays registered — a dropped
    /// `Subscription` deregisters silently (the P4a T10b trap). Cleared
    /// alongside `header_rename`.
    pub(crate) header_rename_sub: Option<Subscription>,
    /// Folded visible columns (source→display, display order, deletes excluded),
    /// recomputed from the active stack whenever it changes (P4c T5). Drives the
    /// header labels + order and the screen-col→source addressing used by every
    /// mutating path. Empty until a data source binds; with no projection ops
    /// active it is the identity over `ds.visible_column_names()`, so screen-col
    /// index == schema index and existing behaviour is unchanged.
    pub(crate) column_view: Vec<dat0_engine::transform::ProjectionColumn>,
    /// GPUI focus handle for the workspace shell (T11). The outer container
    /// element tracks this handle so that `on_key_down` receives key events
    /// when the workspace has focus.  Constructed once in `new`; the element
    /// receives focus on the first click or programmatic request.
    ///
    /// PD-018 note: the grid render-cache work (PD-018) may later gate
    /// fine-grained cell focus; this shell-level handle is sufficient for
    /// T11's keyboard map + selection navigation.
    focus_handle: FocusHandle,
    /// PipelineBar expanded/collapsed toggle state (P4c T9). The expanded
    /// timeline view is T10 — this stub stores the toggle flag so the `⌄`
    /// button can flip it and be rendered correctly on the next frame.
    pub(crate) pipeline_bar_state: crate::view::pipeline_bar::PipelineBarState,
    /// Currently-mounted Export… dialog (P4c T11). `Some` while the File →
    /// Export… dialog is open; cleared when its `ExportEvent` is routed
    /// (Export → run + dismiss, or Cancel → dismiss). Rendered as an overlay
    /// child in `render` when present.
    export_dialog: Option<Entity<crate::view::export_dialog::ExportDialog>>,
    /// Subscription to the active export dialog's [`ExportEvent`]. Stored so the
    /// Export/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside
    /// `export_dialog`.
    ///
    /// [`ExportEvent`]: crate::view::export_dialog::ExportEvent
    export_dialog_sub: Option<Subscription>,
    /// SQL Console panel (P5a T5). Lazily constructed on the first
    /// `toggle_sql_console` call (which has the `&mut Window` that the per-tab
    /// code editors need). `None` until first toggled; visibility is gated by
    /// `sql_console_visible` so a second toggle hides without tearing it down.
    pub(crate) sql_console: Option<Entity<crate::view::sql_console::SqlConsole>>,
    /// Subscription to the console's [`SqlConsoleEvent`]. Stored so the
    /// run/cancel/persist callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap).
    ///
    /// Only written (never read) until P5a T11 wires the toggle action; the
    /// field's purpose is to keep the subscription alive for the entity's life.
    ///
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    #[allow(dead_code)] // read indirectly (keep-alive); toggle wired in P5a T11
    pub(crate) sql_console_sub: Option<Subscription>,
    /// Whether the SQL Console panel is currently shown. Toggled by
    /// `toggle_sql_console`; the render gate respects this independently of
    /// whether `sql_console` is `Some`.
    pub(crate) sql_console_visible: bool,
}

impl WorkspaceShell {
    pub fn new(session: Arc<Mutex<Session>>, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            data_source: None,
            table_state: None,
            theme_subscription: None,
            view_model: None,
            active_popover: None,
            popover_sub: None,
            selection: None,
            cell_editor: None,
            cell_editor_sub: None,
            copied_range: None,
            column_view: Vec::new(),
            focus_handle: cx.focus_handle(),
            header_rename: None,
            header_rename_sub: None,
            pipeline_bar_state: crate::view::pipeline_bar::PipelineBarState::default(),
            export_dialog: None,
            export_dialog_sub: None,
            sql_console: None,
            sql_console_sub: None,
            sql_console_visible: false,
        }
    }

    pub fn set_data_source(&mut self, ds: Arc<GridDataSource>) {
        // Drop any stale TableState — it was built around the previous
        // delegate's `Arc<GridDataSource>` and would render stale rows.
        // The next `render` call rebuilds one against the new source.
        self.table_state = None;
        // Clear the selection so it is rebuilt against the new source's
        // dimensions on the next render.  Without this a second file drop
        // would leave SelectionModel with the old row/column counts, and
        // `selection.active().col` could point past the new schema.
        self.selection = None;
        self.data_source = Some(ds);
        // Re-derive the ColumnView from the new source's visible columns + the
        // active stack (P4c T5). On a fresh bind this is the identity over the
        // visible columns (no projection ops yet); after a rebind that carries
        // an active stack (e.g. a filter view) the source columns are unchanged,
        // so the fold is still identity unless a projection op is present.
        self.refresh_column_view();
    }

    /// Install or replace the active `GridDataSource` after a `ViewChange`
    /// round-trip completes (T13). Clears the stale `TableState` so the
    /// next `render` promotes the new source into a fresh `Entity<TableState>`.
    pub fn apply_view_change(&mut self, new_ds: Arc<GridDataSource>, cx: &mut Context<Self>) {
        self.table_state = None;
        // Defensively clear the selection — a view-change is the rebind path
        // and, while P4b preserves the schema, clearing keeps the selection
        // model consistent and prevents stale-dimension bugs if column count
        // ever changes (e.g., a future hide-column transform).
        self.selection = None;
        self.data_source = Some(new_ds);
        // A view-change rebind re-derives the source columns; recompute the
        // ColumnView so the header labels/order and screen-col→source addressing
        // track the (possibly new) active stack (P4c T5).
        self.refresh_column_view();
        cx.notify();
    }

    /// Prefetch the page(s) covering screen rows `[start, end)` into the
    /// `GridDataSource` LRU so the grid's synchronous `render_td` paints real
    /// values for the rows the user can see (PD-018).
    ///
    /// The fetch runs OFF the GPUI main thread — `GridDataSource::page_for` is
    /// async DuckDB I/O and must never block the 60 fps render loop. Once the
    /// page is in the LRU, the re-render `notify` is posted back onto the main
    /// thread via the [`crate::main_bridge::MainThreadDispatcher`] (the canonical
    /// `spawn_view_change` discipline — NEVER `cx.update` from the tokio task).
    /// Re-rendering the shell re-renders the mounted `Table`, whose `render_td`
    /// now finds the cached page.
    ///
    /// Called on grid bind (page 0) and from the delegate's
    /// `visible_rows_changed` hook (scroll-paging).  When both boundary pages
    /// are already resident in the LRU, the spawn is skipped entirely — no
    /// tokio task, no `cx.notify()` — eliminating the gratuitous task/notify
    /// storm on fast scroll over already-loaded data.
    pub fn prefetch_visible_rows(&self, start: usize, end: usize, cx: &mut Context<Self>) {
        let Some(ds) = self.data_source.as_ref() else {
            return;
        };

        // Cheap resident guard: if both boundary pages are already in the LRU
        // cache, the synchronous `render_td` will already paint real values —
        // there is nothing to fetch and no notify to post.  This eliminates the
        // gratuitous task + notify storm when the user scrolls quickly over
        // pages that were prefetched on an earlier tick.
        //
        // The guard does NOT perturb LRU eviction order (`contains` is
        // non-mutating) and is O(1).
        //
        // Prefetch-on-bind path: on first render, page 0 is absent, so
        // `pages_resident` returns false and the spawn proceeds as normal.
        let last = end.saturating_sub(1);
        if ds.pages_resident(start, last) {
            return;
        }

        let ds = Arc::clone(ds);
        let ws_weak = cx.entity().downgrade();

        // Page-align the range to the rows actually requested; `page_for`
        // internally aligns each `row` to its `PAGE_ROWS` boundary, so issuing
        // one fetch per visible row would be wasteful. We sample the start and
        // (inclusive) last row so a visible range that straddles a page boundary
        // loads both pages.
        let start = start as u64;
        let last = last as u64;

        tokio::spawn(async move {
            // Load the page covering the first visible row, then (if different)
            // the page covering the last visible row. `page_for` is idempotent
            // (cache hit on the second call for the same page).
            let mut any_loaded = false;
            for row in [start, last] {
                match ds.page_for(row).await {
                    Ok(_) => any_loaded = true,
                    Err(e) => {
                        tracing::warn!(row, error = %e, "prefetch_visible_rows: page_for failed");
                    }
                }
            }
            if !any_loaded {
                return;
            }
            // Post the re-render onto the GPUI main thread via the dispatcher.
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    if let Some(h) = ws_weak.upgrade() {
                        h.update(app_cx, |_ws, cx| cx.notify());
                    }
                });
            } else {
                tracing::warn!(
                    "prefetch_visible_rows: no MainThreadDispatcher installed; grid will not refresh"
                );
            }
        });
    }

    /// Mutable access to the per-tab `ViewModel` (T13). Returns `None` if
    /// no table has been registered yet (pre-file-drop state).
    pub fn view_model_mut(&mut self) -> Option<&mut ViewModel> {
        self.view_model.as_mut()
    }

    /// The `Arc<DuckDBEngine>` bound to this session (T13 helper).
    pub fn engine(&self) -> Arc<dat0_engine::DuckDBEngine> {
        Arc::clone(&self.session.lock().engine)
    }

    /// The base table name (already-quoted, suitable for ViewModel construction).
    /// Returns `None` if no file has been registered yet.
    pub fn base_table(&self) -> Option<String> {
        self.view_model
            .as_ref()
            .map(|vm| vm.base_table().to_string())
    }

    /// Recompute `column_view` from the base columns (the visible source columns
    /// of the active view) + the active transform stack (P4c T5). Called after
    /// every stack change and after a data-source (re)bind so the view never
    /// goes stale.
    ///
    /// With no projection ops in the stack the fold is the identity over the
    /// visible columns, so `source_for_screen_col(&column_view, i)` returns the
    /// same column `ds.column_name(i)` does — existing behaviour is unchanged.
    pub(crate) fn refresh_column_view(&mut self) {
        let base: Vec<String> = self
            .data_source
            .as_ref()
            .map(|ds| ds.visible_column_names())
            .unwrap_or_default();
        let ops: &[dat0_engine::Transformation] = self
            .view_model
            .as_ref()
            .map(|vm| vm.active())
            .unwrap_or(&[]);
        self.column_view = crate::view::fold_columns(&base, ops);
    }

    /// Resolve a header (screen) column index to its bare SOURCE column name via
    /// the active `ColumnView` (P4c T5). Returns `None` if no column maps to
    /// `col_ix`.
    ///
    /// Screen-col→source is resolved through the folded `column_view` rather
    /// than positionally over the Arrow schema, so after a display-only reorder
    /// or delete a screen index still addresses the right source column. With no
    /// projection ops the view is identity, so this is equivalent to the
    /// previous `ds.column_name(col_ix)`.
    pub(crate) fn column_name(&self, col_ix: usize) -> Option<String> {
        crate::view::column_view::source_for_screen_col(&self.column_view, col_ix)
            .map(str::to_string)
    }

    /// Drive the engine round-trip + grid rebind for a [`ViewChange`] (T6 —
    /// extracted from `on_sort_zone_click` / `route_filter_outcome` so the
    /// `spawn_view_change` + `apply_view_change` boilerplate is written once;
    /// reused by T6/T7/T8 mutation handlers).
    ///
    /// Reads the base-table name from the active `ViewModel` (the round-trip
    /// rebinds to it when `change` clears the stack). No-op if no `ViewModel`
    /// is mounted yet.
    ///
    /// Preserves the dispatcher discipline established by `spawn_view_change`:
    /// the closure runs on the GPUI main thread via the `MainThreadDispatcher`,
    /// never `cx.update` from the tokio task.
    pub(crate) fn spawn_rebind(&mut self, change: crate::view::ViewChange, cx: &mut Context<Self>) {
        // The ViewModel stack has already been mutated by the caller (set_sort /
        // set_filter / edit_cells / delete_rows / a projection op). Refresh the
        // ColumnView so the header labels/order + screen-col→source addressing
        // reflect the new active stack immediately — a display-only change
        // (Rename/Reorder/DeleteColumn, T6+) never round-trips through
        // `apply_view_change`, so this is the only refresh hook for those. For a
        // real data-view change this is harmless (the source columns are
        // unchanged) and `apply_view_change` refreshes again on rebind (P4c T5).
        self.refresh_column_view();
        let Some(base_table) = self.base_table() else {
            return;
        };
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        crate::view::spawn_view_change(
            engine,
            base_table,
            change,
            Arc::new(move |new_ds, app_cx| {
                if let Some(h) = ws_weak.upgrade() {
                    h.update(app_cx, |ws, cx| ws.apply_view_change(new_ds, cx));
                }
            }),
        );
    }

    /// Sort-zone click (T0 / PD-016). Reads the current sort, cycles the
    /// clicked column (plain `click` or `shift_click` extend), writes it back
    /// via [`ViewModel::set_sort`], and drives the engine round-trip exactly
    /// like `dispatch_undo` in `actions/view_actions.rs`.
    pub fn on_sort_zone_click(&mut self, col_ix: usize, shift: bool, cx: &mut Context<Self>) {
        let Some(column) = self.column_name(col_ix) else {
            return;
        };
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let active = vm.current_sort_as_active();
        let active = if shift {
            active.shift_click(&column)
        } else {
            active.click(&column)
        };
        let change = vm.set_sort(active.keys().to_vec());
        self.spawn_rebind(change, cx);
    }

    /// Funnel-zone click (T0 / PD-016). Mounts the filter popover for
    /// `col_ix`, pre-populated from any active filter on that column, and
    /// subscribes to its `FilterPopoverEvent` so the terminal `Outcome` is
    /// routed back into the `ViewModel` + engine round-trip.
    pub fn on_funnel_click(&mut self, col_ix: usize, _window: &mut Window, cx: &mut Context<Self>) {
        use crate::view::filter_popover_entity::{FilterPopoverEntity, FilterPopoverEvent};

        let Some(column) = self.column_name(col_ix) else {
            return;
        };
        let Some(ds) = self.data_source.as_ref() else {
            return;
        };
        // Type the popover off the SOURCE column (resolved via the ColumnView)
        // so a display-only reorder can't hand the funnel the wrong column's
        // operator surface (P4c T5). Identity with no projection ops.
        let column_type = ds
            .column_type_for_source(&column)
            .unwrap_or(crate::view::filter_popover::ColumnType::String);

        // Pre-populate from any active filter on this column (edit-existing flow).
        let pre = self
            .view_model
            .as_ref()
            .and_then(|vm| vm.find_filter_for(&column).cloned());

        let popover = cx.new(|_| match &pre {
            Some(existing) => {
                FilterPopoverEntity::from_existing(column.clone(), column_type, existing)
            }
            None => FilterPopoverEntity::new(column.clone(), column_type),
        });

        // STORE the subscription — callbacks fire silently if the returned
        // Subscription is dropped (P4a T10b post-review lesson).
        let sub = cx.subscribe(
            &popover,
            move |ws: &mut Self, _pop, ev: &FilterPopoverEvent, cx| {
                let FilterPopoverEvent::OutcomeEmitted(outcome) = ev;
                ws.route_filter_outcome(outcome.clone(), cx);
            },
        );
        self.popover_sub = Some(sub);
        self.active_popover = Some(popover);
        cx.notify();
    }

    /// Route a filter-popover [`Outcome`] into the ViewModel + engine
    /// round-trip, then dismiss the popover (T0 / PD-016).
    ///
    /// [`Outcome`]: crate::view::filter_popover_entity::Outcome
    fn route_filter_outcome(
        &mut self,
        outcome: crate::view::filter_popover_entity::Outcome,
        cx: &mut Context<Self>,
    ) {
        // Dismiss the popover regardless of the outcome.
        self.active_popover = None;
        self.popover_sub = None;

        let change = {
            let Some(vm) = self.view_model.as_mut() else {
                cx.notify();
                return;
            };
            // Pure decision lives in `view::route_outcome` (shared with the
            // click_wiring integration test); the engine round-trip below stays
            // in this GPUI handler.
            crate::view::route_outcome(vm, outcome)
        };
        let Some(change) = change else {
            cx.notify();
            return;
        };
        self.spawn_rebind(change, cx);
    }

    // ── Export… dialog + native save panel + streaming COPY (P4c T11) ─────────

    /// Mount the File → Export… dialog (P4c T11).
    ///
    /// Follows the `on_funnel_click` popover pattern: build the entity via
    /// `cx.new`, subscribe to its [`ExportEvent`], and STORE the subscription in
    /// `export_dialog_sub` (a dropped `Subscription` deregisters the callback
    /// silently — the P4a T10b trap). No-op (graceful) when no `ViewModel` is
    /// mounted, so Export… off an empty workspace does nothing rather than
    /// presenting a dialog that can't build a SELECT.
    ///
    /// [`ExportEvent`]: crate::view::export_dialog::ExportEvent
    pub fn open_export_dialog(&mut self, cx: &mut Context<Self>) {
        use crate::view::export_dialog::{ExportDialog, ExportEvent};

        if self.view_model.is_none() {
            tracing::debug!("open_export_dialog: no ViewModel (no file registered yet)");
            return;
        }

        let dialog = cx.new(|_| ExportDialog::new());
        // STORE the subscription — callbacks fire silently if the returned
        // Subscription is dropped (P4a T10b post-review lesson; mirrors
        // `on_funnel_click`'s `popover_sub`).
        let sub = cx.subscribe(&dialog, |ws: &mut Self, _dialog, ev: &ExportEvent, cx| {
            ws.route_export_event(ev.clone(), cx);
        });
        self.export_dialog_sub = Some(sub);
        self.export_dialog = Some(dialog);
        cx.notify();
    }

    /// Route an [`ExportEvent`] from the dialog: `Export` runs the save panel +
    /// COPY (and dismisses); `Cancel` just dismisses.
    ///
    /// [`ExportEvent`]: crate::view::export_dialog::ExportEvent
    fn route_export_event(
        &mut self,
        ev: crate::view::export_dialog::ExportEvent,
        cx: &mut Context<Self>,
    ) {
        use crate::view::export_dialog::ExportEvent;
        match ev {
            ExportEvent::Export { scope, format } => {
                self.run_export(scope, format, cx);
            }
            ExportEvent::Cancel => {
                self.export_dialog = None;
                self.export_dialog_sub = None;
                cx.notify();
            }
        }
    }

    // ── SQL Console panel (P5a T5) ────────────────────────────────────────────

    /// Toggle the SQL Console bottom panel (P5a T5).
    ///
    /// On the first toggle, lazily constructs the [`SqlConsole`] from the
    /// session's persisted SQL tabs (which needs the `&mut Window` for the
    /// per-tab code editors) and subscribes to its [`SqlConsoleEvent`]. The
    /// subscription is STORED in `sql_console_sub` — a dropped `Subscription`
    /// deregisters the callback silently (the P4a T10b trap). Subsequent
    /// toggles just flip `sql_console_visible` without tearing the console
    /// down, preserving the editor buffers.
    ///
    /// Run/Cancel are wired in P5a T6/T7; for now the event handler only
    /// services `Persist`.
    ///
    /// [`SqlConsole`]: crate::view::sql_console::SqlConsole
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    #[allow(dead_code)] // wired to an action/keybind/menu in P5a T11
    pub(crate) fn toggle_sql_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sql_console.is_none() {
            let (persisted, active) = {
                let s = self.session.lock();
                (s.sql_tabs().to_vec(), s.active_sql_tab())
            };
            let console = cx.new(|cx| {
                crate::view::sql_console::SqlConsole::new(&persisted, active, window, cx)
            });
            let sub = cx.subscribe(
                &console,
                |ws: &mut Self, console, ev: &crate::view::sql_console::SqlConsoleEvent, cx| {
                    ws.on_sql_console_event(console.clone(), ev.clone(), cx);
                },
            );
            self.sql_console_sub = Some(sub);
            self.sql_console = Some(console);
            self.sql_console_visible = true;
        } else {
            self.sql_console_visible = !self.sql_console_visible;
        }
        cx.notify();
    }

    /// Route a [`SqlConsoleEvent`] from the console.
    ///
    /// Stub for T5 — only `Persist` is serviced. `Run`/`Cancel` are implemented
    /// in P5a T6/T7.
    ///
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    #[allow(dead_code)] // reached via the toggle's subscription, wired in P5a T11
    pub(crate) fn on_sql_console_event(
        &mut self,
        _console: Entity<crate::view::sql_console::SqlConsole>,
        ev: crate::view::sql_console::SqlConsoleEvent,
        cx: &mut Context<Self>,
    ) {
        use crate::view::sql_console::SqlConsoleEvent::*;
        match ev {
            Persist => self.persist_sql_console(cx),
            // T6 implements Run; T7 implements Cancel.
            Run { .. } | Cancel => {}
        }
    }

    /// Snapshot the console's tabs into the session and persist (P5a T5).
    #[allow(dead_code)] // reached via on_sql_console_event, wired in P5a T11
    pub(crate) fn persist_sql_console(&mut self, cx: &mut Context<Self>) {
        if let Some(console) = &self.sql_console {
            let app: &gpui::App = cx;
            let (tabs, active) = console.read(app).snapshot(app);
            let _ = self.session.lock().set_sql_tabs(tabs, active);
        }
    }

    /// Open the native save panel, then stream the export via COPY (P4c T11).
    ///
    /// Builds the surrogate-stripped projection SELECT off `scope` + the live
    /// view state (current-view applies rename/reorder/exclude via `column_view`;
    /// full-table is the raw base columns minus the surrogate). The save panel
    /// (`App::prompt_for_new_path`) returns a `oneshot::Receiver`, awaited on the
    /// GPUI foreground executor inside `cx.spawn`; the async engine COPY
    /// (`export_query_to_path`) is awaited directly because the tokio runtime is
    /// entered for the whole `Application::run` closure (window.rs `runtime.enter()`),
    /// mirroring the file-drop async-engine pattern. The result surfaces through
    /// the `error_ux` banner queue (the same surface as the paste-reject banner).
    pub fn run_export(
        &mut self,
        scope: crate::view::export_dialog::ExportScope,
        format: dat0_engine::types::ExportFormat,
        cx: &mut Context<Self>,
    ) {
        use crate::view::export_dialog::build_export;

        let Some(base_table) = self.base_table() else {
            self.export_dialog = None;
            self.export_dialog_sub = None;
            cx.notify();
            return;
        };
        // Active view name, already-quoted (the inner SELECT reads it directly).
        let active_view = self
            .view_model
            .as_ref()
            .and_then(|vm| vm.active_view())
            .map(|v| format!("\"{}\"", v.replace('"', "\"\"")));
        let base_columns = self
            .data_source
            .as_ref()
            .map(|ds| ds.visible_column_names())
            .unwrap_or_default();
        let (inner, cols) = build_export(
            scope,
            &base_table,
            active_view.as_deref(),
            &self.column_view,
            &base_columns,
        );
        let select = dat0_engine::render::render_export_select(&inner, &cols);
        let ext = match format {
            dat0_engine::types::ExportFormat::Csv => "csv",
            dat0_engine::types::ExportFormat::Json => "json",
            dat0_engine::types::ExportFormat::Parquet => "parquet",
        };
        let suggested = format!("export.{ext}");
        let engine = self.engine();

        // GPUI native save panel (`App::prompt_for_new_path` derefs through
        // `Context`). Returns a `oneshot::Receiver<Result<Option<PathBuf>>>`:
        // `Ok(Some(path))` on confirm, `Ok(None)` on cancel.
        let path_rx = cx.prompt_for_new_path(std::path::Path::new(""), Some(&suggested));
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, _async_cx| {
            // `export_query_to_path` is a `QueryEngine` trait method.
            use dat0_engine::QueryEngine as _;
            // `await` yields `Result<Result<Option<PathBuf>>, oneshot::Canceled>`;
            // collapse both layers to `Option<PathBuf>` (cancel / closed = None).
            let dest = match path_rx.await {
                Ok(Ok(Some(dest))) => dest,
                _ => return,
            };
            // The engine COPY is async + Send; the tokio runtime is entered for
            // the GPUI loop (window.rs `runtime.enter()`), so awaiting it here on
            // the foreground executor drives the streaming COPY to completion.
            match engine.export_query_to_path(&select, format, &dest).await {
                Ok(()) => {
                    let mut banner =
                        crate::error_ux::Banner::info(dat0_i18n::t("export.done.title"));
                    banner.body = format!("{}", dest.display());
                    crate::error_ux::push(banner);
                }
                Err(e) => {
                    crate::error_ux::push(crate::error_ux::Banner::error(
                        dat0_i18n::t("export.failed.title"),
                        e.to_string(),
                    ));
                }
            }
        })
        .detach();

        // Dismiss the dialog immediately — the save panel + COPY run async.
        self.export_dialog = None;
        self.export_dialog_sub = None;
        cx.notify();
    }

    /// PipelineBar scrubber: jump to state `k` (keep first `k` ops) as one undo
    /// step (P4c T9). Refreshes the `ColumnView` and routes the resulting
    /// `ViewChange` — display-only ops re-render immediately; data-view changes
    /// spawn an engine round-trip. No-op when no `ViewModel` is mounted.
    pub fn pipeline_jump_to(&mut self, k: usize, cx: &mut Context<Self>) {
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.jump_to(k);
        self.refresh_column_view();
        self.route_change(change, cx);
    }

    /// PipelineBar expanded timeline: remove the transform at stack position `i`
    /// in ONE undo step (P4c T10). Refreshes the `ColumnView` and routes the
    /// resulting `ViewChange` — display-only ops re-render immediately; data-view
    /// changes spawn an engine round-trip. No-op when no `ViewModel` is mounted.
    pub fn pipeline_remove_at(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.remove_at(i);
        self.refresh_column_view();
        self.route_change(change, cx);
    }

    /// Return the active inline header-rename editor for `col_ix`, if one is
    /// mounted for that column. Used by `GridTableDelegate::render_th` to render
    /// the editor in-place instead of the column label (P4c T7).
    pub fn header_rename_for(
        &self,
        col_ix: usize,
    ) -> Option<Entity<crate::grid::cell_editor::HeaderRenameEditor>> {
        self.header_rename
            .as_ref()
            .filter(|(c, _)| *c == col_ix)
            .map(|(_, e)| e.clone())
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

/// Inclusive bounding rectangle `(r0, c0, r1, c1)` over a set of `(row, col)`
/// cells, or `None` when the set is empty (T7 copy/cut). Used to build the
/// dense bounding-rect grid a discontiguous selection serializes to (gaps in
/// the rect become empty cells).
pub(crate) fn bounding_rect(cells: &[(usize, usize)]) -> Option<(usize, usize, usize, usize)> {
    let mut it = cells.iter();
    let &(r, c) = it.next()?;
    let (mut r0, mut c0, mut r1, mut c1) = (r, c, r, c);
    for &(row, col) in it {
        r0 = r0.min(row);
        c0 = c0.min(col);
        r1 = r1.max(row);
        c1 = c1.max(col);
    }
    Some((r0, c0, r1, c1))
}

impl Render for WorkspaceShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Subscribe to Theme global changes once, on the first render. The
        // subscription returns a `Subscription` that must be kept alive
        // (drop = unregister) per `gpui-api-notes.md` §0.A.2.
        //
        // P3b T12 flipped the type parameter from the T4 placeholder
        // `gpui_component::Theme` to `crate::theme::Theme` — dat0's own
        // theme type is now a `gpui::Global` (see `theme/mod.rs`), so the
        // Settings dropdown's `Theme::switch` fans out here.
        if self.theme_subscription.is_none() {
            let sub = cx.observe_global::<crate::theme::Theme>(|_view, cx| {
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
                // Build the delegate's columns from the active ColumnView so the
                // header renders display labels in display order (P4c T5). With
                // no projection ops the view is identity over the visible schema,
                // so the columns match the pre-P4c schema-derived ones exactly.
                let delegate = GridTableDelegate::new(
                    Arc::clone(ds),
                    cx.entity().downgrade(),
                    &self.column_view,
                );
                self.table_state = Some(cx.new(|cx| TableState::new(delegate, window, cx)));

                // PD-018 prefetch-on-bind: kick a background fetch of the first
                // visible page so the grid paints real values on the next frame
                // instead of em-dash placeholders. The delegate's
                // `visible_rows_changed` hook takes over on scroll. We seed a
                // generous first window (PAGE_ROWS worth) so the initial viewport
                // is fully covered even before the first scroll event fires.
                let initial_rows = usize::try_from(ds.row_count).unwrap_or(usize::MAX);
                self.prefetch_visible_rows(0, initial_rows.min(1024), cx);
            }

            // Lazily construct the selection model once a non-empty source is
            // mounted (T4/T6). `SelectionModel::new` debug-asserts non-empty
            // dimensions, so we only build it when the grid actually has cells.
            // T11 wires keyboard movers; T6 reads `selection.active()` on edit
            // commit. Rebuilt when the dimensions change (data-source swap).
            let rows = usize::try_from(ds.row_count).unwrap_or(usize::MAX);
            let cols = ds.visible_column_count();
            if rows > 0 && cols > 0 && self.selection.is_none() {
                self.selection = Some(crate::grid::selection::SelectionModel::new(rows, cols));
            }
        }

        let session = Arc::clone(&self.session);

        let drop_listener = cx.listener(move |_view, paths: &ExternalPaths, _window, cx| {
            let paths_vec: Vec<std::path::PathBuf> = paths.paths().to_vec();
            let session = Arc::clone(&session);
            cx.spawn(
                async move |weak_shell: gpui::WeakEntity<WorkspaceShell>, async_cx| {
                    let outcomes = handle_drop(paths_vec, session).await;

                    // Route any `OpenWizard` outcomes to `import_wizard::open`
                    // so T10/T11 implementers can light up the drawer view
                    // through the existing seam. The current `open()` is a
                    // stub that logs, so this is wiring-only — no behaviour
                    // change for users until the drawer lands.
                    //
                    // Per spec §3.5: the last successfully-registered file
                    // becomes the active tab. Partition outcomes into wizard
                    // requests and registered tables in one pass.
                    let mut wizard_requests: Vec<(
                        std::path::PathBuf,
                        crate::import_wizard::SniffSummary,
                    )> = Vec::new();
                    let mut last_registered: Option<String> = None;
                    for o in outcomes {
                        match o {
                            DropOutcome::Registered { table_name, .. } => {
                                last_registered = Some(table_name);
                            }
                            DropOutcome::OpenWizard { path, sniff } => {
                                wizard_requests.push((path, sniff));
                            }
                            _ => {}
                        }
                    }
                    for (path, sniff) in wizard_requests {
                        let _ = async_cx.update(|app_cx| {
                            crate::import_wizard::open(app_cx, &path, sniff);
                        });
                    }

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
                            match GridDataSource::new(engine, table_name.clone()).await {
                                Ok(ds) => {
                                    let _ = async_cx.update(|app_cx| {
                                        let _ = weak_shell.update(app_cx, |view, cx| {
                                            // T13: initialise ViewModel for the new table.
                                            // The base_table is already-quoted per ViewModel
                                            // design §4 ("base_table passed to ViewModel must
                                            // already be quoted").
                                            let quoted =
                                                format!("\"{}\"", table_name.replace('"', "\"\""));
                                            view.view_model = Some(ViewModel::new(
                                                // Use table_name as tab_id for the single-tab
                                                // P4a case; P4b will replace with a real UUID.
                                                table_name.clone(),
                                                quoted,
                                            ));
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

        // `Table<D>` and the empty-state hero are different concrete
        // types, so we widen every arm with `.into_any_element()` to
        // satisfy `impl IntoElement`'s single-return-type requirement.
        //
        // P3b T7 adds the empty-state hero branch: when no data source is
        // mounted (or the mounted source is empty), pick between the
        // "samples picker" hero (recents empty) and the recents-only hero
        // (recents non-empty). Recents emptiness is read directly from
        // disk here so the view doesn't need a plumbed-in `Arc<Mutex<Recents>>`
        // — `Recents::with_path` is a cheap JSON read and the empty-state
        // render is not on the per-row hot path.
        let body = match (self.data_source.as_ref(), self.table_state.as_ref()) {
            (Some(ds), Some(state)) if !ds.is_empty() => {
                // Real Table mount — closes the P3a T10 placeholder.
                // Per `docs/internal/gpui-table-api-notes.md` §3:
                //   `Table::new(state: &Entity<TableState<D>>) -> Self`
                // Theming flows implicitly via `cx.theme()` inside the
                // widget (spike §1.3); no prop to pass.
                let table = Table::new(state).stripe(true).bordered(true);

                // T9: mount the selection-aware right-click context menu on the
                // grid body. `ContextMenuExt::context_menu` requires
                // `ParentElement + Styled`, which the `Table` (a `RenderOnce`
                // widget) does not implement directly — so we wrap it in a
                // `div` and hang the menu off that. `build_menu` snapshots the
                // current selection flag and captures a weak handle to this
                // shell so the items dispatch into the live edit handlers.
                use crate::grid::context_menu::{ContextMenuExt, build_menu};
                let ws_weak = cx.entity().downgrade();
                // Use the active cell's column as the fallback for "Delete
                // Column" when no column selection is active (body-level menu;
                // the header right-click handler passes the header's col_ix
                // directly when that wiring lands in a later task).
                let active_col = self.selection.as_ref().map(|s| s.active().col).unwrap_or(0);
                let menu_builder = build_menu(ws_weak, self.selection.as_ref(), active_col);
                div()
                    .size_full()
                    .child(table)
                    .context_menu(menu_builder)
                    .into_any_element()
            }
            (Some(_), None) => {
                // Data source landed but TableState hasn't been promoted
                // yet (the next frame promotes it). Brief placeholder.
                div().child("Loading grid…").into_any_element()
            }
            // Either no data source, or a data source with zero rows —
            // both fall back to the empty-state hero. `recents_empty`
            // toggles the right-column content (samples vs. recents).
            _ => {
                let recents_empty = match crate::platform::config_dir() {
                    Ok(cfg) => Recents::with_path(cfg.join("recents.json"))
                        .list()
                        .is_empty(),
                    Err(_) => true,
                };
                EmptyState::new(recents_empty).render(cx).into_any_element()
            }
        };

        // Funnel-click filter popover overlay (T0 / PD-016). Anchored top-right
        // while open; the entity drives its own Apply/Cancel/Clear buttons,
        // whose `Outcome` routes back via the stored subscription. A later P4b
        // polish task can anchor it precisely under the clicked funnel icon.
        let popover_overlay: Option<gpui::AnyElement> = self.active_popover.as_ref().map(|p| {
            div()
                .absolute()
                .top_8()
                .right_4()
                .child(p.clone())
                .into_any_element()
        });

        // Inline cell-editor overlay (T6). Mounted by `begin_cell_edit` over the
        // active cell; commits via the stored `cell_editor_sub` subscription. A
        // later P4b polish task can anchor it precisely over the active cell —
        // T6 mounts it top-left so the widget is reachable for UAT (T14).
        let editor_overlay: Option<gpui::AnyElement> = self.cell_editor.as_ref().map(|e| {
            div()
                .absolute()
                .top_8()
                .left_4()
                .child(e.clone())
                .into_any_element()
        });

        // Export… dialog overlay (P4c T11). Mounted by `open_export_dialog`;
        // emits `ExportEvent` routed via the stored `export_dialog_sub`
        // subscription. Centred-ish near the top; a later polish task can centre
        // it precisely in a modal scrim.
        let export_overlay: Option<gpui::AnyElement> = self.export_dialog.as_ref().map(|d| {
            div()
                .absolute()
                .top_16()
                .left_1_2()
                .child(d.clone())
                .into_any_element()
        });

        // T10: tab-strip with dirty-dot indicator. Shown whenever a ViewModel
        // is mounted (i.e. a file has been loaded). The "•" glyph appears next
        // to the tab label when `vm.is_dirty()` is true — meaning the active
        // transformation stack contains at least one Edit or RowDelete op.
        // Undo clears the stack back past the dirty ops and the dot disappears
        // on the next render (cx.notify() fires after every rebind).
        let tab_strip: Option<gpui::AnyElement> = self.view_model.as_ref().map(|vm| {
            let is_dirty = vm.is_dirty();
            let label = vm.tab_id().to_string();
            let tab_label = h_flex()
                .gap_1()
                .items_center()
                .child(div().child(label))
                .children(is_dirty.then(|| div().child("•")));
            h_flex()
                .w_full()
                .px_3()
                .py_1()
                .border_b_1()
                .child(tab_label)
                .into_any_element()
        });

        // ── T11 / PD-018: focus ring for the active cell ─────────────────────────
        //
        // PD-018 closed the render-cache gap, so the focus ring is now drawn
        // PER-CELL inside `GridTableDelegate::render_td` (a 2-px blue border on
        // the cell at `selection.active()`, plus a lighter tint on selected
        // cells). It reads the live selection through the delegate's weak
        // `WorkspaceShell` handle, so it always tracks the current cursor and
        // re-renders whenever the selection changes (`cx.notify()` after every
        // mover / mutation). The previous bottom-left floating badge is therefore
        // removed — there is no overlay element here anymore.

        // ── T11: key-down handler — navigation keys → SelectionModel movers ──────
        //
        // The handler is attached to the outer container so it fires whenever
        // the shell has focus (tracked via `focus_handle`).
        //
        // Keys handled here:
        //   arrows (plain/shift/cmd) → `apply_key` → `SelectionModel` movers
        //   Escape                   → `apply_key(Escape)` → `SelectionModel::clear`
        //   Cmd/Ctrl+A               → `apply_key(SelectAll)`
        //   Enter / F2               → `begin_cell_edit` (T6)
        //   Cmd/Ctrl+C               → `copy_selection` (T7)
        //   Cmd/Ctrl+X               → `cut_selection` (T7)
        //   Cmd/Ctrl+V               → `paste_clipboard` (T7)
        //   Delete / Backspace       → `set_null_selection` (T8)
        //   Cmd/Ctrl+D               → `fill_down` (T8)
        //
        // Undo/Redo (Cmd-Z / Cmd-Shift-Z) are bound globally via cx.on_action
        // in run_app — do NOT rebind here.
        let key_handler = cx.listener(|ws: &mut Self, ev: &KeyDownEvent, window, cx| {
            use crate::grid::keymap::{Key, apply_key, key_from_event};

            let ks = &ev.keystroke;
            let mods = &ks.modifiers;
            let key_str = ks.key.as_str();

            // ── Check for non-navigation keys first ───────────────────────────
            // secondary = Cmd on macOS, Ctrl on Linux/Windows.
            let secondary = mods.secondary();
            let secondary_only = secondary && !mods.shift && !mods.alt;
            let no_mods = !mods.shift && !mods.platform && !mods.control && !mods.alt;

            // Enter / F2 → begin cell edit (T6).
            if (key_str == "enter" || key_str == "f2") && no_mods {
                ws.begin_cell_edit(window, cx);
                return;
            }

            // Cmd/Ctrl+C → copy (T7).
            if key_str == "c" && secondary_only {
                ws.copy_selection(cx);
                return;
            }

            // Cmd/Ctrl+X → cut (T7).
            if key_str == "x" && secondary_only {
                ws.cut_selection(cx);
                return;
            }

            // Cmd/Ctrl+V → paste (T7).
            if key_str == "v" && secondary_only {
                ws.paste_clipboard(cx);
                return;
            }

            // Delete / Backspace → set null (T8).
            if (key_str == "delete" || key_str == "backspace") && no_mods {
                ws.set_null_selection(cx);
                return;
            }

            // Cmd/Ctrl+D → fill down (T8).
            if key_str == "d" && secondary_only {
                ws.fill_down(cx);
                return;
            }

            // Escape with an open cell editor → cancel the edit and keep the
            // cursor on the cell (do NOT clear the selection). With no editor
            // open, Escape falls through to the keymap below and clears the
            // selection.
            if key_str == "escape" && no_mods && ws.cell_editor.is_some() {
                ws.cell_editor = None;
                ws.cell_editor_sub = None;
                cx.notify();
                return;
            }

            // ── Navigation keys via the pure keymap ───────────────────────────
            if let Some(nav_key) = key_from_event(ev) {
                // SelectAll (Cmd+A) is in the keymap but we still need cx.notify().
                if let Some(sel) = ws.selection.as_mut() {
                    apply_key(sel, nav_key);
                }
                // Marching-ants border (T12): clear ONLY on Escape so the user
                // can navigate to a paste target while the marquee is visible.
                // Paste clears it via `paste_clipboard`; a new copy/cut overwrites
                // it via `build_selection_tsv`.  Plain arrows / Shift+arrow /
                // Cmd+arrow / Cmd+A must NOT clear it.
                if nav_key == Key::Escape {
                    ws.copied_range = None;
                }
                cx.notify();
            }
        });

        // Request focus on click so the shell captures key events.
        let focus_handle_for_click = self.focus_handle.clone();
        let click_to_focus =
            cx.listener(move |_ws: &mut Self, _ev: &gpui::ClickEvent, window, _cx| {
                focus_handle_for_click.focus(window);
            });

        // PipelineBar (P4c T9 collapsed pills / T10 expanded timeline). Shown
        // when the active transform stack is non-empty. The render fn from
        // `view::pipeline_bar` takes the current active stack; pill/row clicks
        // and the ✕ remove use `cx.listener` (which supplies `&mut self`), so no
        // weak handle is threaded. The `⌄`/`⌃` toggle flips
        // `pipeline_bar_state.expanded` (collapsed pills ↔ expanded timeline).
        let pipeline_bar: Option<gpui::AnyElement> = {
            if let Some(vm) = self.view_model.as_ref() {
                let stack = vm.active();
                if !stack.is_empty() {
                    crate::view::pipeline_bar::render_pipeline_bar(
                        stack,
                        &mut self.pipeline_bar_state,
                        cx,
                    )
                } else {
                    None
                }
            } else {
                None
            }
        };

        // SQL Console bottom panel (P5a T5). Mounted between the PipelineBar and
        // the grid body when the console exists AND is visible. A fixed-height
        // panel with a top border; the inner `SqlConsole` entity renders the tab
        // strip + code editor + result region.
        let sql_console_panel: Option<gpui::AnyElement> = self
            .sql_console
            .as_ref()
            .filter(|_| self.sql_console_visible)
            .map(|c| {
                div()
                    .h(px(260.))
                    .w_full()
                    .border_t_1()
                    .child(c.clone())
                    .into_any_element()
            });

        div()
            .id("workspace-shell")
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .track_focus(&self.focus_handle)
            .on_key_down(key_handler)
            .on_click(click_to_focus)
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
            .on_drop::<ExternalPaths>(drop_listener)
            .children(tab_strip)
            .children(pipeline_bar)
            .children(sql_console_panel)
            .child(div().flex_1().child(body))
            .children(popover_overlay)
            .children(editor_overlay)
            .children(export_overlay)
    }
}
