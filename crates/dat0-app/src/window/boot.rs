//! GPUI application bootstrap: `run_app`, the menu action handlers, and
//! window spawning.
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

use super::*;

/// Resolve the `Arc<Mutex<Session>>` backing the currently-focused workspace
/// shell, if any (P8 T9 export). Mirrors the focused-shell resolution in
/// `promote_focused_into`.
pub(super) fn focused_session_arc(cx: &App) -> Option<Arc<Mutex<Session>>> {
    let weak = crate::window_registry::focused_workspace_weak()?;
    let any_entity = weak.upgrade()?;
    let shell = any_entity.downcast::<WorkspaceShell>().ok()?;
    Some(shell.read(cx).session_arc())
}

// ─── Live-data refresh (P7c) ──────────────────────────────────────────────

/// Parse macOS `application:openURLs:` file URLs into local paths. macOS
/// delivers opened files as percent-encoded `file://` URLs (e.g. `%20` for a
/// space), so decode via `url::Url::to_file_path` rather than a raw strip.
/// Non-file URLs (or unparseable entries) are skipped.
fn paths_from_open_urls(urls: &[String]) -> Vec<std::path::PathBuf> {
    urls.iter()
        .filter_map(|u| url::Url::parse(u).ok()?.to_file_path().ok())
        .collect()
}

// ── Chart toolbar axis-field plumbing (P9a T7) ──────────────────────────────
//
// Maps an `AxisRole` to the `ChartSpec` field that `build_plot_sql` reads for
// it. The mapping is NOT 1:1 — for BoxPlot the `Value` axis is carried in
// `spec.y`, and for Heatmap in `spec.color` (see charts/query.rs). The toolbar
// only shows the `Value` role for those two types, so `Value` always resolves
// to whichever field that type's SQL reads.

/// Open a new GPUI window for `session`, register it in `registry`, and
/// install the focused-workspace singleton. Extracted from `spawn_window` so
/// both the scratch path and the workspace path can share the same logic.
///
/// `workspace_path`: `Some(folder)` for workspace windows, `None` for scratch.
/// `read_only`: `true` for an Inspect (read-only package) window — sets the
/// shell's mutation gate so every edit/DDL entry point refuses (P8 T9).
pub(super) fn open_window_view(
    cx: &mut App,
    session: Arc<Mutex<Session>>,
    window_id: uuid::Uuid,
    workspace_path: Option<PathBuf>,
    registry: Arc<Mutex<WindowRegistry>>,
    read_only: bool,
) {
    let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
    let gpui_window = cx
        .open_window(
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
                    shell.read_only = read_only;
                    shell.reconnect_persisted_md(cx);
                    shell
                });
                crate::window_registry::install_focused_workspace(view.downgrade().into());
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("open window");

    registry.lock().register(WindowHandle {
        window_id,
        workspace_path,
        gpui_handle: Some(gpui_window.into()), // WindowHandle<Root> -> AnyWindowHandle
    });
    tracing::debug!(%window_id, "open_window_view: window registered");
}

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
        Ok(handle) => handle.block_on(Session::new(state_root, configured_memory_budget())),
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
    open_window_view(cx, session, window_id, None, registry, false);
    tracing::debug!(%window_id, "spawn_window: window registered in WindowRegistry");
}

/// Interim Help ▸ Documentation target until the P11b docs site ships; the
/// P11c launch-ops checklist swaps this const for the real docs URL.
const DOCS_URL: &str = "https://github.com/accidentally-awesome-labs/dat0#readme";

/// Interim Help ▸ Join Discord target — the server/invite is minted by the
/// P11c launch-ops checklist (P0 runbook §D3); swap this const then.
const DISCORD_URL: &str = "https://github.com/accidentally-awesome-labs/dat0";

