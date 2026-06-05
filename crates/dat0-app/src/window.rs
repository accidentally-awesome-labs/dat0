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
            let view = cx.new(|cx| {
                let mut shell = WorkspaceShell::new(Arc::clone(&session), cx);
                shell.reconnect_persisted_md(cx);
                shell
            });
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

        // Wire the SQL Console keystrokes (P5a T11):
        //   Cmd+Enter / Ctrl+Enter      → SqlRun    (run the active statement)
        //   Cmd+.     / Ctrl+.          → SqlCancel (interrupt the in-flight run)
        //   Cmd+Shift+C / Ctrl+Shift+C  → SqlConsoleToggle (show/hide the console)
        //
        // Unlike Export/Undo/Redo (handled by GLOBAL `cx.on_action` here in
        // run_app), these actions are handled VIEW-scoped on the WorkspaceShell
        // root in `render` — they reach `self`, and toggle/new-tab need a
        // `&mut Window` that the App-level dispatch path can't supply. We only
        // register the keystrokes here; gpui routes the dispatched action up the
        // focused element tree to the shell's `.on_action` handlers. SqlNewTab /
        // SqlCloseTab are reachable via the menu + command palette (and the
        // console's own "+"/"✕" tab buttons) — no default keystroke is bound to
        // avoid colliding with the editor's own text-editing keymap.
        {
            #[cfg(target_os = "macos")]
            let (run_ks, cancel_ks, toggle_ks) = ("cmd-enter", "cmd-.", "cmd-shift-c");
            #[cfg(not(target_os = "macos"))]
            let (run_ks, cancel_ks, toggle_ks) = ("ctrl-enter", "ctrl-.", "ctrl-shift-c");
            cx.bind_keys([
                gpui::KeyBinding::new(run_ks, crate::menu_macos::SqlRun, None),
                gpui::KeyBinding::new(cancel_ks, crate::menu_macos::SqlCancel, None),
                gpui::KeyBinding::new(toggle_ks, crate::menu_macos::SqlConsoleToggle, None),
            ]);
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
                    let view = cx.new(|cx| {
                        let mut shell = WorkspaceShell::new(Arc::clone(&session_for_window), cx);
                        shell.reconnect_persisted_md(cx);
                        shell
                    });
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
/// What a confirmed [`NamePrompt`](crate::view::name_prompt::NamePrompt)
/// should do (P5b T8 + T10). The shared single-line name modal is reused for
/// several "name this thing" flows; the intent is the single routing point for
/// the `Confirm(name)` arm in
/// [`on_name_prompt_event`](WorkspaceShell::on_name_prompt_event), so adding a
/// new flow is a new variant + a new match arm — nothing else moves.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// All three variants are intentionally `Save*` — they name distinct "save this
// thing as…" flows routed by `on_name_prompt_event`. The shared prefix is
// meaningful, not redundant, so the `enum_variant_names` lint is suppressed.
#[allow(clippy::enum_variant_names)]
enum NamePromptIntent {
    /// Save the captured SQL (`name_prompt_sql`) as a named saved query (T8).
    SaveQuery,
    /// Promote the console statement-under-cursor to a derived table (T10).
    SaveConsoleAsTable,
    /// Promote the active grid view's transform stack to a derived table,
    /// recording its lineage as `DerivedOrigin::Transform { parent, ops }`
    /// (T11). The handler re-reads the `ViewModel` on confirm, so no per-intent
    /// state is captured up front.
    SaveViewAsTable,
}

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
    /// Written (never explicitly read); the field's sole purpose is to keep the
    /// `Subscription` alive for the entity's life so `on_sql_console_event` keeps
    /// firing. Dropping a `Subscription` deregisters silently, so this must be a
    /// stored field — hence the lint allowance (a keep-alive, not dead code).
    ///
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    #[allow(dead_code)] // keep-alive: storing the Subscription is the read
    pub(crate) sql_console_sub: Option<Subscription>,
    /// Whether the SQL Console panel is currently shown. Toggled by
    /// `toggle_sql_console`; the render gate respects this independently of
    /// whether `sql_console` is `Some`.
    pub(crate) sql_console_visible: bool,
    /// Whether the window-close `Persist` backstop has been registered (P5a
    /// T10). Set the first time the console is built so the
    /// `on_window_should_close` hook is installed exactly once per window.
    pub(crate) sql_console_close_hooked: bool,
    /// Cancellation guard for the in-flight SQL console run (P5a T6). `Some`
    /// while a run is executing; dropped/disarmed in `finish_sql_run`. The
    /// guard's `Drop` (or an explicit `cancel()` in T7) fires the engine's
    /// connection-wide `interrupt()`.
    pub(crate) active_query_cancel: Option<crate::query::QueryCancel>,
    /// Shared per-window autocomplete schema cache (P5b T2). Lazily created on
    /// the first `toggle_sql_console` (so it can be cloned into the console's
    /// per-tab providers), then refreshed off the engine on console-open and
    /// after every run. `None` until the console is first opened.
    pub(crate) sql_snapshot: Option<crate::query::completion::SharedSnapshot>,
    /// Currently-mounted Save-query name prompt (P5b T8). `Some` while the
    /// 💾 → Save-query modal is open; cleared when its
    /// [`NamePromptEvent`](crate::view::name_prompt::NamePromptEvent) is routed
    /// (Confirm → save + dismiss, or Cancel → dismiss). Rendered as a window
    /// overlay child in `render` when present.
    name_prompt: Option<Entity<crate::view::name_prompt::NamePrompt>>,
    /// Subscription to the active name prompt's `NamePromptEvent`. Stored so the
    /// Confirm/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside `name_prompt`.
    name_prompt_sub: Option<Subscription>,
    /// The active tab's SQL captured at the moment 💾 was pressed (P5b T8). Held
    /// while the name prompt is open so a Confirm saves the SQL as it was THEN,
    /// not whatever is in the editor when the user finishes typing the name.
    /// Only the `SaveQuery` intent uses this; `SaveConsoleAsTable` re-reads the
    /// statement-under-cursor on confirm, so it leaves this `None`.
    name_prompt_sql: Option<String>,
    /// What the currently-open name prompt should do on Confirm (P5b T8 + T10).
    /// `Some` exactly while `name_prompt` is mounted; the `Confirm` arm of
    /// [`on_name_prompt_event`](Self::on_name_prompt_event) matches on it to
    /// route to the right handler. Cleared alongside `name_prompt`.
    name_prompt_intent: Option<NamePromptIntent>,
    /// Whether the window-level saved-query picker overlay is shown (P5b T8).
    /// Toggled by `show_saved_picker` (📑) / closed on pick or the overlay's ✕.
    /// The overlay reads `session.saved_queries()` live at render, so no
    /// snapshot is stored here — the flag alone gates the overlay.
    saved_picker_open: bool,
    /// Runtime connection state (MotherDuck status + sqlite attachments) for this
    /// window (P5c T6/T10). The persisted projection lives in
    /// `SessionState.attachments` (T7); this is the live UI-facing copy the
    /// Connections panel renders from.
    pub(crate) connections: crate::connections::ConnectionManager,
    /// Whether the left-dock Connections panel is shown (P5c T10/T11). Toggled by
    /// the `ConnectionsToggle` action; gates the panel in `render`.
    pub(crate) connections_panel_visible: bool,
    /// Token-entry modal (reuses [`NamePrompt`](crate::view::name_prompt::NamePrompt)).
    /// `Some` while the MotherDuck token prompt is open; cleared on Confirm /
    /// Cancel. Rendered as a window overlay child in `render` when present.
    pub(crate) md_token_prompt: Option<gpui::Entity<crate::view::name_prompt::NamePrompt>>,
    /// Subscription to the token prompt's `NamePromptEvent`. Stored so the
    /// Confirm/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside
    /// `md_token_prompt`.
    pub(crate) md_token_prompt_sub: Option<gpui::Subscription>,
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
            sql_console_close_hooked: false,
            active_query_cancel: None,
            sql_snapshot: None,
            name_prompt: None,
            name_prompt_sub: None,
            name_prompt_sql: None,
            name_prompt_intent: None,
            saved_picker_open: false,
            connections: Default::default(),
            connections_panel_visible: false,
            md_token_prompt: None,
            md_token_prompt_sub: None,
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

    /// Prefetch the page(s) covering screen rows `[start, end)` into the MAIN
    /// grid's `GridDataSource` LRU so the grid's synchronous `render_td` paints
    /// real values for the rows the user can see (PD-018).
    ///
    /// Thin wrapper over [`Self::prefetch_rows_for`] bound to `self.data_source`.
    /// Callers that page a DIFFERENT source (e.g. the console results pane, which
    /// owns a separate `GridDataSource` with its own LRU) must call
    /// `prefetch_rows_for(&that_source, …)` directly so the right cache is
    /// populated (P5a T9).
    pub fn prefetch_visible_rows(&self, start: usize, end: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self.data_source.as_ref() {
            let ds = Arc::clone(ds);
            self.prefetch_rows_for(&ds, start, end, cx);
        }
    }

    /// Source-parameterized prefetch: load the page(s) covering screen rows
    /// `[start, end)` into `ds`'s OWN LRU, then notify the shell so the mounted
    /// view repaints with real values.
    ///
    /// Each [`crate::grid::GridDataSource`] owns a SEPARATE `Mutex<LruCache>`, so
    /// a view's `render_td` only ever finds pages that were fetched into THAT
    /// view's source. The main grid drives this via
    /// [`Self::prefetch_visible_rows`] (passing `self.data_source`); the
    /// console-owned results pane drives it via the delegate's
    /// `visible_rows_changed` hook (passing the PANE's source). Routing both
    /// through this one method means pane scrolling loads the pane's cache and
    /// leaves the main grid's cache untouched (P5a T9 fix).
    ///
    /// The fetch runs OFF the GPUI main thread — `GridDataSource::page_for` is
    /// async DuckDB I/O and must never block the 60 fps render loop. Once the
    /// page is in the LRU, the re-render `notify` is posted back onto the main
    /// thread via the [`crate::main_bridge::MainThreadDispatcher`] (the canonical
    /// `spawn_view_change` discipline — NEVER `cx.update` from the tokio task).
    pub(crate) fn prefetch_rows_for(
        &self,
        ds: &Arc<crate::grid::GridDataSource>,
        start: usize,
        end: usize,
        cx: &mut Context<Self>,
    ) {
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
                        tracing::warn!(row, error = %e, "prefetch_rows_for: page_for failed");
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
                    "prefetch_rows_for: no MainThreadDispatcher installed; grid will not refresh"
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
    pub(crate) fn toggle_sql_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sql_console.is_none() {
            let (persisted, active) = {
                let s = self.session.lock();
                (s.sql_tabs().to_vec(), s.active_sql_tab())
            };
            // Ensure the per-window autocomplete snapshot exists, then clone it
            // into the console so every tab's provider shares one `RefCell`
            // (P5b T2). The refresh below populates `tables` off the engine.
            let snapshot = self
                .sql_snapshot
                .get_or_insert_with(crate::query::completion::new_shared_snapshot)
                .clone();
            let console = cx.new(|cx| {
                crate::view::sql_console::SqlConsole::new(
                    &persisted,
                    active,
                    snapshot.clone(),
                    window,
                    cx,
                )
            });
            // `subscribe_in` (not `subscribe`) so the event callback receives a
            // live `&mut Window` — the Save-query path (`SaveQuery`) builds a
            // `NamePrompt` whose single-line `InputState` needs one eagerly
            // (P5b T8). The window is valid because the subscription fires inside
            // a window update.
            let sub = cx.subscribe_in(
                &console,
                window,
                |ws: &mut Self,
                 console,
                 ev: &crate::view::sql_console::SqlConsoleEvent,
                 window,
                 cx| {
                    ws.on_sql_console_event(console.clone(), ev.clone(), window, cx);
                },
            );
            self.sql_console_sub = Some(sub);
            self.sql_console = Some(console);
            self.sql_console_visible = true;

            // Persist the console one last time on window close (P5a T10). This
            // is a best-effort backstop ON TOP OF the guaranteed per-mutation
            // persists (Run / tab add / close / active-switch each emit
            // `Persist` → `set_sql_tabs` → disk), so disk is already current;
            // the close hook flushes any edit-buffer text typed since the last
            // mutation. Registered once, the first time the console is built
            // (we hold the only `&mut Window` here). `should_close` returns
            // `true` so the default close proceeds.
            if !self.sql_console_close_hooked {
                self.sql_console_close_hooked = true;
                let ws_weak = cx.entity().downgrade();
                window.on_window_should_close(cx, move |_window, app| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| ws.persist_sql_console(cx));
                    }
                    true
                });
            }
        } else {
            self.sql_console_visible = !self.sql_console_visible;
        }
        // Refresh the autocomplete schema whenever the console is (re)shown so
        // tables created/dropped while it was hidden are reflected (P5b T2).
        if self.sql_console_visible {
            self.refresh_completion_snapshot(cx);
        }
        cx.notify();
    }

    /// Rebuild the autocomplete schema snapshot off the live engine (P5b T2).
    /// Runs `get_tables()` OFF the GPUI main thread, then posts the result back
    /// via the canonical `MainThreadDispatcher` and writes the shared `RefCell`
    /// ON the main thread. Called on console-open and after every run (covers
    /// CREATE/DROP/Save-as-Table).
    ///
    /// Send discipline: `SharedSnapshot` is `Rc<RefCell<..>>` — neither `Send`
    /// nor allowed in the dispatcher's `Send + 'static` closure. So the snapshot
    /// is NEVER captured across the thread boundary. Instead a weak
    /// `WorkspaceShell` handle (Send) crosses into the task; the dispatcher
    /// closure upgrades it on the main thread and reaches `self.sql_snapshot`
    /// there, mirroring the `finish_sql_run` / `prefetch_rows_for` bridge.
    pub(crate) fn refresh_completion_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.sql_snapshot.is_none() {
            return; // console never opened; nothing to refresh
        }
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let tables = match engine.get_tables().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "refresh_completion_snapshot: get_tables failed");
                    return;
                }
            };
            // `tables` is `Vec<TableInfo>` (Send). Build the `TableEntry`s and
            // write the shared `RefCell` on the main thread, where the `Rc` is
            // reachable via the upgraded shell handle.
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else { return };
                    ws.update(app_cx, |ws, _cx| {
                        let Some(snapshot) = ws.sql_snapshot.as_ref() else {
                            return;
                        };
                        let entries = tables
                            .iter()
                            .map(|t| crate::query::completion::TableEntry {
                                name: t.name.clone().into(),
                                columns: t.columns.iter().map(|c| c.name.clone().into()).collect(),
                            })
                            .collect();
                        snapshot.borrow_mut().tables = entries;
                    });
                });
            } else {
                tracing::warn!(
                    "refresh_completion_snapshot: no MainThreadDispatcher installed; snapshot stale"
                );
            }
        });
    }

    /// Route a [`SqlConsoleEvent`] from the console.
    ///
    /// T5 stubbed `Run`/`Cancel`; T6 implements `Run` (statement resolve →
    /// VIEW/EXEC → bind grid). `Cancel` lands in T7.
    ///
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    pub(crate) fn on_sql_console_event(
        &mut self,
        console: Entity<crate::view::sql_console::SqlConsole>,
        ev: crate::view::sql_console::SqlConsoleEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::sql_console::SqlConsoleEvent::*;
        match ev {
            Persist => self.persist_sql_console(cx),
            Run { target } => self.spawn_sql_run(console, target, cx),
            Cancel => self.cancel_sql_run(cx),
            // P5b T5: fetch the session's persisted history (newest last in the
            // store; the list view reverses to newest-first) and hand it to the
            // console, which renders the overlay where a row click owns a live
            // `Window` to load into a new tab.
            ShowHistory => {
                let entries = self.session.lock().query_history().to_vec();
                console.update(cx, |c, cx| c.show_history(entries, cx));
            }
            // P5b T8: capture the active tab's SQL NOW and open the Save-query
            // name-prompt overlay (export-dialog idiom). Confirm → save; Cancel →
            // dismiss (both in `on_name_prompt_event`).
            SaveQuery => {
                let sql = console.read(cx).active_sql_and_cursor(cx).0;
                self.open_name_prompt(sql, window, cx);
            }
            // P5b T8: mount the window-level saved-query picker overlay. Picking
            // a row queues its SQL into a new tab (via the console's `queue_load`,
            // drained by `SqlConsole::render` with a real `Window`); deleting a
            // row removes it from the session and refreshes the overlay.
            ShowSaved => {
                self.show_saved_picker(cx);
            }
            // P5b T10: open the shared name modal with the SaveConsoleAsTable
            // intent. Confirm re-reads the statement-under-cursor and CTAS-
            // promotes it via `create_table(.., DerivedOrigin::Sql)`; the
            // SaveQuery path's captured-SQL snapshot is not needed here.
            SaveAsTable => {
                self.open_name_prompt_with(
                    "Save as table…",
                    NamePromptIntent::SaveConsoleAsTable,
                    window,
                    cx,
                );
            }
        }
    }

    /// Fire the active run's cancel drop-guard (P5a T7). `QueryCancel::cancel()`
    /// invokes the engine's connection-wide `interrupt()`, so the in-flight
    /// `spawn_sql_run` task's engine call resolves to `EngineError::Interrupted`,
    /// which `classify_run_err` maps to `SqlRunOutcome::Cancelled`; `finish_sql_run`
    /// then renders the muted "Cancelled" region and clears `running`.
    ///
    /// Safe when there is no active run (`active_query_cancel` is `None`). Safe
    /// under double-cancel: `QueryCancel::cancel()` is idempotent (disarms after
    /// firing), and `finish_sql_run`'s later `take()+disarm()` on the
    /// already-disarmed guard is a no-op.
    pub(crate) fn cancel_sql_run(&mut self, _cx: &mut Context<Self>) {
        if let Some(g) = self.active_query_cancel.as_mut() {
            g.cancel(); // fires engine.interrupt(); the in-flight task resolves to Cancelled
        }
    }

    /// Snapshot the console's tabs into the session and persist (P5a T5).
    /// Now LIVE — called from `finish_sql_run` after every run (T6).
    ///
    /// Persistence cadence (P5a T10): every console mutation that emits
    /// `SqlConsoleEvent::Persist` routes here — Run, tab add (`new_tab`), tab
    /// close (`close_tab`), and active-tab switch — plus a window-close backstop
    /// registered in `toggle_sql_console`. Editor-buffer text typed between
    /// mutations is captured by the next mutation or the close backstop. Blur is
    /// intentionally NOT wired: `InputState` owns its focus handle internally
    /// (no clean seam to subscribe its blur at this gpui-component rev), and the
    /// guaranteed per-mutation + close triggers already keep disk current.
    pub(crate) fn persist_sql_console(&mut self, cx: &mut Context<Self>) {
        if let Some(console) = &self.sql_console {
            let app: &gpui::App = cx;
            let (tabs, active) = console.read(app).snapshot(app);
            let _ = self.session.lock().set_sql_tabs(tabs, active);
        }
    }

    /// Short, stable per-window discriminator for the TEMP VIEW name. The
    /// session `window_id` is a `Uuid`; its canonical `to_string()` always
    /// renders `8-4-4-4-12` hex, so the first 4 chars are always ASCII hex.
    fn window_disc(&self) -> String {
        self.session.lock().window_id.to_string()[..4].to_string()
    }

    /// Execute the SQL statement under the cursor OFF the GPUI main thread and
    /// bind the result to the grid (P5a T6). Structurally mirrors
    /// [`crate::view::spawn_view_change`] / `run_view_change_inner`: the engine
    /// round-trip + `GridDataSource::new` run inside a `tokio::spawn`, then the
    /// main-thread apply is posted back via the [`MainThreadDispatcher`]
    /// (`crate::window_registry::dispatcher`). NEVER `cx.update` from the task.
    ///
    /// **Cursor-only** (T0 spike): there is no public selection accessor on
    /// `InputState` at this gpui-component rev, so the run statement is resolved
    /// via [`crate::query::statement::statement_at`] from the editor cursor.
    ///
    /// [`MainThreadDispatcher`]: crate::main_bridge::MainThreadDispatcher
    pub(crate) fn spawn_sql_run(
        &mut self,
        console: gpui::Entity<crate::view::sql_console::SqlConsole>,
        target: crate::query::ResultTarget,
        cx: &mut Context<Self>,
    ) {
        use crate::query::statement::{ResultKind, classify, statement_at};
        use crate::view::sql_console::ResultRegion;

        // Resolve the statement under the cursor (cursor-only; no selection).
        let (sql, cursor) = console.read(cx).active_sql_and_cursor(cx);
        let span = statement_at(&sql, cursor);
        let stmt = sql[span.start..span.end].trim().to_string();
        if stmt.is_empty() {
            return;
        }
        let kind = classify(&stmt);

        let engine = self.engine();
        let win_disc = self.window_disc();
        let tab_ix = console.read(cx).active;
        let view_name = crate::query::result_view_name(&win_disc, tab_ix);

        // Flip the console into the running state immediately.
        console.update(cx, |c, cx| {
            c.set_running(true, cx);
            c.set_region(ResultRegion::Empty, cx);
        });
        self.active_query_cancel = Some(crate::query::QueryCancel::new(&engine));

        let ws_weak = cx.entity().downgrade();
        let console_weak = console.downgrade();
        let engine_for_task = std::sync::Arc::clone(&engine);

        tokio::spawn(async move {
            // `create_or_replace_view` / `execute` are `QueryEngine` trait methods.
            use dat0_engine::QueryEngine as _;
            let outcome: SqlRunOutcome = match kind {
                ResultKind::Result => match engine_for_task
                    .create_or_replace_view(&view_name, &stmt)
                    .await
                {
                    Ok(()) => match crate::grid::GridDataSource::new(
                        std::sync::Arc::clone(&engine_for_task),
                        view_name.clone(),
                    )
                    .await
                    {
                        Ok(ds) => SqlRunOutcome::Bound(std::sync::Arc::new(ds)),
                        Err(e) => SqlRunOutcome::Error(e.to_string()),
                    },
                    Err(e) => classify_run_err(e),
                },
                ResultKind::Exec => match engine_for_task.execute(&stmt).await {
                    Ok(r) => SqlRunOutcome::Status(format_exec_status(&r)),
                    Err(e) => classify_run_err(e),
                },
            };
            // Post the apply onto the GPUI main thread. Matches the dispatcher
            // discipline of `run_view_change_inner` / `prefetch_visible_rows`.
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    if let (Some(ws), Some(console)) = (ws_weak.upgrade(), console_weak.upgrade()) {
                        ws.update(app_cx, |ws, cx| {
                            ws.finish_sql_run(&console, target, outcome, cx);
                        });
                    }
                });
            } else {
                tracing::warn!("spawn_sql_run: no MainThreadDispatcher installed; result dropped");
            }
        });
    }

    /// Apply a completed SQL run on the GPUI main thread (P5a T6). Disarms the
    /// cancel guard, clears the running flag, then routes the outcome: a bound
    /// result rebinds the grid; status/error/cancelled render the inline strip.
    fn finish_sql_run(
        &mut self,
        console: &gpui::Entity<crate::view::sql_console::SqlConsole>,
        target: crate::query::ResultTarget,
        outcome: SqlRunOutcome,
        cx: &mut Context<Self>,
    ) {
        use crate::view::sql_console::ResultRegion;

        // Normal completion: disarm so the dropped guard does NOT interrupt.
        if let Some(mut g) = self.active_query_cancel.take() {
            g.disarm();
        }

        // Compute elapsed from the console's run-start stamp BEFORE set_running(false)
        // clears it (set_running(false) sets started_at = None). Capture the FULL
        // editor buffer (not just the statement under cursor) — history shows what
        // the user ran/typed, and load re-opens the whole buffer.
        let elapsed_ms = console
            .read(cx)
            .started_at
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let (sql_text, _) = console.read(cx).active_sql_and_cursor(cx);
        let ok = !matches!(outcome, SqlRunOutcome::Error(_) | SqlRunOutcome::Cancelled);
        // P5c T9: routing tag for the chip, using the live set of attached
        // MotherDuck database names (workspace mode attaches them under real
        // names) so a query is tagged `md`/`mixed` only when it references one
        // that is actually attached.
        let routing = crate::connections::routing::classify_routing(
            &sql_text,
            self.connections.md_databases(),
        );
        console.update(cx, |c, cx| c.set_last_elapsed(elapsed_ms, routing, cx));
        {
            let entry = crate::session::queries::HistoryEntry {
                sql: sql_text,
                ran_at: now_unix_millis(),
                ok,
                elapsed_ms,
            };
            let mut sess = self.session.lock();
            let mut hist = sess.query_history().to_vec();
            crate::session::queries::push_history(&mut hist, entry);
            let _ = sess.set_query_history(hist);
        }

        console.update(cx, |c, cx| c.set_running(false, cx));

        match outcome {
            SqlRunOutcome::Bound(ds) => match target {
                crate::query::ResultTarget::MainGrid => {
                    self.apply_view_change(ds, cx);
                    console.update(cx, |c, cx| c.set_region(ResultRegion::BoundToGrid, cx));
                }
                crate::query::ResultTarget::Pane => {
                    // T9 (Tier 2): route into the console-owned results grid
                    // instead of the main DataGrid. `set_pane_source` stores the
                    // `Arc` + this shell's weak handle (for the pane delegate's
                    // header/scroll closures) and kicks a first-page prefetch;
                    // the console's `render` lazily promotes it to a `TableState`
                    // (it owns the `&mut Window` this callback lacks). The main
                    // grid / table tab is left untouched.
                    let ws_weak = cx.entity().downgrade();
                    console.update(cx, |c, cx| {
                        c.set_pane_source(ds, ws_weak, cx);
                        c.set_region(ResultRegion::Pane, cx);
                    });
                }
            },
            SqlRunOutcome::Status(s) => {
                console.update(cx, |c, cx| c.set_region(ResultRegion::Status(s), cx))
            }
            SqlRunOutcome::Error(e) => {
                console.update(cx, |c, cx| c.set_region(ResultRegion::Error(e), cx))
            }
            SqlRunOutcome::Cancelled => {
                console.update(cx, |c, cx| c.set_region(ResultRegion::Cancelled, cx))
            }
        }
        self.persist_sql_console(cx);
        // Pick up tables created/dropped by this run (CREATE/DROP/Save-as-Table)
        // so autocomplete reflects the new schema on the next keystroke (P5b T2).
        self.refresh_completion_snapshot(cx);
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

    /// Persist the current tab's SQL as a named saved query (P5b T6). Upserts by
    /// name (case-insensitive). No-op on empty name/sql. Called from
    /// [`on_name_prompt_event`](Self::on_name_prompt_event) on a Save confirm (T8).
    pub(crate) fn save_named_query(&mut self, name: String, sql: String, _cx: &mut Context<Self>) {
        if name.trim().is_empty() || sql.trim().is_empty() {
            return;
        }
        let q = crate::session::queries::SavedQuery {
            id: uuid::Uuid::now_v7(),
            name: name.trim().to_string(),
            sql,
            saved_at: now_unix_millis(),
        };
        let mut sess = self.session.lock();
        let mut list = sess.saved_queries().to_vec();
        crate::session::queries::upsert_saved(&mut list, q);
        let _ = sess.set_saved_queries(list);
    }

    /// Promote the statement under the cursor to a derived table (P5b T10).
    /// Called from [`on_name_prompt_event`](Self::on_name_prompt_event) on a
    /// confirm of the [`SaveConsoleAsTable`](NamePromptIntent::SaveConsoleAsTable)
    /// intent. Resolves the statement-under-cursor itself (it does NOT use the
    /// SaveQuery captured-SQL), wraps it in a CTAS-style `SELECT * FROM (…)`, and
    /// runs `create_table(.., DerivedOrigin::Sql)` off-thread. On success the
    /// console shows a status line and the autocomplete snapshot is refreshed so
    /// the new table appears in completions; on failure (bad SQL, name
    /// collision) the DuckDB error renders inline in the console's Error region
    /// (no modal — sidesteps PD-021).
    ///
    /// Send discipline (matches the T2/T6/T8 bridge): only `Send + 'static`
    /// values cross into the `tokio::spawn` — the engine `Arc`, the owned
    /// `name`/`stmt`/`select` strings, and the `Weak` shell/console handles. The
    /// GPUI entities are touched ONLY inside the dispatcher closure on the main
    /// thread after `.upgrade()`.
    pub(crate) fn save_console_as_table(&mut self, name: String, _cx: &mut Context<Self>) {
        let Some(console) = self.sql_console.clone() else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let (sql, cursor) = {
            let app: &gpui::App = _cx;
            console.read(app).active_sql_and_cursor(app)
        };
        let span = crate::query::statement::statement_at(&sql, cursor);
        let stmt = sql[span.start..span.end].trim().to_string();
        if stmt.is_empty() {
            return;
        }
        let select = format!("SELECT * FROM ({stmt})");
        let engine = self.engine();
        let ws_weak = _cx.entity().downgrade();
        let console_weak = console.downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let origin = dat0_engine::DerivedOrigin::Sql(stmt);
            let outcome = engine.create_table(&name, &select, origin).await;
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let (Some(ws), Some(console)) = (ws_weak.upgrade(), console_weak.upgrade()) {
                        ws.update(app, |ws, cx| match &outcome {
                            Ok(_) => {
                                console.update(cx, |c, cx| {
                                    c.set_region(
                                        crate::view::sql_console::ResultRegion::Status(format!(
                                            "Saved table {name}"
                                        )),
                                        cx,
                                    )
                                });
                                ws.refresh_completion_snapshot(cx);
                            }
                            Err(e) => console.update(cx, |c, cx| {
                                c.set_region(
                                    crate::view::sql_console::ResultRegion::Error(e.to_string()),
                                    cx,
                                )
                            }),
                        });
                    }
                });
            } else {
                tracing::warn!(
                    "save_console_as_table: no MainThreadDispatcher installed; result dropped"
                );
            }
        });
    }

    /// Open the shared name-prompt overlay to promote the active grid view's
    /// transform stack to a derived table (P5b T11). Guards on an active
    /// `ViewModel` with a non-empty op stack (no-op otherwise — the PipelineBar
    /// pill already only renders in that case, but this is defensive). The
    /// `ViewModel` is re-read on confirm by [`save_view_as_table`], so nothing
    /// is captured here beyond opening the modal with the
    /// [`SaveViewAsTable`](NamePromptIntent::SaveViewAsTable) intent.
    pub(crate) fn open_save_view_as_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        if vm.active().is_empty() {
            return;
        }
        self.open_name_prompt_with(
            "Save view as table…",
            NamePromptIntent::SaveViewAsTable,
            window,
            cx,
        );
    }

    /// Promote the active grid view's transform stack to a derived table (P5b
    /// T11), invoked from the [`SaveViewAsTable`](NamePromptIntent::SaveViewAsTable)
    /// Confirm arm of [`on_name_prompt_event`](Self::on_name_prompt_event).
    ///
    /// Compiles the active op stack against the base table via
    /// [`compile_view_sql`](dat0_engine::compile_view_sql) for the CTAS SQL, and
    /// records the parent + ops as `DerivedOrigin::Transform` — the
    /// lineage-meaningful path (the engine now honors the passed origin, see the
    /// T11 engine fix). On success the autocomplete snapshot is refreshed so the
    /// new table appears in completions; on failure the error is logged.
    ///
    /// Send discipline (matches the T2/T8/T10 bridge): only `Send + 'static`
    /// values cross into `tokio::spawn` — the engine `Arc`, the owned
    /// `name`/`base`/`sql` strings + `ops` vec, and the `Weak` shell handle. The
    /// GPUI entity is touched ONLY inside the dispatcher closure after
    /// `.upgrade()`.
    pub(crate) fn save_view_as_table(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let base = vm.base_table().to_string();
        let ops = vm.active().to_vec();
        if ops.is_empty() {
            return;
        }
        let sql = match dat0_engine::compile_view_sql(&base, &ops) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "save_view_as_table: compile failed");
                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                    dat0_i18n::t("save_as_table.failed.title"),
                    format!("{e}"),
                ));
                return;
            }
        };
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let origin = dat0_engine::DerivedOrigin::Transform { parent: base, ops };
            let outcome = engine.create_table(&name, &sql, origin).await;
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| match &outcome {
                            Ok(_) => ws.refresh_completion_snapshot(cx),
                            Err(e) => {
                                tracing::warn!(error = %e, "save_view_as_table failed");
                                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                                    dat0_i18n::t("save_as_table.failed.title"),
                                    format!("{e}"),
                                ));
                            }
                        });
                    }
                });
            } else {
                tracing::warn!(
                    "save_view_as_table: no MainThreadDispatcher installed; result dropped"
                );
            }
        });
    }

    /// Delete a saved query by id (P5b T6). Called from the saved-query picker's
    /// per-row ✕ (T8).
    pub(crate) fn delete_named_query(&mut self, id: uuid::Uuid, _cx: &mut Context<Self>) {
        let mut sess = self.session.lock();
        let mut list = sess.saved_queries().to_vec();
        crate::session::queries::delete_saved(&mut list, id);
        let _ = sess.set_saved_queries(list);
    }

    /// Mount the Save-query name-prompt overlay (P5b T8). Thin wrapper over the
    /// generalized [`open_name_prompt_with`](Self::open_name_prompt_with): it
    /// captures the active tab's SQL (held in `name_prompt_sql` so a later
    /// Confirm saves THAT text, not whatever is in the editor by then) and opens
    /// the modal with the [`SaveQuery`](NamePromptIntent::SaveQuery) intent.
    pub(crate) fn open_name_prompt(
        &mut self,
        sql: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name_prompt_sql = Some(sql);
        self.open_name_prompt_with("Save query as…", NamePromptIntent::SaveQuery, window, cx);
    }

    /// Mount the shared single-line name-prompt overlay for a given `intent`
    /// (P5b T8 generalized; T10). The `intent` is the ONLY thing that varies the
    /// Confirm behaviour — it is stashed in `name_prompt_intent` and matched in
    /// [`on_name_prompt_event`](Self::on_name_prompt_event).
    ///
    /// Mirrors [`open_export_dialog`](Self::open_export_dialog): build the entity
    /// via `cx.new`, subscribe to its `NamePromptEvent`, and STORE the
    /// subscription in `name_prompt_sub` (a dropped `Subscription` deregisters
    /// the callback silently — the P4a T10b trap).
    ///
    /// Per-intent inputs (e.g. the captured SQL for `SaveQuery`) are set by the
    /// caller BEFORE calling this; the `SaveConsoleAsTable` intent needs none
    /// (it re-reads the statement-under-cursor on confirm).
    ///
    /// Needs `&mut Window` because `NamePrompt::new` builds an `InputState`
    /// (single-line name field) eagerly.
    fn open_name_prompt_with(
        &mut self,
        title: impl Into<gpui::SharedString>,
        intent: NamePromptIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::name_prompt::{NamePrompt, NamePromptEvent};
        let prompt = cx.new(|cx| NamePrompt::new(title, window, cx));
        let sub = cx.subscribe(
            &prompt,
            |ws: &mut Self, _prompt, ev: &NamePromptEvent, cx| {
                ws.on_name_prompt_event(ev.clone(), cx);
            },
        );
        self.name_prompt_sub = Some(sub);
        self.name_prompt_intent = Some(intent);
        self.name_prompt = Some(prompt);
        cx.notify();
    }

    /// Route a `NamePromptEvent` from the shared name modal (P5b T8 + T10).
    /// `Confirm` dispatches on the stored [`NamePromptIntent`] to the right
    /// handler (the single routing point — a new flow is one new arm here);
    /// `Cancel` just dismisses. Either way the entity + subscription + per-intent
    /// state are dropped (closes the overlay).
    fn on_name_prompt_event(
        &mut self,
        ev: crate::view::name_prompt::NamePromptEvent,
        cx: &mut Context<Self>,
    ) {
        use crate::view::name_prompt::NamePromptEvent;
        if let NamePromptEvent::Confirm(name) = ev {
            match self.name_prompt_intent {
                Some(NamePromptIntent::SaveQuery) => {
                    if let Some(sql) = self.name_prompt_sql.clone() {
                        self.save_named_query(name, sql, cx);
                    }
                }
                Some(NamePromptIntent::SaveConsoleAsTable) => {
                    self.save_console_as_table(name, cx);
                }
                Some(NamePromptIntent::SaveViewAsTable) => {
                    self.save_view_as_table(name, cx);
                }
                None => {}
            }
        }
        self.name_prompt = None;
        self.name_prompt_sub = None;
        self.name_prompt_sql = None;
        self.name_prompt_intent = None;
        cx.notify();
    }

    /// Open the window-level saved-query picker overlay (P5b T8). The overlay is
    /// a flag-gated render of `render_saved_picker` over the live
    /// `session.saved_queries()`, so this just flips the flag.
    pub(crate) fn show_saved_picker(&mut self, cx: &mut Context<Self>) {
        self.saved_picker_open = true;
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Connections panel event handling (P5c T10/T11)
    // -----------------------------------------------------------------------

    /// Single routing point for the Connections panel's buttons
    /// ([`ConnectionsEvent`]). Runs the async MotherDuck connect/disconnect/forget
    /// flows (T8) and updates the [`ConnectionManager`] + persisted attachment set.
    ///
    /// The engine-touching connect/disconnect paths can only be compile-verified
    /// here (no MotherDuck token in this environment); CI/UAT exercise them later.
    ///
    /// [`ConnectionsEvent`]: crate::connections::panel::ConnectionsEvent
    /// [`ConnectionManager`]: crate::connections::ConnectionManager
    pub(crate) fn handle_connections_event(
        &mut self,
        ev: crate::connections::panel::ConnectionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::connections::ConnectionStatus;
        use crate::connections::connect::{Precheck, precheck};
        use crate::connections::panel::ConnectionsEvent;
        use crate::connections::token_store::KeychainTokenStore;

        match ev {
            // Connect (or Retry from an error state).
            ConnectionsEvent::ConnectMd => {
                let store = match KeychainTokenStore::new() {
                    Ok(s) => s,
                    Err(e) => {
                        self.connections
                            .set_md_status(ConnectionStatus::Error(e.to_string()));
                        cx.notify();
                        return;
                    }
                };
                match precheck(&store) {
                    Ok(Precheck::NeedToken) => self.open_md_token_prompt(window, cx),
                    Ok(Precheck::Ready(token)) => {
                        self.connections.set_md_status(ConnectionStatus::Connecting);
                        cx.notify();
                        self.spawn_md_connect(token, cx);
                    }
                    Err(e) => {
                        self.connections
                            .set_md_status(ConnectionStatus::Error(e.to_string()));
                        cx.notify();
                    }
                }
            }
            ConnectionsEvent::DisconnectMd => self.disconnect_md(cx),
            ConnectionsEvent::ForgetMd => {
                // Best-effort token forget, then disconnect.
                if let Ok(store) = KeychainTokenStore::new() {
                    use crate::connections::token_store::TokenStore as _;
                    let _ = store.forget();
                }
                self.disconnect_md(cx);
            }
            // TRIM-VALVE ②: the native file picker is not yet wired into this
            // codebase (files are loaded only via drag-and-drop). The
            // ConnectionManager `add_sqlite`/`remove_attachment` + the async
            // `engine().attach`/`detach` plumbing exist (Detach below uses them),
            // so wiring a picker here is the only remaining piece.
            // TODO P5c: wire native file picker (cx.prompt_for_paths) → attach the
            // chosen sqlite file via engine().attach("sqlite:<path>", alias, …),
            // then self.connections.add_sqlite(alias, path) + persist.
            ConnectionsEvent::AttachSqlite => {}
            ConnectionsEvent::Detach(alias) => self.detach_attachment(alias, cx),
        }
    }

    /// Disconnect MotherDuck: spawn the async detach, flip the manager to
    /// Disconnected, and drop the persisted md attachment. Shared by the
    /// Disconnect and Forget flows (P5c T11).
    fn disconnect_md(&mut self, cx: &mut Context<Self>) {
        use crate::connections::ConnectionStatus;
        let engine = self.engine();
        // Workspace mode has no single `md` alias — detach each attached MD db
        // by its real name. Capture the names BEFORE set_md_status clears them.
        let md_dbs = self.connections.md_databases().to_vec();
        tokio::spawn(async move {
            crate::connections::connect::run_disconnect(engine, md_dbs).await;
        });
        self.connections
            .set_md_status(ConnectionStatus::Disconnected);
        // Drop the persisted md attachment so a session recover does not re-attach.
        let mut sess = self.session.lock();
        let atts: Vec<crate::session::PersistedAttachment> = sess
            .attachments()
            .iter()
            .filter(|a| !matches!(a.kind, crate::session::PersistedAttachmentKind::Md))
            .cloned()
            .collect();
        let _ = sess.set_attachments(atts);
        drop(sess);
        cx.notify();
    }

    /// Detach a sqlite attachment by alias: spawn the async detach, remove it from
    /// the manager, and drop its persisted entry (P5c T11).
    fn detach_attachment(&mut self, alias: String, cx: &mut Context<Self>) {
        let engine = self.engine();
        let alias_for_engine = alias.clone();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let _ = engine.detach(&alias_for_engine).await;
        });
        self.connections.remove_attachment(&alias);
        let mut sess = self.session.lock();
        let atts: Vec<crate::session::PersistedAttachment> = sess
            .attachments()
            .iter()
            .filter(|a| a.alias != alias)
            .cloned()
            .collect();
        let _ = sess.set_attachments(atts);
        drop(sess);
        cx.notify();
    }

    /// Spawn the async MotherDuck connect (mirrors [`save_view_as_table`]'s
    /// engine bridge, P5c T11). Only `Send + 'static` values cross into
    /// `tokio::spawn` — the engine `Arc`, the owned `token` string, and the
    /// `Weak` shell handle. The GPUI entity is touched ONLY inside the dispatcher
    /// closure after `.upgrade()`. On a Connected result the md attachment is
    /// persisted so a session recover re-attaches it.
    ///
    /// The token is never logged: it is moved straight into `run_connect` (which
    /// itself never logs it) and dropped when the task ends.
    ///
    /// [`save_view_as_table`]: Self::save_view_as_table
    fn spawn_md_connect(&mut self, token: String, cx: &mut Context<Self>) {
        let engine = self.engine();
        let engine_for_list = engine.clone();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            let status = crate::connections::connect::run_connect(engine, token).await;
            let connected = matches!(status, crate::connections::ConnectionStatus::Connected);
            // On success, enumerate database names for the panel (design §4.3).
            let dbs = if connected {
                crate::connections::connect::list_databases(engine_for_list).await
            } else {
                Vec::new()
            };
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| {
                            // `set_md_status` clears md_databases when not
                            // Connected, so set the list AFTER it on success.
                            ws.connections.set_md_status(status.clone());
                            if connected {
                                ws.connections.set_md_databases(dbs.clone());
                                // Persist the md attachment (idempotent).
                                let mut sess = ws.session.lock();
                                let mut atts = sess.attachments().to_vec();
                                if !atts.iter().any(|a| {
                                    matches!(a.kind, crate::session::PersistedAttachmentKind::Md)
                                }) {
                                    atts.push(crate::session::PersistedAttachment {
                                        alias: crate::connections::MD_ALIAS.to_string(),
                                        kind: crate::session::PersistedAttachmentKind::Md,
                                    });
                                    let _ = sess.set_attachments(atts);
                                }
                                drop(sess);
                            }
                            cx.notify();
                        });
                    }
                });
            } else {
                tracing::warn!(
                    "spawn_md_connect: no MainThreadDispatcher installed; result dropped"
                );
            }
        });
    }

    /// On workspace load, if this session had MotherDuck attached, background-
    /// reconnect it (design §5). Non-md workspaces never touch the network: the
    /// early return guards on the persisted attachment set. The token comes from
    /// the keychain (never session.json); if it is gone, we leave the panel
    /// Disconnected so the user can reconnect manually.
    pub(crate) fn reconnect_persisted_md(&mut self, cx: &mut Context<Self>) {
        use crate::connections::ConnectionStatus;
        use crate::connections::connect::{Precheck, precheck};
        use crate::connections::token_store::KeychainTokenStore;
        let has_md = self
            .session
            .lock()
            .attachments()
            .iter()
            .any(|a| matches!(a.kind, crate::session::PersistedAttachmentKind::Md));
        if !has_md {
            return;
        }
        let Ok(store) = KeychainTokenStore::new() else {
            return;
        };
        if let Ok(Precheck::Ready(token)) = precheck(&store) {
            self.connections.set_md_status(ConnectionStatus::Connecting);
            cx.notify();
            self.spawn_md_connect(token, cx);
        }
        // NeedToken / errors: leave Disconnected (panel shows Connect).
    }

    /// Open the MotherDuck token-entry modal (reuses
    /// [`NamePrompt`](crate::view::name_prompt::NamePrompt), P5c T11). On Confirm
    /// the entered token is stored in the keychain, the prompt closes, the manager
    /// flips to Connecting, and the async connect spawns. On Cancel the prompt is
    /// just dismissed.
    ///
    /// Needs `&mut Window` because `NamePrompt::new` builds a single-line
    /// `InputState` eagerly. The subscription is stored in `md_token_prompt_sub`
    /// (a dropped `Subscription` deregisters the callback silently — the P4a T10b
    /// trap).
    fn open_md_token_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::view::name_prompt::{NamePrompt, NamePromptEvent};
        let prompt =
            cx.new(|cx| NamePrompt::new(dat0_i18n::t("connections.md.token_prompt"), window, cx));
        let sub = cx.subscribe_in(
            &prompt,
            window,
            |ws: &mut Self, _prompt, ev: &NamePromptEvent, _window, cx| match ev {
                NamePromptEvent::Confirm(token) => {
                    use crate::connections::ConnectionStatus;
                    use crate::connections::token_store::{KeychainTokenStore, TokenStore as _};
                    let token = token.clone();
                    // Close the prompt first.
                    ws.md_token_prompt = None;
                    ws.md_token_prompt_sub = None;
                    // Store the token; on failure surface an error and stop.
                    match KeychainTokenStore::new().and_then(|s| s.set(&token)) {
                        Ok(()) => {
                            ws.connections.set_md_status(ConnectionStatus::Connecting);
                            cx.notify();
                            ws.spawn_md_connect(token, cx);
                        }
                        Err(e) => {
                            ws.connections
                                .set_md_status(ConnectionStatus::Error(e.to_string()));
                            cx.notify();
                        }
                    }
                }
                NamePromptEvent::Cancel => {
                    ws.md_token_prompt = None;
                    ws.md_token_prompt_sub = None;
                    cx.notify();
                }
            },
        );
        self.md_token_prompt_sub = Some(sub);
        self.md_token_prompt = Some(prompt);
        cx.notify();
    }
}