/// Global (App-scoped) handlers + keybindings for every menu action that
/// needs no view state. Called once from `run_app` inside `Application::run`,
/// and by `tests/menu_reachability.rs`, which asserts that every
/// `MenuItem::action` in `menu_macos::build_menus` resolves to a registered
/// handler — macOS grays out menu items whose action has none (that is how
/// View ▸ Settings… shipped dead; 2026-07-21 UI-redesign master plan §4b).
///
/// View-scoped actions (SqlRun, the dock toggles, …) are deliberately NOT
/// here: they are handled on the `WorkspaceShell` root in `render` and
/// enable only while the shell has focus.
pub fn register_menu_action_handlers(cx: &mut App) {
    // Wire Cmd-N → NewWindow action. `cx.on_action` registers a global
    // handler called on the GPUI main thread whenever the action fires
    // (keyboard shortcut or menu item); `spawn_window` is synchronous and
    // safe to call from there. Reads the process-wide singletons at
    // dispatch time (the same pattern as the ActionRegistry `window.new`
    // descriptor) instead of capturing `run_app` locals, so this fn stays
    // parameterless for the menu-reachability test. Registered
    // unconditionally (was macOS-only): the action is declared
    // unconditionally in `menu_macos.rs`, and on Linux nothing fires it.
    cx.on_action(|_action: &crate::menu_macos::NewWindow, cx: &mut App| {
        tracing::info!("Cmd-N: spawning new window");
        let Some(state_root) = crate::window_registry::state_root() else {
            tracing::warn!("action: NewWindow — state_root singleton not installed; skipping");
            return;
        };
        let Some(registry) = crate::window_registry::window_registry() else {
            tracing::warn!("action: NewWindow — window_registry singleton not installed; skipping");
            return;
        };
        spawn_window(cx, state_root, registry);
    });

    // Cmd-Shift-P / Ctrl-Shift-P → OpenCommandPalette, plus the palette-scoped
    // arrows (B4). Moved into `command_palette::register_command_palette_keys`
    // so the TEST harness can call the identical function — the bindings used to
    // live only here, which meant no test binary had them and a keystroke test
    // would have passed vacuously (measured by the B4 T0 gate).
    //
    // `OpenCommandPalette` is declared unconditionally in `menu_macos.rs`, so
    // this binds on Linux too even though the Linux menu module does not exist:
    // the handler resolves and the keystroke fires without a visible menu item.
    crate::command_palette::register_command_palette_keys(cx);

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

    // Wire OpenWorkspace / SaveWorkspace → workspace flows (P7a T7-T9).
    // Both actions are declared in menu_macos.rs (unconditional), so the
    // handlers resolve on Linux too even without a visible menu item.
    cx.on_action(|_action: &crate::menu_macos::OpenWorkspace, cx: &mut App| {
        open_workspace_flow(cx);
    });
    cx.on_action(|_action: &crate::menu_macos::SaveWorkspace, cx: &mut App| {
        save_workspace_flow(cx);
    });

    // Wire Help → About → About box (P10a T5). Declared unconditionally in
    // menu_macos.rs so the handler resolves on Linux too.
    cx.on_action(|_action: &crate::menu_macos::ShowAbout, cx: &mut App| {
        crate::about::open(cx);
    });

    // Wire Help → Report a Bug → crash/bug-report dialog (P10c T8).
    // Declared unconditionally in menu_macos.rs so the handler resolves on
    // Linux too (no visible menu item there, but the action still dispatches).
    cx.on_action(|_action: &crate::menu_macos::ReportBug, cx: &mut App| {
        if let Ok(dir) = crate::platform::data_dir() {
            crate::view::crash_report::open_report(
                cx,
                crate::telemetry::report_logic::ReportKind::Bug,
                dir,
            );
        }
    });

    // Wire Help → Take a Tour → onboarding carousel (P11a T7).
    // Declared unconditionally in menu_macos.rs so the handler resolves on
    // Linux too (no visible menu item there, but the action still dispatches).
    // `open_deferred` (not `open`): this handler runs INSIDE a
    // `window.update` of the active window, where a synchronous
    // `onboarding::open` would re-enter that taken window and silently
    // no-op. The deferred hop runs the open from a plain App context after
    // the frame — same mechanism the auto-show uses.
    cx.on_action(|_a: &crate::menu_macos::TakeTour, cx: &mut App| {
        crate::onboarding::open_deferred(cx);
    });

    // Wire hero → Open demo.dat0 → editable workspace (P11a T9).
    // Declared unconditionally in menu_macos.rs; no menu item needed —
    // only the first-run hero band button triggers it.
    cx.on_action(|_a: &crate::menu_macos::OpenDemoWorkspace, cx: &mut App| {
        open_demo_workspace(cx);
    });

    // Wire Help → Check for Updates (P10a-2 T6). Declared unconditionally in
    // menu_macos.rs so the handler resolves on Linux too.
    cx.on_action(
        |_action: &crate::menu_macos::CheckForUpdates, cx: &mut App| {
            crate::update::ui::run_update_flow(cx, true);
        },
    );

    // Wire the .dat0 package actions (P8 T9). All declared unconditionally in
    // menu_macos.rs so the handlers resolve on Linux too (no visible menu).
    cx.on_action(|_action: &crate::menu_macos::ExportPackage, cx: &mut App| {
        export_package_flow(cx);
    });
    cx.on_action(|_action: &crate::menu_macos::OpenPackage, cx: &mut App| {
        open_package_flow(cx);
    });
    cx.on_action(|_action: &crate::menu_macos::UnpackPackage, cx: &mut App| {
        unpack_package_flow(cx);
    });
    cx.on_action(|_action: &crate::menu_macos::ReplayPackage, cx: &mut App| {
        replay_package_flow(cx);
    });

    // Wire File → Open Recent fan-out (P7a T10).
    //
    // Each OpenRecentN action maps to slot N in the filtered workspace-recents
    // list.  The helper reads the live recents store at invocation time so a
    // stale menu (e.g. the recents store changed between menu-rebuild and click)
    // is handled gracefully: if the index is now out of range the handler is a
    // no-op.  Cap is OPEN_RECENT_MENU_CAP=10; entries ≥10 are not in the menu.
    cx.on_action(|_: &crate::menu_macos::OpenRecent0, cx: &mut App| {
        open_recent_n(cx, 0);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent1, cx: &mut App| {
        open_recent_n(cx, 1);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent2, cx: &mut App| {
        open_recent_n(cx, 2);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent3, cx: &mut App| {
        open_recent_n(cx, 3);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent4, cx: &mut App| {
        open_recent_n(cx, 4);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent5, cx: &mut App| {
        open_recent_n(cx, 5);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent6, cx: &mut App| {
        open_recent_n(cx, 6);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent7, cx: &mut App| {
        open_recent_n(cx, 7);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent8, cx: &mut App| {
        open_recent_n(cx, 8);
    });
    cx.on_action(|_: &crate::menu_macos::OpenRecent9, cx: &mut App| {
        open_recent_n(cx, 9);
    });

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

    // Hotfix (2026-07-21, found by the UI-redesign A0 spike — master plan
    // §4b): `OpenSettings`, `OpenDocs` and `OpenDiscord` were declared in
    // `menu_macos.rs` and attached to menu items but had NO gpui handler —
    // macOS auto-enablement therefore grayed them out, and the Settings
    // window was unreachable in production (the ActionRegistry
    // `settings.open` descriptor's only consumer is the stub command
    // palette). `tests/menu_reachability.rs` is the regression gate.
    cx.on_action(|_action: &crate::menu_macos::OpenSettings, cx: &mut App| {
        crate::settings_ui::open_settings_window(cx);
    });
    cx.on_action(|_action: &crate::menu_macos::OpenDocs, _cx: &mut App| {
        if let Err(e) = crate::platform::open_url(DOCS_URL) {
            tracing::warn!(error = %e, "menu: open documentation failed");
        }
    });
    cx.on_action(|_action: &crate::menu_macos::OpenDiscord, _cx: &mut App| {
        if let Err(e) = crate::platform::open_url(DISCORD_URL) {
            tracing::warn!(error = %e, "menu: open discord failed");
        }
    });

    // Dead-menu-item follow-up (2026-07-22): Quit / Close Window / Minimize /
    // Zoom had no handlers since their menus were added — permanently grayed
    // (Cmd-Q included: the key equivalent hangs off the menu item, so a
    // disabled item swallows it; quitting only worked via the Dock). Standard
    // macOS chords are bound here; the handlers are unconditional (on Linux
    // nothing fires them — no menu bar and no bindings).
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-q", crate::menu_macos::Quit, None),
        gpui::KeyBinding::new("cmd-w", crate::menu_macos::CloseWindow, None),
        gpui::KeyBinding::new("cmd-m", crate::menu_macos::Minimize, None),
    ]);
    // File ▸ Open File… — global (not view-scoped) so the item is enabled on
    // a fresh boot before anything in the window has keyboard focus, matching
    // Open Workspace…. Routes through the focused workspace's picker (the
    // same handle_drop flow the hero button uses); no-op without a workspace.
    cx.on_action(|_action: &crate::menu_macos::OpenFile, cx: &mut App| {
        let Some(ws) = crate::window_registry::focused_workspace_weak()
            .and_then(|w| w.upgrade())
            .and_then(|e| e.downcast::<WorkspaceShell>().ok())
        else {
            return;
        };
        ws.update(cx, |ws, cx| ws.open_file_picker(cx));
    });

    cx.on_action(|_action: &crate::menu_macos::Quit, cx: &mut App| {
        // Best-effort SQL-console flush first (the per-mutation persists keep
        // disk current; this catches editor text typed since). The platform
        // terminate path does NOT run per-window `on_window_should_close`
        // hooks — same semantics as Dock-quit today.
        flush_focused_workspace_sql(cx);
        cx.quit();
    });
    cx.on_action(|_action: &crate::menu_macos::CloseWindow, cx: &mut App| {
        // `Window::remove_window` bypasses `on_window_should_close` (that hook
        // only fires from the OS close-button path), so run the same SQL
        // persist backstop it would have run before removing.
        flush_focused_workspace_sql(cx);
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_root, window, _cx| window.remove_window());
        }
    });
    cx.on_action(|_action: &crate::menu_macos::Minimize, cx: &mut App| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_root, window, _cx| window.minimize_window());
        }
    });
    cx.on_action(|_action: &crate::menu_macos::Zoom, cx: &mut App| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_root, window, _cx| window.zoom_window());
        }
    });
}