// ---------------------------------------------------------------------------
// SQL console run-path support types (P5a T6)
// ---------------------------------------------------------------------------

/// The terminal state of one SQL console run, computed OFF the GPUI main thread
/// inside `spawn_sql_run` and applied on the main thread by `finish_sql_run`.
pub(crate) enum SqlRunOutcome {
    /// A result-producing statement bound to a fresh `GridDataSource`.
    Bound(std::sync::Arc<crate::grid::GridDataSource>),
    /// A DDL/DML statement completed; carries the status line.
    Status(String),
    /// The run failed; carries the DuckDB error message.
    Error(String),
    /// The run was interrupted (cooperative cancel).
    Cancelled,
}

/// Map a `dat0_engine::EngineError` onto a run outcome. The dedicated
/// `EngineError::Interrupted` variant (engine `execute/mod.rs` surfaces it when
/// `Engine::interrupt()` fires) maps to `Cancelled`; everything else is an
/// inline error.
fn classify_run_err(e: dat0_engine::EngineError) -> SqlRunOutcome {
    if matches!(e, dat0_engine::EngineError::Interrupted) {
        SqlRunOutcome::Cancelled
    } else {
        SqlRunOutcome::Error(e.to_string())
    }
}

/// Build the status line for a completed EXEC statement. DuckDB does not
/// uniformly expose an affected-row count through `QueryResult` here, so a
/// generic localized "OK" is used for P5a.
fn format_exec_status(_r: &dat0_engine::QueryResult) -> String {
    dat0_i18n::t("sql.ok")
}