/// Best-effort flush of the focused workspace's SQL-console edit buffer AND its
/// dock layout to disk — the same persists the `on_window_should_close` backstop
/// runs on the OS close-button path. Used by the menu Quit / Close Window
/// handlers, whose paths (`platform.quit()` / `Window::remove_window`) never
/// fire that hook. No-op when no workspace is registered or the entity is gone.
///
/// ⚠ The layout flush is what captures a dock RESIZE (B9). Resizing is a pure
/// upstream mouse drag: it runs no dat0 code and emits no event — `Dock` is not
/// an `EventEmitter` at all — so a resize never followed by a dock toggle would
/// otherwise never reach disk. This is the only place that catches it.
fn flush_focused_workspace_sql(cx: &mut App) {
    let Some(ws) = crate::window_registry::focused_workspace_weak()
        .and_then(|w| w.upgrade())
        .and_then(|e| e.downcast::<WorkspaceShell>().ok())
    else {
        return;
    };
    ws.update(cx, |ws, cx| {
        ws.persist_sql_console(cx);
        ws.persist_dock_layout(cx);
        ws.persist_dock_layout_seed(cx);
    });
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
    let budget = configured_memory_budget();
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

    // P7a T9: the recents store singleton is installed from `main.rs` using the
    // canonical `AppContext.recents` instance (the same one the rest of the app
    // shares), so the workspace open/save flows push into the live store.

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

    // macOS (and Linux GUI via XDG) deliver double-clicked / "Open With dat0"
    // files through `application:openURLs:` (GPUI `on_open_urls`), NOT argv — so
    // on macOS this is the ONLY intake path for a `.dat0` double-click (Linux
    // also gets them via `Exec=dat0 %F` argv → `initial_paths`). Route them into
    // the same `handle_drop` flow the cold-start `initial_paths` block uses.
    // `on_open_urls` is on `Application` and must be registered before `run`.
    // (S1 spike.)
    // A5: the ONE AssetSource for the process. Without it every `Icon` renders as
    // nothing at all — gpui does not panic on an unresolved asset path (A0 spike).
    let application = Application::new().with_assets(crate::assets::Dat0Assets);
    let session_for_open = Arc::clone(&session);
    application.on_open_urls(move |urls: Vec<String>| {
        let paths = paths_from_open_urls(&urls);
        if paths.is_empty() {
            return;
        }
        let session = Arc::clone(&session_for_open);
        // The callback receives no `cx`, so re-enter the GPUI main thread with
        // `&mut App` via the process-wide dispatcher (the same hop the UDS
        // handler and `menu_macos::rebuild_menus_with_recents` use). Routing
        // through GPUI's `cx.spawn` (not a bare `tokio::spawn`) lets the window
        // observe the session mutation and refresh.
        let Some(d) = crate::window_registry::dispatcher() else {
            tracing::warn!("on_open_urls: dispatcher not installed; dropping open-files request");
            return;
        };
        let _ = d.dispatch(move |cx: &mut App| {
            let Some(handle) = cx.active_window() else {
                tracing::warn!("on_open_urls: no active window; dropping open-files request");
                return;
            };
            let _ = handle.update(cx, move |_root, _window, cx| {
                cx.spawn(async move |_async_cx| {
                    let outcomes = handle_drop(paths, session).await;
                    let n = outcomes
                        .iter()
                        .filter(|o| matches!(o, DropOutcome::Registered { .. }))
                        .count();
                    tracing::info!(n_registered = n, "macOS/XDG open-urls files processed");
                })
                .detach();
            });
        });
    });

    application.run(move |cx: &mut App| {
        // Required before opening any window: initialises the gpui-component
        // theme, global state, and (in debug builds) the inspector. Without
        // this, dialogs/sheets/notifications wired up in later tasks (T17)
        // will fail silently.
        gpui_component::init(cx);

        // Register the SqlConsole-scoped `escape` keybinding so a real Escape
        // keypress reaches the console's Escape ladder even when focus sits on a
        // non-Input transient-bar button (see `register_sql_console_keys`).
        crate::view::sql_console::register_sql_console_keys(cx);

        // Register the `Dat0Modal`-scoped tab/shift-tab/escape bindings (B1) so
        // the modal focus trap and modal-wide Escape are live in production.
        // Tests must call this too — see `overlay::register_modal_keys`.
        crate::overlay::register_modal_keys(cx);

        // B5: register the dock panels so `DockArea::load` can resolve them by
        // name (B9). Tests must call this too — see `panels::register_panels`.
        crate::panels::register_panels(cx);

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
            // global (`cx.global::<Theme>` panics otherwise). Installs the
            // built-in default via the same activate path `Theme::install`
            // uses.
            crate::theme::Theme::install_default(cx);
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
        // Global menu-action handlers + keybindings for every menu action
        // that needs no view state — extracted to
        // `register_menu_action_handlers` so `tests/menu_reachability.rs`
        // registers the exact production set. MUST run before `set_menus`:
        // menu items derive their displayed key equivalents (⌘Q, ⌘W, …) from
        // the keymap at install time.
        register_menu_action_handlers(cx);

        #[cfg(target_os = "macos")]
        {
            let menus = crate::menu_macos::build_menus(cx);
            cx.set_menus(menus);
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
            workspace_path: None,
            gpui_handle: Some(first_window.into()),
        });
        tracing::debug!(%first_window_id, "run_app: first window registered in WindowRegistry");

        // Boot recovery scan (P7c T7): emit ONE consolidated "Review" banner
        // (wired to `recovery.review`) covering BOTH recovery sources —
        // orphan scratch dirs under `$state_root/scratch/*` AND interrupted
        // workspace promotions among the user's recent workspace folders. The
        // recents come from the canonical singleton installed in main.rs (the
        // same `AppContext.recents` the rest of the app shares); workspace
        // entries only — a `Package` recent is not a `.dat0/` promotion.
        // Per spec §11 exit criterion #4 (extended for P7c workspace recovery).
        let scratch_root = state_root_for_action.join("scratch");
        let recent_roots: Vec<PathBuf> = crate::window_registry::recents()
            .and_then(|r| {
                r.lock().ok().map(|g| {
                    g.list()
                        .iter()
                        .filter_map(|e| match e {
                            crate::recents::RecentEntry::Workspace { path } => Some(path.clone()),
                            crate::recents::RecentEntry::Package { .. } => None,
                        })
                        .collect()
                })
            })
            .unwrap_or_default();
        let _emitted = recovery_scan_emit(&scratch_root, &recent_roots);

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

        // P10a-2 T6: launch-time update check.
        //
        // Gated on the persisted `Settings.update_auto_check` (default: true).
        // Reads the settings store synchronously here (same pattern as
        // `load_workspace_settings`), then fires the check off-thread so app
        // startup is never blocked on the network.  The `run_update_flow` call
        // itself just spawns a thread and returns immediately.
        {
            let auto_check = if let Ok(cfg_dir) = crate::platform::config_dir() {
                let store =
                    crate::settings::store::SettingsStore::with_path(cfg_dir.join("settings.toml"));
                store
                    .load_or_default()
                    .map(|s| s.update_auto_check)
                    .unwrap_or(true) // err → safe default: check
            } else {
                true // no config dir → safe default: check
            };
            if crate::update::ui::should_check_on_launch(auto_check) {
                tracing::debug!("run_app: firing background update check");
                crate::update::ui::run_update_flow(cx, false);
            } else {
                tracing::debug!("run_app: update_auto_check=false; skipping launch check");
            }
        }

        // P10c T8: relaunch crash-report prompt.
        //
        // Runs once at cold start, AFTER the first window is open and registered
        // in the WindowRegistry.  We defer via dispatcher so that when
        // `open_report` calls `cx.active_window()` the freshly-opened window
        // is already considered "active" by gpui (the direct-call path has the
        // window in the registry but macOS may not have assigned focus yet on
        // the same tick).
        //
        // Gating (verbatim from D-029 spec):
        //   1. opt_in=false         → discard staged data, NEVER prompt/transmit.
        //   2. prior crash detected AND opt_in AND staged payload present
        //                           → show Crash dialog with the payload.
        //   3. prior crash (marker only, no staged JSON, e.g. SIGKILL)
        //                           → discard bare marker; minimal-report is UAT-only.
        //
        // NOTE: `prior_crash_detected` returns true for EVERY cold start because
        // `CrashGuard::arm` sets the running.marker BEFORE `run_app` enters.
        // The REAL gate that prevents a dialog on clean exits is
        // `read_staged(&dir).is_some()` (clear-exit Drop clears the marker but
        // never the staged file — the staged file is only written by the panic hook).
        if let Some(d) = crate::window_registry::dispatcher() {
            let _ = d.dispatch(|cx: &mut App| {
                if let Ok(dir) = crate::platform::data_dir() {
                    // Read persisted setting once at startup (same pattern as
                    // the P10a-2 update check above and boot.rs init_logging).
                    // `unwrap_or(false)` = privacy-safe default (opt-out).
                    let opt_in = crate::settings::store::SettingsStore::with_path(
                        crate::platform::config_dir()
                            .map(|d| d.join("settings.toml"))
                            .unwrap_or_default(),
                    )
                    .load_or_default()
                    .map(|s| s.telemetry.crash_submission_enabled)
                    .unwrap_or(false);

                    // Gate composition lives in the pure `resolve_relaunch_action`
                    // seam (unit-tested in report_logic.rs, including the opt-out
                    // discard guarantee); this closure only reads `opt_in` above
                    // and dispatches the resulting action.
                    use crate::telemetry::report_logic::{RelaunchAction, ReportKind};
                    match crate::telemetry::report_logic::resolve_relaunch_action(&dir, opt_in) {
                        RelaunchAction::ShowCrash(staged) => {
                            tracing::info!(
                                "run_app: prior crash detected with staged payload; \
                                 opening crash report dialog"
                            );
                            crate::view::crash_report::open_report(
                                cx,
                                ReportKind::Crash(staged),
                                dir.clone(),
                            );
                        }
                        RelaunchAction::DiscardMarkerOnly => {
                            // Marker survived but no panic payload (SIGKILL /
                            // native crash).  Discard the bare marker; the
                            // minimal-report path is a v1.x / UAT-only feature.
                            tracing::debug!(
                                "run_app: prior crash marker present but no staged \
                                 payload (SIGKILL/native); clearing marker"
                            );
                            crate::telemetry::crash::clear_staged(&dir);
                        }
                        RelaunchAction::DiscardOptOut => {
                            // Opt-out: discard any staged data unconditionally.
                            // MUST NOT prompt or transmit anything.
                            crate::telemetry::crash::clear_staged(&dir);
                            tracing::debug!(
                                "run_app: crash_submission_enabled=false; staged data discarded"
                            );
                        }
                        RelaunchAction::Nothing => {}
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_urls_decode_to_local_paths() {
        // macOS `application:openURLs:` delivers percent-encoded `file://` URLs.
        // A plain path round-trips unchanged.
        assert_eq!(
            paths_from_open_urls(&["file:///tmp/a.dat0".into()]),
            vec![PathBuf::from("/tmp/a.dat0")]
        );
        // A percent-encoded space (`%20`) must decode back to a real space.
        assert_eq!(
            paths_from_open_urls(&["file:///tmp/My%20Data/b.dat0".into()]),
            vec![PathBuf::from("/tmp/My Data/b.dat0")]
        );
        // Non-file URLs and unparseable garbage are skipped (filtered out).
        assert!(paths_from_open_urls(&["https://example.com".into()]).is_empty());
        assert!(paths_from_open_urls(&["not a url".into()]).is_empty());
        // A mixed batch keeps only the decodable file URLs, in order.
        assert_eq!(
            paths_from_open_urls(&[
                "file:///tmp/one.dat0".into(),
                "https://example.com".into(),
                "file:///tmp/two.dat0".into(),
            ]),
            vec![
                PathBuf::from("/tmp/one.dat0"),
                PathBuf::from("/tmp/two.dat0")
            ]
        );
    }
}