/// Wall-clock millis since the Unix epoch (app runtime; not a workflow script).
fn now_unix_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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

        // Save-query name-prompt overlay (P5b T8). Mounted by `open_name_prompt`;
        // emits `NamePromptEvent` routed via the stored `name_prompt_sub`
        // subscription (Confirm → save + dismiss, Cancel → dismiss). Same
        // top-centre placement as the export dialog.
        let name_prompt_overlay: Option<gpui::AnyElement> = self.name_prompt.as_ref().map(|p| {
            div()
                .absolute()
                .top_16()
                .left_1_2()
                .child(p.clone())
                .into_any_element()
        });

        // MotherDuck token-entry overlay (P5c T11). Mounted by
        // `open_md_token_prompt`; emits `NamePromptEvent` routed via the stored
        // `md_token_prompt_sub` subscription (Confirm → store token + connect,
        // Cancel → dismiss). Same top-centre placement as the other modals.
        let md_token_prompt_overlay: Option<gpui::AnyElement> =
            self.md_token_prompt.as_ref().map(|p| {
                div()
                    .absolute()
                    .top_16()
                    .left_1_2()
                    .child(p.clone())
                    .into_any_element()
            });

        // Saved-query picker overlay (P5b T8). Window-level, flag-gated on
        // `saved_picker_open`; reads `session.saved_queries()` LIVE so a delete
        // refreshes the list on the next render. Picking a row routes the SQL
        // through the console's `queue_load` (the console's render drains it with
        // a real `Window` for `load_into_new_tab`) and closes the overlay.
        // Deleting calls `delete_named_query` and re-notifies so the list shrinks.
        // A trailing ✕ closes the overlay.
        let saved_picker_overlay: Option<gpui::AnyElement> = if self.saved_picker_open {
            let saved = self.session.lock().saved_queries().to_vec();
            let ws = cx.entity();
            let console = self.sql_console.clone();
            // Pick: route the SQL into a new tab via the console's `queue_load`
            // (windowless), then close the overlay.
            let on_pick = {
                let ws = ws.clone();
                move |sql: String, app: &mut gpui::App| {
                    if let Some(console) = console.clone() {
                        console.update(app, |c, cx| c.queue_load(sql, cx));
                    }
                    ws.update(app, |ws, cx| {
                        ws.saved_picker_open = false;
                        cx.notify();
                    });
                }
            };
            // Delete: remove from the session, then re-notify so the LIVE
            // `saved_queries()` read above re-runs next frame and the row drops.
            let on_delete = {
                let ws = ws.clone();
                move |id: uuid::Uuid, app: &mut gpui::App| {
                    ws.update(app, |ws, cx| {
                        ws.delete_named_query(id, cx);
                        cx.notify();
                    });
                }
            };
            let close = ws.clone();
            let picker = div()
                .absolute()
                .top_16()
                .right_2()
                .w(gpui::px(420.))
                .max_h(gpui::px(320.))
                .overflow_hidden()
                .border_1()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .child(dat0_i18n::t("sql.load_query"))
                        .child(
                            div()
                                .id("sql-saved-close")
                                .cursor_pointer()
                                .px_1()
                                .child("✕")
                                .on_click(move |_ev, _window, cx| {
                                    close.update(cx, |ws, cx| {
                                        ws.saved_picker_open = false;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                .child(crate::view::query_library::render_saved_picker(
                    &saved, on_pick, on_delete,
                ))
                .into_any_element();
            Some(picker)
        } else {
            None
        };

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
            // ── SQL Console actions (P5a T11) ─────────────────────────────────
            // View-scoped (not global `cx.on_action`) because these reach `self`
            // and three of them need a `&mut Window` (which the global App-level
            // dispatch path does NOT supply). gpui dispatches actions up the
            // focus/element tree, so `Cmd+Enter` / `Cmd+.` fired while the console
            // editor has focus still bubble here to the shell root.
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlRun, _window, cx| {
                    if let Some(c) = ws.sql_console.clone() {
                        ws.spawn_sql_run(c, crate::query::ResultTarget::MainGrid, cx);
                    }
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlCancel, _window, cx| {
                    ws.cancel_sql_run(cx);
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlConsoleToggle, window, cx| {
                    ws.toggle_sql_console(window, cx);
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlNewTab, window, cx| {
                    if let Some(c) = ws.sql_console.clone() {
                        c.update(cx, |c, cx| c.new_tab(window, cx));
                    }
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlCloseTab, _window, cx| {
                    if let Some(c) = ws.sql_console.clone() {
                        let active = c.read(cx).active;
                        c.update(cx, |c, cx| c.close_tab(active, cx));
                    }
                },
            ))
            // ── Connections panel toggle (P5c T11) ────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::ConnectionsToggle, _window, cx| {
                    ws.connections_panel_visible = !ws.connections_panel_visible;
                    cx.notify();
                },
            ))
            .on_key_down(key_handler)
            .on_click(click_to_focus)
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
            .on_drop::<ExternalPaths>(drop_listener)
            .children(tab_strip)
            .children(pipeline_bar)
            .children(sql_console_panel)
            // Body row: the Connections panel (left dock, when visible) + the
            // grid/console body (P5c T10/T11). When the panel is hidden this is
            // just the body in a flex_row — identical layout to before.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .children(self.connections_panel_visible.then(|| {
                        div().w_64().border_r_1().child(
                            crate::connections::panel::render_connections(&self.connections, cx),
                        )
                    }))
                    .child(div().flex_1().child(body)),
            )
            .children(popover_overlay)
            .children(editor_overlay)
            .children(export_overlay)
            .children(name_prompt_overlay)
            .children(saved_picker_overlay)
            .children(md_token_prompt_overlay)
    }
}
