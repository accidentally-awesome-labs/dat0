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
use crate::workspace::Home;

// ─── Recents helper ──────────────────────────────────────────────────────────

/// Push a workspace path into the persisted recents store.
///
/// Silently no-ops if the recents singleton was not installed (e.g., in tests
/// that exercise sub-modules without booting the full app).
pub(crate) fn recents_push_workspace(path: &std::path::Path) {
    if let Some(recents) = crate::window_registry::recents() {
        if let Ok(mut guard) = recents.lock() {
            let _ = guard.push(crate::recents::RecentEntry::Workspace {
                path: path.to_path_buf(),
            });
        }
    }
}

// ─── Workspace open flow (T8) ─────────────────────────────────────────────

/// Global on_action handler for `OpenWorkspace` / `workspace.open`.
///
/// Shows the native folder-picker then delegates to [`open_workspace_at`].
pub(crate) fn open_workspace_flow(cx: &mut App) {
    let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let folder = match rx.await {
            Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
            _ => return,
        };
        let _ = cx.update(|cx| open_workspace_at(cx, folder));
    })
    .detach();
}

/// Load the workspace settings for the networked-path decision. On a genuine
/// settings error (config dir unavailable, or `settings.toml` fails to
/// parse/read — a *missing* file is fine, `load_or_default` handles it), err
/// toward networked-safe: design D2 says over-detection is free but
/// under-detection risks silent cross-machine corruption. Always logs the error.
fn load_workspace_settings() -> crate::settings::Workspace {
    let networked_safe = || crate::settings::Workspace {
        treat_all_as_networked: true,
        ..Default::default()
    };
    let Ok(dir) = crate::platform::config_dir() else {
        tracing::warn!(
            "config_dir unavailable; treating workspaces as networked (D2 safe default)"
        );
        return networked_safe();
    };
    let store = crate::settings::store::SettingsStore::with_path(dir.join("settings.toml"));
    match store.load_or_default() {
        Ok(s) => s.workspace,
        Err(e) => {
            tracing::warn!(
                ?e,
                "settings load failed; treating workspaces as networked (D2 safe default)"
            );
            networked_safe()
        }
    }
}

/// Per-window DuckDB memory budget from the persisted setting (1 GiB default on any error).
///
/// Reads `settings.toml` from the OS config dir on every call so new windows
/// pick up the latest value without requiring an app restart. Falls back to 1 GiB
/// on any error (config dir unavailable, missing file, or parse failure) — mirrors
/// the same config_dir path pattern used by `load_workspace_settings` above.
fn configured_memory_budget() -> u64 {
    let store = crate::settings::store::SettingsStore::with_path(
        crate::platform::config_dir()
            .expect("config dir")
            .join("settings.toml"),
    );
    crate::settings::budget::memory_budget_bytes(&store)
}

/// Validate `folder` is a `.dat0/` workspace and open a window for it.
///
/// Called by [`open_workspace_flow`] after the user picks a folder. Also
/// callable directly (e.g. from a "Recent workspaces" list). Shows a warning
/// banner if the folder is not a workspace or if a promote is incomplete.
/// If a window already has this path, logs and returns (bring-to-front lands
/// in P7b).
pub(crate) fn open_workspace_at(cx: &mut App, folder: PathBuf) {
    let folder = std::fs::canonicalize(&folder).unwrap_or(folder);
    let dat0 = Home::dat0_dir_for(&folder);
    if !dat0.exists() {
        crate::error_ux::push(crate::error_ux::Banner::warning(dat0_i18n::t(
            "workspace.open.not_a_workspace",
        )));
        return;
    }
    if crate::workspace::promote::detect_incomplete(&dat0) {
        crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
            dat0_i18n::t("workspace.open.incomplete.title"),
            dat0_i18n::t("workspace.open.incomplete.body"),
        ));
        return;
    }
    // Same-machine, in-process? (the common "two windows" case)
    let in_process = crate::window_registry::window_registry()
        .map(|reg| reg.lock().find_by_workspace(&folder).is_some())
        .unwrap_or(false);

    // Is this workspace on a sync drive?
    let settings = load_workspace_settings();
    let networked = crate::workspace::networked::is_networked(&folder, &settings);

    // Read the cross-machine record (networked only).
    let outcome = if networked {
        let lock_json = dat0.join("lock.json");
        crate::workspace::lock_manifest::acquire(
            &lock_json,
            &crate::workspace::identity::hostname(),
        )
        .unwrap_or(crate::workspace::lock_manifest::AcquireOutcome::Available)
    } else {
        crate::workspace::lock_manifest::AcquireOutcome::Available
    };

    use crate::workspace_in_use_modal::{ModalDecision, decide};
    match decide(&outcome, in_process) {
        ModalDecision::Proceed => open_workspace_proceed(cx, folder, networked),
        ModalDecision::SameMachineInUse => focus_existing_workspace(cx, &folder),
        ModalDecision::BlockSameMachine => {
            crate::error_ux::push(crate::error_ux::Banner::warning(dat0_i18n::t(
                "workspace.in_use.same_machine.title",
            )));
        }
        ModalDecision::ConflictForeign(holder) => {
            let folder2 = folder.clone();
            crate::workspace_in_use_modal::open_conflict_dialog(cx, holder, move |cx| {
                open_workspace_proceed(cx, folder2.clone(), true /* networked: claim ours */);
            });
        }
    }
}

/// Open + (if networked) claim the manifest, then spawn the window.
fn open_workspace_proceed(cx: &mut App, folder: PathBuf, networked: bool) {
    let guard = if networked {
        let lock_json = Home::dat0_dir_for(&folder).join("lock.json");
        match crate::workspace::lock_manifest::claim(&lock_json, now_epoch_secs()) {
            Ok(g) => Some(g),
            Err(e) => {
                // Read-only drive etc. — open anyway, degraded to flock-only.
                tracing::warn!(?e, "lock.json claim failed; opening flock-only");
                crate::error_ux::push(crate::error_ux::Banner::warning(dat0_i18n::t(
                    "workspace.in_use.claim_failed.title",
                )));
                None
            }
        }
    } else {
        None
    };
    spawn_workspace_window(cx, folder, guard);
}

/// Same-machine in-process: offer to focus the existing window. On confirm,
/// `bring_workspace_to_front` raises the registry-resolved window (P7b T8).
fn focus_existing_workspace(cx: &mut App, folder: &std::path::Path) {
    let folder = folder.to_path_buf();
    crate::workspace_in_use_modal::open_same_machine_dialog(cx, move |cx| {
        bring_workspace_to_front(cx, &folder);
    });
}

/// Bring the existing window backing `folder` to the foreground (P7b T8).
/// Resolves the target handle from the registry (NOT `active_window()`, which
/// is whatever is platform-focused) and raises it; also brings the app forward.
pub(crate) fn bring_workspace_to_front(cx: &mut App, folder: &std::path::Path) {
    cx.activate(true); // app to foreground (macOS) — already used at boot
    let Some(reg) = crate::window_registry::window_registry() else {
        return;
    };
    let handle = reg.lock().gpui_handle_by_workspace(folder);
    if let Some(handle) = handle {
        // T0 §7.3 confirmed: raise a specific window via its handle.
        let _ = handle.update(cx, |_root, window, _cx| {
            window.activate_window();
        });
    }
}

/// Open a new window backed by a `.dat0/` workspace at `folder`.
///
/// Recovers the session (acquire flock, reopen DB) then calls
/// [`open_window_view`] to create the GPUI window.
pub(crate) fn spawn_workspace_window(
    cx: &mut App,
    folder: PathBuf,
    manifest_guard: Option<crate::workspace::lock_manifest::LockManifestGuard>,
) {
    let registry = match crate::window_registry::window_registry() {
        Some(r) => r,
        None => {
            tracing::warn!("spawn_workspace_window: window_registry singleton not installed");
            return;
        }
    };
    let budget = configured_memory_budget();
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => {
            tracing::warn!("spawn_workspace_window: no tokio runtime on calling thread");
            return;
        }
    };
    let session = match rt.block_on(crate::session::Session::recover_workspace(
        folder.clone(),
        budget,
    )) {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                dat0_i18n::t("workspace.open.failed.title"),
                format!("{e}"),
            ));
            return;
        }
    };
    if let Some(g) = manifest_guard {
        session.lock().set_manifest_lock(g);
    }
    let window_id = session.lock().window_id;
    recents_push_workspace(&folder);
    // Refresh File → Open Recent so the just-opened workspace appears (the save
    // flow does the same after a promote — keep open/save symmetric).
    crate::menu_macos::rebuild_menus_with_recents();
    open_window_view(cx, session, window_id, Some(folder), registry, false);
}

// ─── Workspace save flow (T9) ─────────────────────────────────────────────

/// Global on_action handler for `SaveWorkspace` / `workspace.save`.
///
/// Shows the native folder-picker then delegates to [`promote_focused_into`].
pub(crate) fn save_workspace_flow(cx: &mut App) {
    let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let folder = match rx.await {
            Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
            _ => return,
        };
        let _ = cx.update(|cx| promote_focused_into(cx, folder));
    })
    .detach();
}

/// Promote the currently-focused scratch session into `target` folder.
///
/// Acquires the session from the focused workspace shell, calls
/// `promote_files` to move the DuckDB + session.json into `target/.dat0/`,
/// then calls `Session::adopt_workspace` to reopen from the new location.
fn promote_focused_into(cx: &mut App, target: PathBuf) {
    let Some(weak) = crate::window_registry::focused_workspace_weak() else {
        tracing::warn!("promote_focused_into: no focused workspace");
        return;
    };
    let Some(any_entity) = weak.upgrade() else {
        return;
    };
    let Ok(shell) = any_entity.downcast::<WorkspaceShell>() else {
        return;
    };
    let budget = configured_memory_budget();
    shell.update(cx, |shell, _cx| {
        let session = shell.session_arc();
        let mut guard = session.lock();
        if guard.is_workspace() {
            crate::error_ux::push(crate::error_ux::Banner::info(dat0_i18n::t(
                "workspace.save.already",
            )));
            return;
        }
        let scratch_dir = guard.home.root_dir().to_path_buf();
        let now = now_epoch_secs();
        let rt = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => {
                tracing::warn!("promote_focused_into: no tokio runtime");
                return;
            }
        };
        let result = rt.block_on(async {
            // `close` is a `QueryEngine` trait method (imported locally, matching
            // the other engine-call sites in this file).
            use dat0_engine::QueryEngine as _;
            // Close the engine before moving its file — matches the sequence the
            // T6 integration test proved lossless (close → promote_files →
            // adopt_workspace). `close()` only flags; the data-preserving flush
            // is the old engine's DROP inside `adopt_workspace` after the move.
            guard.engine.close().await.ok();
            let promoted = crate::workspace::promote::promote_files(&target, &scratch_dir, now)?;
            guard
                .adopt_workspace(promoted.root.clone(), promoted.lock, budget)
                .await?;
            std::fs::remove_dir_all(&promoted.old_scratch_dir).ok();
            anyhow::Ok(promoted.root)
        });
        match result {
            Ok(root) => {
                // If the destination is a sync drive, claim the cross-machine record.
                let settings = load_workspace_settings();
                if crate::workspace::networked::is_networked(&root, &settings) {
                    let lock_json = Home::dat0_dir_for(&root).join("lock.json");
                    match crate::workspace::lock_manifest::claim(&lock_json, now_epoch_secs()) {
                        Ok(g) => guard.set_manifest_lock(g),
                        Err(e) => tracing::warn!(
                            ?e,
                            "lock.json claim failed after save; workspace is flock-only"
                        ),
                    }
                }
                recents_push_workspace(&root);
                crate::menu_macos::rebuild_menus_with_recents();
                let mut b =
                    crate::error_ux::Banner::info(dat0_i18n::t("workspace.save.done.title"));
                b.body = root.display().to_string();
                crate::error_ux::push(b);
            }
            Err(e) => {
                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                    dat0_i18n::t("workspace.save.failed.title"),
                    format!("{e}"),
                ));
            }
        }
    });
}

// ─── .dat0 package flows (P8 T9) ──────────────────────────────────────────

/// Fixed engine memory budget for read-only package (`.dat0`) inspect/convert
/// operations — intentionally NOT driven by `Settings.memory_budget_mb` because
/// package ops are transient read-only tasks that don't need to follow the
/// user's live workspace preference. Workspace budget is now settings-driven
/// (via `configured_memory_budget()`); this constant documents the deliberate
/// divergence.
const PACKAGE_BUDGET: u64 = 1024 * 1024 * 1024;

/// `ExportPackage` handler: save the FOCUSED live workspace to a `.dat0` package.
///
/// Opens the native save panel (`*.dat0`), then off the GPUI main thread maps the
/// LIVE `Session` → `PackageContents` (preserving derived lineage) and writes the
/// package. Result surfaces through the `error_ux` banner queue.
pub(crate) fn export_package_flow(cx: &mut App) {
    // Resolve the focused workspace's live session Arc (the same precedent as
    // `promote_focused_into` / `dispatch_undo`).
    let session = match focused_session_arc(cx) {
        Some(s) => s,
        None => {
            crate::error_ux::push(crate::error_ux::Banner::warning(dat0_i18n::t(
                "package.export.no_workspace.title",
            )));
            return;
        }
    };
    let path_rx = cx.prompt_for_new_path(std::path::Path::new(""), Some("workspace.dat0"));
    cx.spawn(async move |_cx: &mut gpui::AsyncApp| {
        let out = match path_rx.await {
            Ok(Ok(Some(p))) => p,
            _ => return,
        };
        // Engine work off the GPUI main thread (tokio runtime is entered for the
        // whole Application::run closure — same as run_export). Snapshot the
        // session (engine Arc + portable views/queries + id) under a BRIEF lock,
        // then drop the guard BEFORE the async engine I/O — never hold the
        // parking_lot guard across an `.await` on the single-threaded foreground
        // executor (render also locks the session there).
        let (engine, window_id, views, queries, charts) = {
            let guard = session.lock();
            let engine = Arc::clone(&guard.engine);
            let window_id = guard.window_id;
            let views = dat0_format::Views {
                views: guard
                    .tabs()
                    .iter()
                    .map(|tab| dat0_format::PackageView {
                        table_name: tab.table_name.clone(),
                        transform_stack: tab.transform_stack.clone(),
                        undo_cursor: tab.undo_cursor,
                    })
                    .collect(),
            };
            let queries = dat0_format::Queries {
                queries: guard
                    .saved_queries()
                    .iter()
                    .map(|q| dat0_format::PackageQuery {
                        id: q.id,
                        name: q.name.clone(),
                        sql: q.sql.clone(),
                        saved_at: q.saved_at,
                    })
                    .collect(),
            };
            let charts = dat0_format::Charts {
                charts: guard
                    .charts()
                    .iter()
                    .map(|c| dat0_format::PackageChart {
                        id: c.id,
                        name: c.name.clone(),
                        spec: c.spec.clone(),
                        saved_at: c.saved_at,
                    })
                    .collect(),
            };
            (engine, window_id, views, queries, charts)
        };
        let result: anyhow::Result<()> = async {
            let contents = crate::package::contents_from_engine(
                engine.as_ref(),
                window_id,
                views,
                queries,
                charts,
            )
            .await?;
            dat0_format::Writer::write(&contents, engine.as_ref(), &out).await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                let mut b =
                    crate::error_ux::Banner::info(dat0_i18n::t("package.export.done.title"));
                b.body = out.display().to_string();
                crate::error_ux::push(b);
            }
            Err(e) => {
                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                    dat0_i18n::t("package.export.failed.title"),
                    format!("{e}"),
                ));
            }
        }
    })
    .detach();
}

/// `OpenPackage` handler: open a `.dat0` package read-only (Inspect mode).
///
/// Picks a `*.dat0` file, parses it, then opens an inspect engine
/// (`package::inspect::open_readonly` → `read_parquet` views) into a NEW window
/// whose shell has `read_only = true`. The inspect scratch dir lives under the
/// app state dir and is intentionally NOT cleaned — its parquet backs the views
/// for the window's lifetime.
pub(crate) fn open_package_flow(cx: &mut App) {
    let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let pkg = match rx.await {
            Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
            _ => return,
        };
        let _ = cx.update(|cx| open_package_at(cx, pkg));
    })
    .detach();
}

/// Parse `pkg` and mount a read-only Inspect window for it. Engine I/O runs via
/// `block_on` on the tokio runtime (mirrors `spawn_workspace_window`).
pub(crate) fn open_package_at(cx: &mut App, pkg: PathBuf) {
    let registry = match crate::window_registry::window_registry() {
        Some(r) => r,
        None => {
            tracing::warn!("open_package_at: window_registry singleton not installed");
            return;
        }
    };
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => {
            tracing::warn!("open_package_at: no tokio runtime on calling thread");
            return;
        }
    };

    // A unique, persistent scratch dir for this inspect window's extracted
    // parquet + view-backing DB (under the app state dir, NOT a TempDir — the
    // views read the parquet for the whole window lifetime).
    let base = crate::window_registry::state_root()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    let scratch_dir = base.join("inspect").join(uuid::Uuid::now_v7().to_string());

    let result: anyhow::Result<(Arc<Mutex<Session>>, uuid::Uuid)> = rt.block_on(async {
        let parsed = dat0_format::Reader::open(&pkg)?;
        std::fs::create_dir_all(&scratch_dir)?;
        let (engine, _views) =
            crate::package::inspect::open_readonly(&parsed, &scratch_dir, PACKAGE_BUDGET).await?;
        // Reconstruct the tabs + saved queries from the package's portable views.
        let tabs: Vec<crate::session::Tab> = parsed
            .views
            .views
            .iter()
            .map(|v| crate::session::Tab {
                table_name: v.table_name.clone(),
                source_path: None,
                transform_stack: v.transform_stack.clone(),
                undo_cursor: v.undo_cursor,
                extra: Default::default(),
            })
            .collect();
        let saved_queries = parsed
            .queries
            .queries
            .iter()
            .map(|q| crate::session::queries::SavedQuery {
                id: q.id,
                name: q.name.clone(),
                sql: q.sql.clone(),
                saved_at: q.saved_at,
            })
            .collect();
        let charts = parsed
            .charts
            .charts
            .iter()
            .map(|c| crate::session::charts::SavedChart {
                id: c.id,
                name: c.name.clone(),
                spec: c.spec.clone(),
                saved_at: c.saved_at,
            })
            .collect();
        let sess = Session::from_parts(
            scratch_dir.clone(),
            Arc::new(engine),
            tabs,
            saved_queries,
            charts,
        );
        let window_id = sess.window_id;
        Ok((Arc::new(Mutex::new(sess)), window_id))
    });

    match result {
        Ok((session, window_id)) => {
            // workspace_path: None — an Inspect window is not a workspace home
            // (no flock / no Open-Recent entry). read_only: true.
            open_window_view(cx, session, window_id, None, registry, true);
            tracing::debug!(%window_id, "open_package_at: inspect window opened");
        }
        Err(e) => {
            // Best-effort cleanup of the half-built scratch dir on failure.
            let _ = std::fs::remove_dir_all(&scratch_dir);
            crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                dat0_i18n::t("package.open.failed.title"),
                format!("{e}"),
            ));
        }
    }
}

/// `UnpackPackage` handler: unpack a `.dat0` package into an EDITABLE workspace.
///
/// Picks a `*.dat0` file, then a target directory, materializes a `.dat0/`
/// workspace there via `package::contents_to_workspace`, and opens it as a
/// normal (edit-enabled) workspace window.
pub(crate) fn unpack_package_flow(cx: &mut App) {
    // First pick the package file.
    let pkg_rx = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let pkg = match pkg_rx.await {
            Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
            _ => return,
        };
        // Then pick the destination directory.
        let dir_rx = cx.update(|cx| {
            cx.prompt_for_paths(gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: None,
            })
        });
        let dir_rx = match dir_rx {
            Ok(rx) => rx,
            Err(_) => return,
        };
        let dir = match dir_rx.await {
            Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
            _ => return,
        };
        let _ = cx.update(|cx| unpack_package_into(cx, pkg, dir));
    })
    .detach();
}

/// Unpack `pkg` into `dir` (materializing a `.dat0/` workspace) and open it as a
/// normal editable workspace. Engine I/O via `block_on` (mirrors
/// `spawn_workspace_window`).
fn unpack_package_into(cx: &mut App, pkg: PathBuf, dir: PathBuf) {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => {
            tracing::warn!("unpack_package_into: no tokio runtime on calling thread");
            return;
        }
    };
    let result: anyhow::Result<()> = rt.block_on(async {
        let parsed = dat0_format::Reader::open(&pkg)?;
        crate::package::contents_to_workspace(&parsed, &dir, PACKAGE_BUDGET).await?;
        Ok(())
    });
    match result {
        Ok(()) => {
            let mut b = crate::error_ux::Banner::info(dat0_i18n::t("package.unpack.done.title"));
            b.body = dir.display().to_string();
            crate::error_ux::push(b);
            // Open the freshly-materialized workspace as a normal editable window.
            open_workspace_at(cx, dir);
        }
        Err(e) => {
            crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                dat0_i18n::t("package.unpack.failed.title"),
                format!("{e}"),
            ));
        }
    }
}

/// `OpenDemoWorkspace` handler: unpack the bundled `demo.dat0` into a fresh
/// editable workspace and open it (P11a T9).
///
/// Flow:
/// 1. Write the `DEMO_DAT0` bundle bytes to `$state_root/demo.dat0` (a
///    staging file that is overwritten on each call — harmless).
/// 2. Choose a fresh dest dir `$state_root/demo/<uuid>/` (a new UUID per
///    invocation prevents collisions if the user opens the demo more than once).
/// 3. Call `unpack_package_into(cx, staging_pkg, dest_dir)`, which
///    materializes the workspace and then calls `open_workspace_at`.
///
/// All error paths surface via `error_ux::push` so the UI stays responsive.
pub(crate) fn open_demo_workspace(cx: &mut App) {
    let base = crate::window_registry::state_root()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(std::env::temp_dir);

    // Write bundled bytes to a staging file (overwrite is safe; same bytes).
    let staging_pkg = base.join("demo.dat0");
    if let Err(e) = std::fs::write(&staging_pkg, crate::sample_data::DEMO_DAT0) {
        crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
            dat0_i18n::t("package.open.failed.title"),
            format!("{e}"),
        ));
        return;
    }

    // Fresh dest dir: each click opens an independent editable workspace.
    let dest_dir = base.join("demo").join(uuid::Uuid::now_v7().to_string());
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
            dat0_i18n::t("package.open.failed.title"),
            format!("{e}"),
        ));
        return;
    }

    unpack_package_into(cx, staging_pkg, dest_dir);
}

/// `ReplayPackage` handler: re-run a `.dat0` recipe against fresh source files.
///
/// Picks the package, then ONE replacement source file, then the output `*.dat0`
/// path, then runs `cli::replay_async`. The source is bound to the package's
/// FIRST source's `logical_name` (a single-source convenience; multi-source
/// replay remains available through the `dat0 replay --source` CLI). Result
/// surfaces through the banner queue.
pub(crate) fn replay_package_flow(cx: &mut App) {
    let pkg_rx = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let pkg = match pkg_rx.await {
            Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
            _ => return,
        };
        // Resolve the package's first source logical name (so the picked file is
        // bound to a real source key). If the package has no sources, abort with
        // a banner — there is nothing to rebind.
        let logical = match dat0_format::Reader::open(&pkg) {
            Ok(p) => p.sources.sources.first().map(|s| s.logical_name.clone()),
            Err(e) => {
                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                    dat0_i18n::t("package.replay.failed.title"),
                    format!("{e}"),
                ));
                return;
            }
        };
        let Some(logical) = logical else {
            crate::error_ux::push(crate::error_ux::Banner::warning(dat0_i18n::t(
                "package.replay.failed.title",
            )));
            return;
        };

        // Pick the replacement source file.
        let src_rx = match cx.update(|cx| {
            cx.prompt_for_paths(gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            })
        }) {
            Ok(rx) => rx,
            Err(_) => return,
        };
        let new_source = match src_rx.await {
            Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
            _ => return,
        };

        // Pick the output package path.
        let out_rx = match cx
            .update(|cx| cx.prompt_for_new_path(std::path::Path::new(""), Some("replayed.dat0")))
        {
            Ok(rx) => rx,
            Err(_) => return,
        };
        let out = match out_rx.await {
            Ok(Ok(Some(p))) => p,
            _ => return,
        };

        // The replay spec for the CLI core: "logical=path".
        let spec = format!("{}={}", logical, new_source.display());
        match crate::cli::replay_async(&pkg, &[spec], Some(out.clone())).await {
            Ok(out_path) => {
                let mut b =
                    crate::error_ux::Banner::info(dat0_i18n::t("package.replay.done.title"));
                b.body = out_path.display().to_string();
                crate::error_ux::push(b);
            }
            Err(e) => {
                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                    dat0_i18n::t("package.replay.failed.title"),
                    format!("{e}"),
                ));
            }
        }
    })
    .detach();
}

/// Resolve the `Arc<Mutex<Session>>` backing the currently-focused workspace
/// shell, if any (P8 T9 export). Mirrors the focused-shell resolution in
/// `promote_focused_into`.
fn focused_session_arc(cx: &App) -> Option<Arc<Mutex<Session>>> {
    let weak = crate::window_registry::focused_workspace_weak()?;
    let any_entity = weak.upgrade()?;
    let shell = any_entity.downcast::<WorkspaceShell>().ok()?;
    Some(shell.read(cx).session_arc())
}

// ─── Live-data refresh (P7c) ──────────────────────────────────────────────

/// Reduce a `ViewModel::base_table()` name to its bare (unquoted, unqualified)
/// form for catalog matching (P7c). `base_table()` is quoted and may be
/// schema-qualified (`"main"."orders"`); the catalog keys on the bare name
/// (`orders`). Mirrors the reduction in [`WorkspaceShell::inspector_projection`].
fn bare_table_name(base: &str) -> String {
    base.rsplit('.')
        .next()
        .unwrap_or(base)
        .trim_matches('"')
        .to_string()
}

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

/// Read the spec field bound to `role`.
fn axis_field(
    spec: &crate::charts::spec::ChartSpec,
    role: crate::charts::spec::AxisRole,
) -> Option<&str> {
    use crate::charts::spec::{AxisRole, ChartType};
    match role {
        AxisRole::X => spec.x.as_deref(),
        AxisRole::Y => spec.y.as_deref(),
        AxisRole::Group => spec.group.as_deref(),
        AxisRole::Color => spec.color.as_deref(),
        // BoxPlot value → y; Heatmap value → color (per query.rs contract).
        AxisRole::Value => match spec.chart_type {
            ChartType::Heatmap => spec.color.as_deref(),
            _ => spec.y.as_deref(),
        },
    }
}

/// Write `val` into the spec field bound to `role`.
fn set_axis_field(
    spec: &mut crate::charts::spec::ChartSpec,
    role: crate::charts::spec::AxisRole,
    val: Option<String>,
) {
    use crate::charts::spec::{AxisRole, ChartType};
    match role {
        AxisRole::X => spec.x = val,
        AxisRole::Y => spec.y = val,
        AxisRole::Group => spec.group = val,
        AxisRole::Color => spec.color = val,
        AxisRole::Value => match spec.chart_type {
            ChartType::Heatmap => spec.color = val,
            _ => spec.y = val,
        },
    }
}

/// i18n key for an axis role's short label.
fn axis_role_key(role: crate::charts::spec::AxisRole) -> &'static str {
    use crate::charts::spec::AxisRole;
    match role {
        AxisRole::X => "chart.axis.x",
        AxisRole::Y => "chart.axis.y",
        AxisRole::Group => "chart.axis.group",
        AxisRole::Color => "chart.axis.color",
        AxisRole::Value => "chart.axis.value",
    }
}

/// Whether a role must always carry a column (X + the value axes) vs may be
/// cleared (Group / Color are optional dims that default to COUNT/none).
fn axis_required(role: crate::charts::spec::AxisRole) -> bool {
    use crate::charts::spec::AxisRole;
    matches!(role, AxisRole::X | AxisRole::Y | AxisRole::Value)
}

/// Advance an axis pick through `opts`. `required` axes cycle only over the
/// options (wrapping); optional axes additionally pass through `None` so the
/// user can clear a Group/Color dim. Picks not in `opts` (stale) reset to the
/// first option (or `None` for optional).
fn cycle_axis(current: Option<&str>, opts: &[String], required: bool) -> Option<String> {
    if opts.is_empty() {
        return None;
    }
    // Build the cycle order: [opt0, opt1, …] for required; [None, opt0, …] for
    // optional (None is index "before" the first option).
    let pos = current.and_then(|c| opts.iter().position(|o| o == c));
    match (required, pos) {
        // Required: just wrap over the options.
        (true, Some(i)) => Some(opts[(i + 1) % opts.len()].clone()),
        (true, None) => Some(opts[0].clone()),
        // Optional: order is None → opt0 → … → optN → None → …
        (false, None) => Some(opts[0].clone()),
        (false, Some(i)) if i + 1 < opts.len() => Some(opts[i + 1].clone()),
        (false, Some(_)) => None,
    }
}

/// Schema-drift pre-validate for live re-import replay (P7c D3).
///
/// The `replayable` ops are column-keyed. Only `Filter` and `Sort` reference
/// columns in the *executed* SQL (the projection ops `Reorder`/`Rename`/
/// `DeleteColumn` are display-only — they never reach `compile_view_sql`, so they
/// can't cause an engine error). If any executed-SQL op references a column that
/// the re-imported file no longer has, the replayed `SELECT` would fail at engine
/// execute time. `compile_view_sql` is a pure string renderer with no schema
/// knowledge, so it cannot detect this — we check column references against the
/// fresh column set here instead.
///
/// Returns `(ops, drifted)`: on drift, `ops` is empty (land on the bare base) and
/// `drifted` is `true` (caller raises the schema-drift banner). All-or-nothing —
/// a partial replay that silently dropped one filter would be more surprising
/// than landing cleanly on the bare base.
fn partition_replay_on_drift(
    replayable: Vec<dat0_engine::transform::Transformation>,
    columns: &[String],
) -> (Vec<dat0_engine::transform::Transformation>, bool) {
    use dat0_engine::transform::Transformation;
    let has = |c: &str| columns.iter().any(|known| known == c);
    let drifted = replayable.iter().any(|op| match op {
        Transformation::Filter { column, .. } => !has(column),
        Transformation::Sort { keys } => keys.iter().any(|k| !has(&k.column)),
        // Display-only projection ops never reach the executed SQL, so a stale
        // column reference in them is harmless (the grid `ColumnView` fold simply
        // ignores an unknown source). Don't treat them as drift.
        Transformation::Reorder { .. }
        | Transformation::Rename { .. }
        | Transformation::DeleteColumn { .. } => false,
        // Edit/RowDelete are never in `replayable` (split_replayable drops them).
        Transformation::Edit { .. } | Transformation::RowDelete { .. } => false,
    });
    if drifted {
        (Vec::new(), true)
    } else {
        (replayable, false)
    }
}

/// `live.refresh` action handler (P7c). Resolves the focused workspace and runs
/// its refresh flow.
///
/// Resolves the focused shell (same precedent as `dispatch_undo` /
/// `promote_focused_into`) and calls [`WorkspaceShell::run_refresh`], which runs
/// the real split → confirm → re-CTAS → replay flow (P7c T6).
pub fn dispatch_live_refresh(app: &mut gpui::App) {
    let Some(weak) = crate::window_registry::focused_workspace_weak() else {
        tracing::debug!("live.refresh: no focused workspace");
        return;
    };
    let Some(any_entity) = weak.upgrade() else {
        return;
    };
    let Ok(shell) = any_entity.downcast::<WorkspaceShell>() else {
        return;
    };
    shell.update(app, |shell, cx| shell.run_refresh(cx));
}

/// Open the Nth recent workspace (P7a T10).
///
/// `idx` is a 0-based index into the filtered list of `RecentEntry::Workspace`
/// entries (most recent first).  If the index is now out of range (recents
/// changed between menu-rebuild and click) this is a silent no-op.
pub(crate) fn open_recent_n(cx: &mut App, idx: usize) {
    let Some(recents_arc) = crate::window_registry::recents() else {
        return;
    };
    let Ok(guard) = recents_arc.lock() else {
        return;
    };
    let path = guard
        .list()
        .iter()
        .filter_map(|e| {
            if let crate::recents::RecentEntry::Workspace { path } = e {
                Some(path.clone())
            } else {
                None
            }
        })
        .nth(idx);
    drop(guard); // release lock before opening a window (which may block_on)
    if let Some(folder) = path {
        open_workspace_at(cx, folder);
    }
}

/// Epoch-seconds timestamp string used as the `now_rfc3339` argument to
/// `promote_files`. No time/chrono/jiff dep exists in dat0-app yet; an
/// integer seconds string is acceptable for P7a (the manifest timestamp is
/// informational, not load-bearing). T12 can upgrade to RFC3339 once a time
/// dep is added.
pub(crate) fn now_epoch_secs() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

// ─── Shared open-window helper ────────────────────────────────────────────

/// Open a new GPUI window for `session`, register it in `registry`, and
/// install the focused-workspace singleton. Extracted from `spawn_window` so
/// both the scratch path and the workspace path can share the same logic.
///
/// `workspace_path`: `Some(folder)` for workspace windows, `None` for scratch.
/// `read_only`: `true` for an Inspect (read-only package) window — sets the
/// shell's mutation gate so every edit/DDL entry point refuses (P8 T9).
fn open_window_view(
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

/// Open a new scratch window by RECOVERING an orphan scratch dir (P7c T9).
///
/// Used by the Recovery Sheet's "Open" row action. Mirrors [`spawn_window`]
/// but reuses [`Session::recover`] (which reads the orphan's `session.json`
/// and rebuilds its engine over the existing `scratch.duckdb`) instead of
/// `Session::new`, so the restored tabs come back live rather than empty.
/// On recovery failure pushes the standard open-failed banner and returns,
/// leaving the orphan row available for retry / discard.
pub(crate) fn spawn_recovered_scratch(cx: &mut App, scratch_dir: PathBuf) {
    let registry = match crate::window_registry::window_registry() {
        Some(r) => r,
        None => {
            tracing::warn!("spawn_recovered_scratch: window_registry singleton not installed");
            return;
        }
    };
    let budget = configured_memory_budget();
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => {
            tracing::warn!("spawn_recovered_scratch: no tokio runtime on calling thread");
            return;
        }
    };
    let session = match rt.block_on(Session::recover(scratch_dir, budget)) {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                dat0_i18n::t("workspace.open.failed.title"),
                format!("{e}"),
            ));
            return;
        }
    };
    let window_id = session.lock().window_id;
    open_window_view(cx, session, window_id, None, registry, false);
    tracing::debug!(%window_id, "spawn_recovered_scratch: orphan recovered into window");
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
    let count = count_orphan_scratch(scratch_root);
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

/// Boot-time recovery scan (P7c T7): consolidate BOTH recovery sources into a
/// single warning Banner with a `"Review"` primary action wired to
/// [`crate::actions::builtin::ids::RECOVERY_REVIEW`]:
///
/// 1. **Orphan scratch** — scratch subdirs containing a `session.json` (a
///    session that didn't exit cleanly), counted the same way as
///    [`orphan_scan_emit`].
/// 2. **Incomplete workspaces** — recent workspace folders whose `.dat0/` is a
///    half-finished promotion (missing `manifest.json` / `workspace.duckdb`),
///    found by [`crate::recovery_scan::scan_incomplete_workspaces`] over the
///    user's `Recents` (no full-filesystem scan).
///
/// The banner's count is `orphans + incompletes`. When there is nothing to
/// recover, no banner is emitted (returns `None`). Pushes the banner onto the
/// global pending queue (so first-render picks it up) and also returns it so
/// callers/tests can inspect without draining the queue.
///
/// This *replaces* `orphan_scan_emit` at the boot call site — kept as a sibling
/// (rather than widening `orphan_scan_emit`'s signature) so the existing
/// orphan-only tests keep working; both still build the same banner shape.
pub fn recovery_scan_emit(
    scratch_root: &std::path::Path,
    recent_roots: &[PathBuf],
) -> Option<crate::error_ux::Banner> {
    let count = count_orphan_scratch(scratch_root)
        + crate::recovery_scan::scan_incomplete_workspaces(recent_roots).len();

    if count == 0 {
        return None;
    }
    let title = dat0_i18n::t("recovery.banner.title").replace("{count}", &count.to_string());
    let banner =
        crate::error_ux::Banner::warning_with_body(title, dat0_i18n::t("recovery.banner.body"))
            .with_primary(
                dat0_i18n::t("recovery.banner.review"),
                crate::actions::builtin::ids::RECOVERY_REVIEW,
            );
    crate::error_ux::push(banner.clone());
    Some(banner)
}

/// Count orphan scratch dirs: scratch subdirs containing a `session.json` (a
/// session that didn't exit cleanly). Shared by [`orphan_scan_emit`] and
/// [`recovery_scan_emit`] so the two cannot drift on the orphan definition.
fn count_orphan_scratch(scratch_root: &std::path::Path) -> usize {
    let mut count = 0usize;
    if let Ok(entries) = std::fs::read_dir(scratch_root) {
        for e in entries.flatten() {
            if e.path().join("session.json").is_file() {
                count += 1;
            }
        }
    }
    count
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
    let application = Application::new();
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
    /// Save the currently-rendered chart under a user name (P9a-2). The
    /// generated default is seeded into the prompt by the opener
    /// (`open_chart_save_prompt`), so the confirm handler just reads the edited
    /// `name` — no per-intent state is captured here.
    SaveChart,
    /// NL prompt confirmed — `spawn_ai_nl2sql` gets the entered text as the NL
    /// prompt (P9c-2 T6).
    Nl2SqlPrompt,
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
    /// Stable per-hero-button focus handles, keyed by the button's static id.
    /// Created once and reused across renders (the transient `EmptyState` must NOT
    /// own these — it is rebuilt every frame).
    hero_focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
    /// Active-row index for keyboard nav of the Home-hero recents list. Held on
    /// the persistent shell because the transient `EmptyState` is rebuilt every
    /// frame; clamped to the recents length at render. Slice: recents-nav.
    /// `pub(crate)`: `empty_state::recents_column` (a sibling module) mutates
    /// this directly from its arrow-key `cx.listener` closure.
    pub(crate) recents_active: usize,
    /// Active-row index for keyboard nav of the Catalog panel (catalog-tree
    /// slice). Held on the persistent shell (the panel render is a free fn,
    /// rebuilt every frame); clamped to the visible-row count at each use.
    /// `pub(crate)`: `catalog::panel` (a sibling module) reaches it from
    /// `cx.listener` closures.
    pub(crate) catalog_active: usize,
    /// Collapsed attach-parent aliases in the Catalog panel (catalog-tree
    /// slice). Empty = all expanded. Mirrored to session v10
    /// `SessionUiState.catalog_collapsed` (restored in the ctor, written back
    /// by `persist_dock_ui` on every toggle).
    pub(crate) catalog_collapsed: std::collections::HashSet<String>,
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
    /// Whether the left-dock Catalog panel is shown (P6a T7). Toggled by the
    /// `CatalogToggle` action; gates the catalog dock in `render`.
    pub(crate) catalog_panel_visible: bool,
    /// Live catalog tree rendered by the Catalog dock (P6a T7). Rebuilt off-thread
    /// by [`Self::refresh_catalog`] whenever the catalog could change (toggle /
    /// import / create / drop).
    pub(crate) catalog_tree: crate::catalog::CatalogTree,
    /// Raw table list last fetched by [`Self::refresh_catalog`] (P6a T11).
    /// Stored so `recompute_lineage` can build the lineage graph without another
    /// engine round-trip. The `CatalogTree` discards origin/parent info, so we
    /// keep the full `Vec<TableInfo>` separately.
    pub(crate) catalog_tables: Vec<dat0_engine::TableInfo>,
    /// Sql-table → referenced base tables (lineage parents), resolved off-thread
    /// by the engine in `refresh_catalog`. Cached so `recompute_lineage` stays
    /// synchronous. Keyed by table name; only Sql-origin tables appear (P6b).
    pub(crate) sql_parents: std::collections::HashMap<String, Vec<String>>,
    /// Whether the right-dock Inspector panel is shown (P6a T9). Toggled by the
    /// `InspectorToggle` action; gates the inspector dock in `render`.
    pub(crate) inspector_panel_visible: bool,
    /// Whether the right-dock Charts panel is shown (P9a T7). Toggled by the
    /// `ChartVisualize` action; gates the chart dock in `render`.
    pub(crate) chart_panel_visible: bool,
    /// Live chart panel state (type + axis picks + last data/error). Bound to
    /// the active grid's base table when the panel opens (P9a T7).
    pub(crate) chart_panel: crate::charts::panel::ChartPanel,
    /// Last rendered chart image (BGRA → gpui `RenderImage`), refreshed by
    /// [`Self::run_plot_query`]. `None` until the first plot query returns.
    pub(crate) chart_image: Option<std::sync::Arc<gpui::RenderImage>>,
    /// Monotonic id incremented on every plot-query kickoff (P9a T7). A spawned
    /// plot result writes its image only if it carries the latest id, so a fast
    /// sequence of type/axis changes never lands a stale chart (mirrors the
    /// inspector's load-supersede guard).
    pub(crate) chart_load_id: u64,
    /// Monotonic id incremented on every Test-connection kickoff AND on every
    /// config-mutation that would change what a test means (provider switch, key
    /// change, model change, toggle flip). A spawned test result is only written
    /// back if it still carries the current id — mirrors `chart_load_id`.
    pub(crate) ai_test_load_id: u64,
    /// Monotonic id incremented on every NL→SQL stream kickoff (P9c-2 T6).
    /// Supersede guard: dispatched deltas only write if the id still matches.
    pub(crate) ai_stream_load_id: u64,
    /// Whether the left-dock AI panel is shown (P9c-1 T9). Toggled by the
    /// `AiPanelToggle` action; gates the AI dock in `render`.
    pub(crate) ai_panel_visible: bool,
    /// AI key/model entry modal (reuses [`NamePrompt`](crate::view::name_prompt::NamePrompt)).
    /// `Some` while the "Set API key…" / "Set model…" prompt is open; cleared on
    /// Confirm / Cancel. Rendered as a window overlay child in `render` (P9c-1 T9).
    pub(crate) ai_entry_prompt: Option<gpui::Entity<crate::view::name_prompt::NamePrompt>>,
    /// Subscription to the AI entry prompt's `NamePromptEvent`. Stored so the
    /// Confirm/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside `ai_entry_prompt`.
    pub(crate) ai_entry_prompt_sub: Option<gpui::Subscription>,
    /// Live AI-panel draft state (provider/model/toggles + key-set indicator +
    /// transient test-result). Loaded from `AiSettings` + a keychain key-presence
    /// probe when the panel opens (P9c-1 T9). The API KEY itself is never held
    /// here — only a "key is set" boolean.
    pub(crate) ai_panel: crate::ai::panel::AiPanel,
    /// Inspector state: profile target + (table,epoch)-keyed profile cache +
    /// load supersede (P6a T8). Profiles are loaded off-thread by
    /// [`Self::load_inspector_profile`].
    pub(crate) inspector: crate::inspector::InspectorModel,
    /// Per-window live banner list (PD-021). Drained from `error_ux::banner::PENDING`
    /// on each render; rendered as a host strip atop the shell.
    pub(crate) banners: Vec<crate::error_ux::banner::Banner>,
    /// Token-entry modal (reuses [`NamePrompt`](crate::view::name_prompt::NamePrompt)).
    /// `Some` while the MotherDuck token prompt is open; cleared on Confirm /
    /// Cancel. Rendered as a window overlay child in `render` when present.
    pub(crate) md_token_prompt: Option<gpui::Entity<crate::view::name_prompt::NamePrompt>>,
    /// Subscription to the token prompt's `NamePromptEvent`. Stored so the
    /// Confirm/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside
    /// `md_token_prompt`.
    pub(crate) md_token_prompt_sub: Option<gpui::Subscription>,
    /// Whether the "Save as workspace?" prompt has been shown this session
    /// (in-memory only — never persisted; shows at most once per launch).
    workspace_prompt_shown: bool,
    /// Whether the first-run tour has been auto-scheduled this per-window
    /// lifetime (in-memory only — never persisted). The persisted
    /// `first_run_done` flag is the authoritative gate across launches; this
    /// bool prevents the render-driven trigger from re-queuing the open on
    /// every subsequent frame before `first_run_done` flips to `true`.
    tour_auto_shown: bool,
    /// Live watcher over the active table's source file (P7c). Re-created
    /// whenever the active table changes (see [`Self::retarget_source_watch`]);
    /// `None` when the active table has no `File` origin. Dropping the field
    /// stops the watch.
    pub(crate) source_watcher: Option<crate::workspace::source_watcher::SourceWatcher>,
    /// When `true` this shell is open in Inspect mode (read-only package).
    /// Every data-mutation entry point (`commit_cell_edit`, `cut_selection`,
    /// `paste_clipboard`, `fill_down`, `set_null_selection`,
    /// `set_value_selection`, `delete_selected_rows`, `delete_column`,
    /// `commit_column_rename`, `save_view_as_table`, and the SQL-console DDL/DML
    /// path) checks this flag via [`crate::grid::edit_ops::mutation_blocked`]
    /// and returns early without executing when it is set. T9 sets this to `true`
    /// immediately after constructing the shell for an Inspect open; the default
    /// is `false` (normal edit-enabled workspace).
    pub(crate) read_only: bool,
}

impl WorkspaceShell {
    pub fn new(session: Arc<Mutex<Session>>, cx: &mut Context<Self>) -> Self {
        // Restore persisted catalog/inspector dock visibility (P6a T13, session
        // v8 `ui`). Read into a local BEFORE building the struct so we don't hold
        // the session lock across the whole ctor.
        let ui = session.lock().ui().clone();
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
            hero_focus: std::collections::HashMap::new(),
            recents_active: 0,
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
            catalog_panel_visible: ui.catalog_panel_visible,
            catalog_active: 0,
            catalog_collapsed: ui.catalog_collapsed.iter().cloned().collect(),
            catalog_tree: crate::catalog::CatalogTree::default(),
            catalog_tables: Vec::new(),
            sql_parents: Default::default(),
            inspector_panel_visible: ui.inspector_panel_visible,
            inspector: crate::inspector::InspectorModel::new(),
            ai_panel_visible: false,
            ai_panel: crate::ai::panel::AiPanel::default(),
            ai_entry_prompt: None,
            ai_entry_prompt_sub: None,
            chart_panel_visible: false,
            chart_panel: crate::charts::panel::ChartPanel::new(),
            chart_image: None,
            chart_load_id: 0,
            ai_test_load_id: 0,
            ai_stream_load_id: 0,
            banners: Vec::new(),
            md_token_prompt: None,
            md_token_prompt_sub: None,
            workspace_prompt_shown: false,
            tour_auto_shown: false,
            source_watcher: None,
            read_only: false,
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
        // PD-022: a rebind (undo/redo or SQL-console bind) may change the
        // inspected table's data; refresh its profile + lineage so the dock is
        // not stale. on_table_mutated_structural bumps the epoch, re-profiles,
        // and notifies; recompute_lineage rebuilds the chain.
        if let Some(target) = self.inspector.target_table.clone() {
            self.recompute_lineage();
            self.on_table_mutated_structural(&target, cx); // bumps epoch + reprofiles + notifies
        }
        cx.notify();
        self.maybe_prompt_save_workspace();
    }

    /// Surface the gentle "save as workspace" prompt once per session, after
    /// ≥3 applied transforms OR ≥1 saved query, while still scratch-backed
    /// (P7a T11). In-memory only — `workspace_prompt_shown` is never persisted.
    pub fn maybe_prompt_save_workspace(&mut self) {
        if self.workspace_prompt_shown {
            return;
        }
        // Never nudge "save as workspace" on a read-only Inspect window (P8 T9):
        // its `Home::Scratch` is the throwaway inspect-parquet dir, not a real
        // scratch session the user is building up.
        if self.read_only {
            return;
        }
        let (is_scratch, transforms, saved) = {
            let s = self.session.lock();
            (
                !s.is_workspace(),
                s.transform_count(),
                s.saved_queries().len(),
            )
        };
        if is_scratch && (transforms >= 3 || saved >= 1) {
            self.workspace_prompt_shown = true;
            let banner = crate::error_ux::Banner::info(dat0_i18n::t("workspace.prompt.title"))
                .with_primary(
                    dat0_i18n::t("workspace.prompt.save"),
                    crate::actions::builtin::ids::WORKSPACE_SAVE,
                );
            crate::error_ux::push(banner);
        }
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

    /// Return a clone of the `Arc<Mutex<Session>>` so workspace flows can
    /// promote the session without holding a borrow on `self`.
    pub fn session_arc(&self) -> Arc<Mutex<Session>> {
        Arc::clone(&self.session)
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

    /// The active grid tab's column projection, for the Inspector to mirror —
    /// but only when the Inspector is actually targeting that tab's table, so
    /// inspecting table X while the grid shows Y never mis-projects. `None`
    /// otherwise (no view/data source, or a cross-table target) → the Inspector
    /// falls back to its raw, unprojected card list. Identity is the bare
    /// (unquoted) table name, consistent with the app's catalog/lineage keying.
    pub(crate) fn inspector_projection(
        &self,
    ) -> Option<crate::inspector::projection::ProjectionContext> {
        let target = self.inspector.target_table.as_deref()?;
        let vm = self.view_model.as_ref()?;
        let ds = self.data_source.as_ref()?;
        // `base_table()` is quoted `"schema"."table"`; reduce to the bare name.
        let active = vm
            .base_table()
            .rsplit('.')
            .next()
            .unwrap_or("")
            .trim_matches('"');
        if active != target {
            return None;
        }
        Some(crate::inspector::projection::ProjectionContext {
            visible: self.column_view.clone(),
            base_sources: ds.visible_column_names(),
        })
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
            // Hydrate ai_ready on the freshly-built console.
            let ready = self.ai_ready();
            console.update(cx, |c, _cx| c.ai_ready = ready);
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
        // Keep the Catalog dock fresh if it's open (P6a T7).
        if self.catalog_panel_visible {
            self.refresh_catalog(cx);
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

    /// Open `name` into the main grid (P6a T7). The main window is single-view —
    /// one `view_model` at a time — so "open" mirrors the file-import load path
    /// (window.rs `last_registered` branch): build a `GridDataSource` off-thread,
    /// then on the main thread install a fresh `ViewModel` + data source.
    ///
    /// Bridge: a raw `tokio::spawn` + the `window_registry::dispatcher()` →
    /// upgraded weak handle, matching `refresh_completion_snapshot` (the import
    /// branch's `async_cx.update` bridge is only reachable from inside the render
    /// `cx.spawn`; this is an on-click handler, so the dispatcher bridge is the
    /// canonical off-thread→main-thread write here).
    pub(crate) fn open_table_tab(
        &mut self,
        name: String,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            match crate::grid::GridDataSource::new(engine, name.clone()).await {
                Ok(ds) => {
                    if let Some(dispatcher) = crate::window_registry::dispatcher() {
                        let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                            let Some(ws) = ws_weak.upgrade() else {
                                return;
                            };
                            ws.update(app_cx, |ws, cx| {
                                // base_table passed to ViewModel must already be quoted.
                                let quoted = format!("\"{}\"", name.replace('"', "\"\""));
                                ws.view_model =
                                    Some(crate::view::model::ViewModel::new(name.clone(), quoted));
                                ws.set_data_source(std::sync::Arc::new(ds));
                                // P6a T9: point the Inspector at the freshly-opened
                                // table and (lazily) load its profile.
                                ws.set_inspector_target(name.clone(), cx);
                                // P7c: re-target the live-data watch onto the
                                // newly-active table's source file. The catalog
                                // is already populated for an already-imported
                                // table being re-opened, so this resolves now;
                                // `refresh_catalog` retargets again as a backstop.
                                ws.retarget_source_watch(cx);
                                cx.notify();
                            });
                        });
                    } else {
                        tracing::warn!(
                            "open_table_tab: no MainThreadDispatcher installed; table not opened"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "open_table_tab: GridDataSource::new failed")
                }
            }
        });
    }

    /// Rebuild [`Self::catalog_tree`] from the engine's table list (P6a T7).
    /// Mirrors `refresh_completion_snapshot`: enumerate `get_tables()` off-thread,
    /// then write the freshly-built tree on the main thread via the dispatcher +
    /// upgraded weak handle. Called on every catalog-mutation point (toggle /
    /// import / create / drop / save-as-table).
    pub(crate) fn refresh_catalog(&mut self, cx: &mut gpui::Context<Self>) {
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::{DerivedOrigin, QueryEngine as _, TableOrigin};
            let tables = match engine.get_tables().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "refresh_catalog: get_tables failed");
                    return;
                }
            };
            // Resolve each Sql-origin table's lineage parents off-thread so
            // `recompute_lineage` (on the main thread) stays synchronous (P6b).
            let mut sql_parents = std::collections::HashMap::new();
            for t in &tables {
                if let TableOrigin::Derived(DerivedOrigin::Sql(sql)) = &t.origin {
                    if !sql.is_empty() {
                        match engine.referenced_tables(sql).await {
                            Ok(parents) => {
                                sql_parents.insert(t.name.clone(), parents);
                            }
                            Err(e) => tracing::warn!(error = %e, table = %t.name,
                                "refresh_catalog: referenced_tables failed; lineage edge skipped"),
                        }
                    }
                }
            }
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        ws.catalog_tree = crate::catalog::CatalogTree::build(&tables);
                        ws.catalog_tables = tables;
                        ws.sql_parents = sql_parents;
                        ws.recompute_lineage();
                        // P7c: now that `catalog_tables` is current, (re)target the
                        // live-data watch onto the active table's source file. This
                        // is the authoritative retarget point after a fresh import
                        // (the file-drop mount has stale catalog at mount time).
                        ws.retarget_source_watch(cx);
                        cx.notify();
                    });
                });
            } else {
                tracing::warn!("refresh_catalog: no MainThreadDispatcher installed; catalog stale");
            }
        });
    }

    /// Apply one keyboard-nav key to the Catalog panel: flatten the current
    /// tree, clamp the active index (SINGLE clamp site — ring, arrows and
    /// Enter all use this same index), then act on the pure `tree_nav`
    /// transition. Both the container's `focus_stop` activate (enter/space)
    /// and the chained arrow handler route here (single source of truth).
    pub(crate) fn catalog_nav_key(
        &mut self,
        key: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let rows = crate::catalog::nav::visible_rows(&self.catalog_tree, &self.catalog_collapsed);
        if rows.is_empty() {
            return;
        }
        let active = self.catalog_active.min(rows.len() - 1);
        self.catalog_active = active;
        match crate::catalog::nav::tree_nav(&rows, active, key) {
            crate::catalog::nav::NavAction::Move(i) => {
                self.catalog_active = i;
                cx.notify();
            }
            crate::catalog::nav::NavAction::Toggle(alias) => {
                self.toggle_catalog_parent(alias, cx);
            }
            crate::catalog::nav::NavAction::Open(name) => {
                self.open_table_tab(name, window, cx);
            }
            crate::catalog::nav::NavAction::None => {}
        }
    }

    /// Flip an attach parent's expand/collapse state. Single source of truth —
    /// the parent row's mouse `on_click` AND the keyboard Toggle arm both call
    /// this (mouse and keyboard cannot drift). Clamps the active index against
    /// the post-toggle row count so a collapse can never dangle the ring.
    pub(crate) fn toggle_catalog_parent(&mut self, alias: String, cx: &mut gpui::Context<Self>) {
        if !self.catalog_collapsed.remove(&alias) {
            self.catalog_collapsed.insert(alias);
        }
        let rows = crate::catalog::nav::visible_rows(&self.catalog_tree, &self.catalog_collapsed);
        self.catalog_active = self.catalog_active.min(rows.len().saturating_sub(1));
        self.persist_dock_ui();
        cx.notify();
    }

    /// Point the Inspector at `name` and load its profile (P6a T9). If the
    /// (table,epoch) profile is already cached the load is skipped (warm hit);
    /// otherwise [`Self::load_inspector_profile`] fetches it off-thread.
    ///
    /// Takes no `Window` — none of the inspector methods need one, which lets
    /// `open_table_tab`'s dispatcher closure (which has no `window`) call this.
    pub(crate) fn set_inspector_target(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        self.inspector.set_target(name);
        self.recompute_lineage();
        if self.inspector.cached().is_some() {
            // Warm hit — nothing to load; just repaint the dock.
            cx.notify();
            return;
        }
        self.load_inspector_profile(cx);
        cx.notify();
    }

    /// Rebuild the Inspector's lineage chain for the current target from the
    /// cached `catalog_tables` + `sql_parents` (P6b). Called on every catalog
    /// refresh, on `set_inspector_target`, and on rebind (PD-022). Takes no `cx`;
    /// the caller is responsible for `cx.notify()` afterward.
    pub(crate) fn recompute_lineage(&mut self) {
        if let Some(target) = self.inspector.target_table.clone() {
            // P9a-2: saved charts join the lineage as descendants of their source
            // table. Each chart's `spec.source` is a quoted (possibly qualified)
            // identifier; reduce it to the bare catalog key the lineage matches on.
            let chart_nodes: Vec<crate::inspector::lineage::ChartNode> = {
                let sess = self.session.lock();
                sess.charts()
                    .iter()
                    .map(|c| crate::inspector::lineage::ChartNode {
                        name: c.name.clone(),
                        source_table: bare_table_name(&c.spec.source),
                    })
                    .collect()
            };
            let graph = crate::inspector::lineage::LineageGraph::build(
                &self.catalog_tables,
                &self.sql_parents,
                &chart_nodes,
            );
            self.inspector.set_lineage(graph.closure(&target));
        }
    }

    /// A mutation touched `table`'s data/schema. Invalidate the inspector's cached
    /// profile for it (epoch bump) and, if that table is the live inspector target,
    /// re-profile it now so the open dock updates. (Hybrid write path, P6a T12.)
    pub(crate) fn on_table_mutated_structural(
        &mut self,
        table: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        self.inspector.bump_epoch(table);
        if self.inspector.target_table.as_deref() == Some(table) {
            self.load_inspector_profile(cx); // re-SUMMARIZE the now-invalidated table
        }
        cx.notify();
    }

    /// The on-disk source path of the currently-mounted table, if it was
    /// imported from a file (P7c). Matches the active `view_model`'s base table
    /// against `catalog_tables` on the bare (unquoted) name — the catalog/lineage
    /// keying used elsewhere (see [`Self::inspector_projection`]). Returns `None`
    /// when no table is mounted, the table has no `File` origin, or the catalog
    /// has not yet been refreshed for it.
    pub(crate) fn active_source_path(&self) -> Option<std::path::PathBuf> {
        let base = self.view_model.as_ref()?.base_table();
        let bare = bare_table_name(base);
        self.catalog_tables.iter().find_map(|t| match &t.origin {
            dat0_engine::TableOrigin::File(p) if t.name == bare => Some(p.clone()),
            _ => None,
        })
    }

    /// (Re)create the source watcher for the active table (P7c). Drops any
    /// previous watch first (so switching tables retargets, not stacks). No-op
    /// (clears the watch) when the active table has no `File` source or the
    /// source file no longer exists on disk.
    pub(crate) fn retarget_source_watch(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_source_path() else {
            self.source_watcher = None;
            return;
        };
        if !path.exists() {
            self.source_watcher = None; // file gone → nothing to watch
            return;
        }
        let ws = cx.entity().downgrade();
        let watcher = crate::workspace::source_watcher::SourceWatcher::start(
            path.clone(),
            std::time::Duration::from_millis(500),
            move |changed| {
                // Runs on the `dat0-source-debounce` bg thread — must NOT touch
                // GPUI directly. Hop to the main thread via the dispatcher, then
                // upgrade the weak shell handle and raise the banner.
                if let Some(d) = crate::window_registry::dispatcher() {
                    let ws = ws.clone();
                    let _ = d.dispatch(move |app| {
                        if let Some(h) = ws.upgrade() {
                            h.update(app, |shell, cx| shell.on_source_changed(changed, cx));
                        }
                    });
                }
            },
        );
        match watcher {
            Ok(w) => self.source_watcher = Some(w),
            Err(e) => tracing::warn!(error = %e, "failed to start source watcher"),
        }
    }

    /// Raise a one-click Refresh banner for an externally-changed source file
    /// (P7c). De-dups: if a refresh banner for this file's title is already
    /// present, do nothing (a burst of saves coalesces to a single banner —
    /// the watcher already debounces, this guards re-raise across drains).
    pub(crate) fn on_source_changed(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let file = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let title = dat0_i18n::t("livedata.changed.title").replace("{file}", &file);
        let already = self.banners.iter().any(|b| b.title == title);
        if already {
            return;
        }
        self.banners.push(
            crate::error_ux::banner::Banner::warning(title).with_primary(
                dat0_i18n::t("livedata.changed.refresh"),
                crate::actions::builtin::ids::LIVE_REFRESH,
            ),
        );
        cx.notify();
    }

    /// Re-import the active table's source file and replay its structural
    /// transforms (P7c D3). If the active stack carries rowid-keyed edits
    /// (`Edit`/`RowDelete`), confirm-discard first via a blocking Dialog — those
    /// ops can't survive a re-CTAS (the `__dat0_rowid` surrogate regenerates).
    /// A pure (filter/sort/projection)-only stack proceeds without a prompt.
    ///
    /// On success the refresh banner for this file is cleared and the watch is
    /// re-targeted (an atomic save may have replaced the inode).
    pub(crate) fn run_refresh(&mut self, cx: &mut Context<Self>) {
        use dat0_engine::transform::split_replayable;
        // Resolve the active table's source up front so a no-source tab (e.g. a
        // SQL-derived table) is a clean no-op rather than half-running.
        if self.active_source_path().is_none() {
            return;
        }
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        let split = split_replayable(vm.stack());
        if split.has_dropped() {
            let body = dat0_i18n::t("livedata.refresh.confirm.body")
                .replace("{edits}", &split.dropped_edits.to_string())
                .replace("{deletes}", &split.dropped_deletes.to_string());
            let ws = cx.entity().downgrade();
            crate::live_refresh_dialog::confirm_discard(cx, body, move |app| {
                if let Some(h) = ws.upgrade() {
                    h.update(app, |shell, cx| shell.perform_reimport(cx));
                }
            });
        } else {
            self.perform_reimport(cx);
        }
    }

    /// Off-thread re-CTAS of the active table's source file, then on-main replay
    /// of the structural stack (P7c). The re-import is idempotent — DuckDB
    /// `CREATE OR REPLACE TABLE` under the same derived name (see
    /// [`dat0_engine::QueryEngine::register_file_as_table`]) — so it overwrites
    /// the base in place and re-injects `__dat0_rowid`. Mirrors the file-drop
    /// import path's spawn discipline (`tokio::spawn` + `window_registry`
    /// dispatcher + upgraded weak shell handle).
    pub(crate) fn perform_reimport(&mut self, cx: &mut Context<Self>) {
        use dat0_engine::transform::split_replayable;
        let Some(path) = self.active_source_path() else {
            return;
        };
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        let replayable = split_replayable(vm.stack()).replayable;
        let engine = self.engine();
        let ws = cx.entity().downgrade();
        let file = path.file_name().map(|s| s.to_string_lossy().to_string());

        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let opts = dat0_engine::RegisterOpts::default();
            let result = engine.register_file_as_table(&path, opts).await;
            if let Some(d) = crate::window_registry::dispatcher() {
                let _ = d.dispatch(move |app| {
                    let Some(h) = ws.upgrade() else {
                        return;
                    };
                    h.update(app, |shell, cx| match result {
                        Ok(info) => {
                            let columns: Vec<String> =
                                info.columns.iter().map(|c| c.name.clone()).collect();
                            shell.apply_refresh_replay(replayable, columns, file, cx);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "live re-import failed");
                            shell.banners.push(crate::error_ux::Banner::error(
                                dat0_i18n::t("livedata.reimport.failed.title"),
                                format!("{e}"),
                            ));
                            cx.notify();
                        }
                    });
                });
            } else {
                tracing::warn!(
                    "perform_reimport: no MainThreadDispatcher installed; re-import result dropped"
                );
            }
        });
    }

    /// On the main thread: replay the structural ops onto the freshly re-imported
    /// base via the force-rebind primitive ([`ViewModel::reset_to_replayed`]),
    /// drive the engine round-trip, and clear the refresh banner (P7c).
    ///
    /// **Schema-drift guard (D3):** the replayable ops are column-keyed. If a
    /// `Filter`/`Sort` op references a column that the re-imported file no longer
    /// has (a rename or drop upstream), the replayed `SELECT` would fail at engine
    /// execute time — `spawn_view_change` only logs that failure, leaving the grid
    /// silently un-rebound. So we pre-validate every column reference against the
    /// fresh column set BEFORE `reset_to_replayed`; on any miss we land on the
    /// bare base (no transforms) plus a schema-drift warning banner. (Note:
    /// `compile_view_sql` is a pure string renderer and does NOT know the schema,
    /// so it can't be the guard — the column-set check is the real validation.)
    pub(crate) fn apply_refresh_replay(
        &mut self,
        replayable: Vec<dat0_engine::transform::Transformation>,
        columns: Vec<String>,
        file: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let (replayable, drifted) = partition_replay_on_drift(replayable, &columns);
        if drifted {
            tracing::warn!("live replay schema drift; landing on bare base");
            self.banners
                .push(crate::error_ux::Banner::warning(dat0_i18n::t(
                    "livedata.replay.schema_drift",
                )));
        }

        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.reset_to_replayed(replayable);
        let base = vm.base_table().to_string();
        let engine = self.engine();
        let ws = cx.entity().downgrade();
        crate::view::spawn_view_change(
            engine,
            base,
            change,
            std::sync::Arc::new(move |new_ds, app_cx| {
                if let Some(h) = ws.upgrade() {
                    h.update(app_cx, |shell, cx| shell.apply_view_change(new_ds, cx));
                }
            }),
        );

        // Drop the refresh banner(s) this file raised — the click is resolved.
        if let Some(file) = file {
            let title = dat0_i18n::t("livedata.changed.title").replace("{file}", &file);
            self.banners.retain(|b| b.title != title);
        }
        // An atomic save (write-temp + rename) replaces the watched inode, so the
        // old watch is now dead — re-target it at the live path.
        self.retarget_source_watch(cx);
        cx.notify();
    }

    /// Load the profile for the current inspector target off-thread, then write
    /// it back on the main thread under the supersede guard (P6a T9). Mirrors
    /// [`Self::open_table_tab`] / [`Self::refresh_catalog`]: `tokio::spawn` +
    /// `window_registry::dispatcher()` + upgraded weak handle.
    ///
    /// In [`ProfileTargetMode::CurrentView`] the active view's `SELECT` is
    /// compiled off the live `view_model` and profiled via `profile_query`;
    /// otherwise the stored table is profiled via `profile_table`.
    pub(crate) fn load_inspector_profile(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(target) = self.inspector.target_table.clone() else {
            return;
        };
        let mode = self.inspector.mode;
        let load_id = self.inspector.begin_load();
        let engine = self.engine();
        // For CurrentView mode, compile the active view's SELECT off the view_model.
        let view_sql: Option<String> =
            if matches!(mode, crate::inspector::ProfileTargetMode::CurrentView) {
                self.view_model
                    .as_ref()
                    .and_then(|vm| dat0_engine::compile_view_sql(vm.base_table(), vm.active()).ok())
            } else {
                None
            };
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let result = match view_sql {
                Some(sql) => engine.profile_query(&sql).await,
                None => engine.profile_table(&target, None).await,
            };
            let profile = match result {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "inspector profile load failed");
                    return;
                }
            };
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        // Supersede guard: only the latest load writes its result.
                        if ws.inspector.is_current(load_id) {
                            ws.inspector.put(profile.clone());
                            cx.notify();
                            // Fetch inline-chart extras for the columns that
                            // qualify, reusing this load's `load_id` to guard
                            // against stale bars when tables switch fast (T10).
                            ws.load_column_extras(load_id, profile, cx);
                        }
                    });
                });
            } else {
                tracing::warn!(
                    "load_inspector_profile: no MainThreadDispatcher installed; profile dropped"
                );
            }
        });
    }

    /// Fetch inline-chart extras for a freshly-loaded profile (P6a T10), reusing
    /// the profile load's `load_id` as the supersede guard so switching tables
    /// fast never lands stale bars. Eager-on-load (vs lazy-on-expand): acceptable
    /// because the *qualifying* set is small — only low-cardinality columns get a
    /// `column_topn` fetch and only numeric high-cardinality columns get a
    /// sampled histogram; everything else issues no query. This naturally bounds
    /// the query count for typical tables.
    ///
    /// Extras are only fetched in `WholeTable` mode: they query the base table by
    /// its bare name, so they match the profiled data only when the profile is of
    /// the whole table (in `CurrentView` the profile is of a filtered SELECT).
    fn load_column_extras(
        &mut self,
        load_id: u64,
        profile: dat0_engine::TableProfile,
        cx: &mut gpui::Context<Self>,
    ) {
        if !matches!(
            self.inspector.mode,
            crate::inspector::ProfileTargetMode::WholeTable
        ) {
            return;
        }
        let Some(table) = self.inspector.target_table.clone() else {
            return;
        };
        let engine = self.engine();

        for col in profile.columns {
            // Low-cardinality → top-N horizontal bars.
            if col.approx_distinct > 0 && col.approx_distinct <= 24 {
                let engine = engine.clone();
                let table = table.clone();
                let col_name = col.name.clone();
                let ws_weak = cx.entity().downgrade();
                tokio::spawn(async move {
                    use dat0_engine::QueryEngine as _;
                    let data = match engine.column_topn(&table, &col_name, 8).await {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(error = %e, col = %col_name, "column_topn failed");
                            return;
                        }
                    };
                    Self::dispatch_extra(ws_weak, load_id, move |ws, cx| {
                        ws.inspector.put_topn(&col_name, data);
                        cx.notify();
                    });
                });
                continue;
            }

            // Numeric high-cardinality → sampled histogram. Cast to DOUBLE so the
            // Arrow column is always Float64 regardless of the source numeric type.
            if let Some(numeric) = col.numeric.clone() {
                if col.approx_distinct > 24 {
                    let engine = engine.clone();
                    let col_name = col.name.clone();
                    let col_q = dat0_engine::quote_ident(&col_name);
                    let tbl_q = dat0_engine::quote_ident(&table);
                    let sql = format!(
                        "SELECT CAST({c} AS DOUBLE) AS v FROM {t} \
                         WHERE {c} IS NOT NULL USING SAMPLE 2048 ROWS",
                        c = col_q,
                        t = tbl_q
                    );
                    let ws_weak = cx.entity().downgrade();
                    tokio::spawn(async move {
                        use dat0_engine::QueryEngine as _;
                        use duckdb::arrow::array::{Array, Float64Array};
                        let result = match engine.execute(&sql).await {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::warn!(error = %e, col = %col_name, "histogram sample failed");
                                return;
                            }
                        };
                        let mut values: Vec<f64> = Vec::new();
                        for batch in &result.batches {
                            if let Some(a) = batch.column(0).as_any().downcast_ref::<Float64Array>()
                            {
                                for row in 0..a.len() {
                                    if a.is_valid(row) {
                                        values.push(a.value(row));
                                    }
                                }
                            }
                        }
                        if values.is_empty() {
                            return;
                        }
                        let bins =
                            crate::charts::histogram_bins(numeric.min, numeric.max, &values, 16);
                        Self::dispatch_extra(ws_weak, load_id, move |ws, cx| {
                            ws.inspector.put_histogram(&col_name, bins);
                            cx.notify();
                        });
                    });
                }
            }
        }
    }

    /// Hop an extras-write back to the main thread via the registry dispatcher
    /// under the supersede guard (T10). `f` runs only if `load_id` is still the
    /// current inspector load.
    fn dispatch_extra(
        ws_weak: gpui::WeakEntity<Self>,
        load_id: u64,
        f: impl FnOnce(&mut Self, &mut gpui::Context<Self>) + Send + 'static,
    ) {
        let Some(dispatcher) = crate::window_registry::dispatcher() else {
            tracing::warn!("load_column_extras: no MainThreadDispatcher; extra dropped");
            return;
        };
        let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
            let Some(ws) = ws_weak.upgrade() else {
                return;
            };
            ws.update(app_cx, |ws, cx| {
                if ws.inspector.is_current(load_id) {
                    f(ws, cx);
                }
            });
        });
    }

    // ── Charts (P9a T7) ────────────────────────────────────────────────────

    /// Show/hide the right-dock Charts panel. On open, bind the panel to the
    /// active grid's base table (off-thread `describe_table`) and kick off the
    /// first plot query. No-op (toggle still flips) when no file is registered.
    ///
    /// Uses the proven off-thread pattern from `load_inspector_profile`:
    /// `tokio::spawn` the engine call, hop the UI write back via the registry
    /// dispatcher. `base_table()` is QUOTED + may be schema-qualified
    /// (`"main"."orders"`); `describe_table` wants the BARE name, while the
    /// chart `source` must be a single quoted identifier — so we reduce to the
    /// bare name then re-quote it with `quote_ident`.
    pub(crate) fn toggle_chart_panel(&mut self, cx: &mut gpui::Context<Self>) {
        self.chart_panel_visible = !self.chart_panel_visible;
        if self.chart_panel_visible {
            if let Some(base) = self.base_table() {
                let bare = bare_table_name(&base);
                let engine = self.engine();
                let ws_weak = cx.entity().downgrade();
                tokio::spawn(async move {
                    use dat0_engine::QueryEngine as _;
                    let cols = engine
                        .describe_table(&bare, None)
                        .await
                        .map(|cs| {
                            cs.into_iter()
                                .map(|c| (c.name, c.data_type))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    // Single quoted identifier; an `a.b` qualified name would be
                    // quoted whole (`"a.b"`) — accepted v1 limitation.
                    let quoted = dat0_engine::quote_ident(&bare);
                    if let Some(dispatcher) = crate::window_registry::dispatcher() {
                        let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                            let Some(ws) = ws_weak.upgrade() else {
                                return;
                            };
                            ws.update(app_cx, |ws, cx| {
                                ws.chart_panel.bind(quoted, cols);
                                ws.run_plot_query(cx);
                            });
                        });
                    } else {
                        tracing::warn!(
                            "toggle_chart_panel: no MainThreadDispatcher installed; chart bind dropped"
                        );
                    }
                });
            }
        }
        cx.notify();
    }

    /// Build the plot SQL for the current spec, run it off-thread, render the
    /// result to a BGRA `RenderImage`, and stash it on the shell. Bumps
    /// `chart_load_id` so only the latest query's image survives (supersede
    /// guard for fast type/axis changes). On a missing-axis spec error the
    /// panel shows the error text in place of a chart and clears the image.
    pub(crate) fn run_plot_query(&mut self, cx: &mut gpui::Context<Self>) {
        let spec = self.chart_panel.spec.clone();
        let engine = self.engine();
        let sql = match crate::charts::query::build_plot_sql(&spec) {
            Ok(s) => s,
            Err(e) => {
                self.chart_panel.error = Some(e);
                self.chart_image = None;
                cx.notify();
                return;
            }
        };
        self.chart_load_id = self.chart_load_id.wrapping_add(1);
        let load_id = self.chart_load_id;
        // Logical chart size (px) × the bitmap supersample factor.
        let (lw, lh) = (520u32, 360u32);
        let scale = 2.0_f32;
        let (pw, ph) = ((lw as f32 * scale) as u32, (lh as f32 * scale) as u32);
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let qr = engine.execute(&sql).await;
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        // Supersede: a newer query already kicked off → drop this.
                        if ws.chart_load_id != load_id {
                            return;
                        }
                        match qr {
                            Ok(qr) => {
                                let pt = crate::charts::data::PlotTable::from_query_result(&qr);
                                let (bgra, w, h) = crate::charts::render::render_bgra(
                                    &ws.chart_panel.spec,
                                    &pt,
                                    (pw, ph),
                                );
                                match image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, bgra)
                                {
                                    Some(buf) => {
                                        let ri =
                                            gpui::RenderImage::new(smallvec::SmallVec::from_elem(
                                                image::Frame::new(buf),
                                                1,
                                            ));
                                        ws.chart_panel.error = None;
                                        ws.chart_panel.data = Some(pt);
                                        ws.chart_image = Some(std::sync::Arc::new(ri));
                                    }
                                    None => {
                                        ws.chart_panel.error =
                                            Some("chart image buffer build failed".into());
                                        ws.chart_image = None;
                                    }
                                }
                            }
                            Err(e) => {
                                ws.chart_panel.error = Some(e.to_string());
                                ws.chart_image = None;
                            }
                        }
                        cx.notify();
                    });
                });
            } else {
                tracing::warn!("run_plot_query: no MainThreadDispatcher installed; chart dropped");
            }
        });
        cx.notify();
    }

    /// Render the Charts dock toolbar (P9a T7): a chart-TYPE cycle button, one
    /// cycle button per *visible* axis (per `visible_axes(type)`), and PNG / SVG
    /// export buttons.
    ///
    /// Toolbar approach: **Button-cycle** (not gpui-component `Select`). Each
    /// click advances the value and immediately re-runs the plot query, so the
    /// data flow is identical to a Select-backed picker — type/axis change →
    /// mutate `spec` → `run_plot_query` → re-render. A button cycle is
    /// borrow-checker-trivial (no `Entity<SelectState>` to thread through the
    /// shell) and re-renders reliably, which the escalation note prefers over a
    /// half-working Select.
    fn render_chart_toolbar(&mut self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        use crate::charts::panel::{column_options, visible_axes};
        use crate::charts::spec::ChartType;
        use gpui_component::button::Button;
        // `.disabled(..)` on `Button` comes from the `Disableable` trait.
        use gpui_component::Disableable;

        let cur_type = self.chart_panel.spec.chart_type;

        // ── Chart-type cycle button ────────────────────────────────────────
        let type_btn = Button::new("chart-type")
            .label(format!(
                "{}: {}",
                dat0_i18n::t("chart.panel.title"),
                dat0_i18n::t(cur_type.label_key())
            ))
            .on_click(cx.listener(|ws, _ev, _window, cx| {
                let cur = ws.chart_panel.spec.chart_type;
                let i = ChartType::ALL.iter().position(|t| *t == cur).unwrap_or(0);
                let next = ChartType::ALL[(i + 1) % ChartType::ALL.len()];
                ws.chart_panel.spec.chart_type = next;
                // A new type may expose axes the old picks don't satisfy; leave
                // the picks as-is (build_plot_sql errors → panel shows a "needs a
                // <role> column" hint until the user picks one).
                ws.run_plot_query(cx);
            }));

        // ── Per-visible-axis cycle buttons ─────────────────────────────────
        let mut row = h_flex().gap_2().flex_wrap().p_2().child(type_btn);
        for role in visible_axes(cur_type) {
            let current = axis_field(&self.chart_panel.spec, role).map(str::to_string);
            let label_role = dat0_i18n::t(axis_role_key(role));
            let label_val = current.clone().unwrap_or_else(|| "—".to_string());
            let id = format!("chart-axis-{}", axis_role_key(role));
            let role_copy = role;
            let btn = Button::new(gpui::SharedString::from(id))
                .label(format!("{label_role}: {label_val}"))
                .on_click(cx.listener(move |ws, _ev, _window, cx| {
                    let opts = column_options(role_copy, &ws.chart_panel.columns);
                    let next = cycle_axis(
                        axis_field(&ws.chart_panel.spec, role_copy),
                        &opts,
                        // Required axes (X always, plus Y/Value) never cycle to
                        // None; optional axes (Group/Color) include a None step.
                        axis_required(role_copy),
                    );
                    set_axis_field(&mut ws.chart_panel.spec, role_copy, next);
                    ws.run_plot_query(cx);
                }));
            row = row.child(btn);
        }

        // ── Export buttons (PNG / SVG) ─────────────────────────────────────
        let png_btn = Button::new("chart-export-png")
            .label(dat0_i18n::t("chart.export.png"))
            .on_click(cx.listener(|ws, _ev, _window, cx| {
                ws.export_chart(true, cx);
            }));
        let svg_btn = Button::new("chart-export-svg")
            .label(dat0_i18n::t("chart.export.svg"))
            .on_click(cx.listener(|ws, _ev, _window, cx| {
                ws.export_chart(false, cx);
            }));
        // ── Save button (P9a-2) ────────────────────────────────────────────
        // Disabled until a chart is renderable (a source is bound AND at least
        // one axis is picked), so an empty chart can never be saved. Mirrors the
        // export guard's spirit but is enforced at the button (disabled) rather
        // than as a silent no-op, so the affordance reads correctly.
        let can_save = self.chart_panel.source.is_some()
            && (self.chart_panel.spec.x.is_some() || self.chart_panel.spec.y.is_some());
        let save_btn = Button::new("chart-save")
            .label(dat0_i18n::t("chart.save"))
            .disabled(!can_save)
            .on_click(cx.listener(|ws, _ev, window, cx| {
                ws.open_chart_save_prompt(window, cx);
            }));

        // Clicking export with no rendered data is a silent no-op
        // (`export_chart` guards on `chart_panel.data`).
        row.child(png_btn).child(svg_btn).child(save_btn)
    }

    /// Open the shared name-prompt overlay to save the currently-bound chart
    /// under a user name (P9a-2). Seeds the prompt with the generated default
    /// ([`default_chart_name`](crate::session::charts::default_chart_name)), then
    /// routes a confirm to [`save_named_chart`](Self::save_named_chart) via the
    /// [`SaveChart`](NamePromptIntent::SaveChart) intent. No-op when no source is
    /// bound (the toolbar Save button is also disabled in that state).
    pub(crate) fn open_chart_save_prompt(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.chart_panel.source.is_none() {
            return;
        }
        let prefill = crate::session::charts::default_chart_name(&self.chart_panel.spec);
        self.open_name_prompt_with(
            dat0_i18n::t("chart.save.prompt"),
            prefill,
            NamePromptIntent::SaveChart,
            window,
            cx,
        );
    }

    /// Open the native save panel and export the current chart to PNG (`png =
    /// true`) or SVG. No-op when there's no rendered data yet — the live
    /// `chart_panel.spec` + `data` carry everything `export_*` needs (P9a T7).
    fn export_chart(&mut self, png: bool, cx: &mut gpui::Context<Self>) {
        let Some(data) = self.chart_panel.data.clone() else {
            return;
        };
        let spec = self.chart_panel.spec.clone();
        let ext = if png { "png" } else { "svg" };
        let suggested = format!("chart.{ext}");
        let path_rx = cx.prompt_for_new_path(std::path::Path::new(""), Some(&suggested));
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, _async_cx| {
            let dest = match path_rx.await {
                Ok(Ok(Some(dest))) => dest,
                _ => return,
            };
            // Export the SAME logical size the dock renders at (the bitmap
            // backend supersamples internally; here we write at logical px).
            let size = (1040u32, 720u32);
            let result: Result<(), String> = if png {
                crate::charts::export::export_png(&spec, &data, size, &dest)
                    .map_err(|e| e.to_string())
            } else {
                crate::charts::export::export_svg(&spec, &data, size, &dest)
                    .map_err(|e| e.to_string())
            };
            match result {
                Ok(()) => crate::error_ux::push(crate::error_ux::Banner::info(format!(
                    "{} → {}",
                    dat0_i18n::t("chart.save"),
                    dest.display()
                ))),
                Err(e) => crate::error_ux::push(crate::error_ux::Banner::warning(e)),
            }
        })
        .detach();
    }

    /// Flip the inspector between Whole-table and Current-view profiling and
    /// re-profile (P6a T9). The cache is keyed by (table,epoch) — *not* by mode —
    /// so a toggle always `begin_load`s and re-fetches; the latest mode's profile
    /// wins (switching back re-fetches). Takes no `Window` (none needed).
    pub(crate) fn toggle_inspector_mode(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        use crate::inspector::ProfileTargetMode::*;
        self.inspector.mode = match self.inspector.mode {
            WholeTable => CurrentView,
            CurrentView => WholeTable,
        };
        // Drop stale extras so a WholeTable column's bars don't survive onto a
        // CurrentView card with the same name (T10).
        self.inspector.clear_extras();
        self.load_inspector_profile(cx);
        cx.notify();
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
                    "",
                    NamePromptIntent::SaveConsoleAsTable,
                    window,
                    cx,
                );
            }
            OpenNl2SqlPrompt => {
                self.open_name_prompt_with(
                    dat0_i18n::t("sql.nl2sql.prompt_title"),
                    "",
                    NamePromptIntent::Nl2SqlPrompt,
                    window,
                    cx,
                );
            }
            StopAiStream => {
                // Supersede: drop the in-flight stream; partial text stays Insert-able.
                // Also finish the Explain panel if that was the active stream (T7).
                self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
                if let Some(console) = &self.sql_console {
                    console.update(cx, |c, cx| {
                        c.finish_nl_preview(None, cx);
                        c.finish_explain(None, cx);
                    });
                }
            }
            Explain => {
                self.spawn_ai_explain(cx);
            }
            CloseExplain => {
                self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
                if let Some(console) = &self.sql_console {
                    console.update(cx, |c, cx| c.clear_explain(cx));
                }
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

    /// Persist the catalog/inspector dock UI state to `session.json` (P6a T13;
    /// v10 adds the catalog collapse set). Sorted for a deterministic wire
    /// format (the insta snapshot gates it).
    pub(crate) fn persist_dock_ui(&self) {
        let mut catalog_collapsed: Vec<String> = self.catalog_collapsed.iter().cloned().collect();
        catalog_collapsed.sort();
        let ui = crate::session::SessionUiState {
            catalog_panel_visible: self.catalog_panel_visible,
            inspector_panel_visible: self.inspector_panel_visible,
            catalog_collapsed,
        };
        if let Err(e) = self.session.lock().set_ui(ui) {
            tracing::warn!(error = %e, "persist_dock_ui: set_ui failed");
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

        // Read-only guard (P8 T8): in Inspect mode only result-producing
        // statements (SELECT / WITH / PRAGMA / DESCRIBE / SUMMARIZE / …) are
        // allowed; DDL/DML (ResultKind::Exec) is silently blocked here because
        // the Parquet-backed VIEWs would reject it at the engine level anyway.
        if crate::grid::edit_ops::mutation_blocked(self.read_only) && kind == ResultKind::Exec {
            return;
        }

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
        // Mirror into the Catalog dock so created/dropped tables appear (P6a T7).
        self.refresh_catalog(cx);
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
        drop(sess);
        self.maybe_prompt_save_workspace();
    }

    /// Persist the currently-bound chart spec as a named saved chart (P9a-2).
    /// Upserts by name (case-insensitive). No-op on empty name / no chart bound.
    /// Mirrors [`save_named_query`](Self::save_named_query) — reaches the session
    /// via `self.session.lock()`, upserts into the persisted list, then pushes an
    /// info banner and refreshes the catalog so the new chart appears in lineage.
    /// Called from [`on_name_prompt_event`](Self::on_name_prompt_event) on a
    /// [`SaveChart`](NamePromptIntent::SaveChart) confirm.
    pub(crate) fn save_named_chart(&mut self, name: String, cx: &mut Context<Self>) {
        if name.trim().is_empty() {
            return;
        }
        // `chart_panel` is a plain field on the shell (not an `Entity`), so the
        // live spec is read directly — there is no chart to save unless a source
        // is bound.
        if self.chart_panel.source.is_none() {
            return;
        }
        let spec = self.chart_panel.spec.clone();
        let c = crate::session::charts::SavedChart {
            id: uuid::Uuid::now_v7(),
            name: name.trim().to_string(),
            spec,
            saved_at: now_unix_millis(),
        };
        let mut sess = self.session.lock();
        let mut list = sess.charts().to_vec();
        crate::session::charts::upsert_chart(&mut list, c);
        let _ = sess.set_charts(list);
        drop(sess);
        crate::error_ux::push(crate::error_ux::Banner::info(dat0_i18n::t(
            "chart.save.done.title",
        )));
        self.refresh_catalog(cx); // so the new chart appears in lineage
        self.maybe_prompt_save_workspace();
    }

    /// Reopen a saved chart by name (P9a-2): look it up in the session, bind the
    /// chart panel to its stored spec, and render. Mirrors the "Visualize" open
    /// path (`toggle_chart_panel`) but seeds the panel from a persisted
    /// [`ChartSpec`] instead of building a fresh one from the active grid.
    /// Invoked from the Inspector lineage chain when a `NodeKind::Chart` row is
    /// clicked. No-op (silently) when the named chart is gone from the session.
    pub(crate) fn open_saved_chart(
        &mut self,
        name: String,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let spec = {
            let sess = self.session.lock();
            sess.charts()
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.spec.clone())
        };
        let Some(spec) = spec else { return };
        self.show_chart_with_spec(spec, window, cx);
    }

    /// Show the Charts dock seeded from a persisted [`ChartSpec`] (P9a-2). Unlike
    /// [`toggle_chart_panel`](Self::toggle_chart_panel) — which binds a *fresh*
    /// chart from the active grid and so resets all axis picks — this preserves
    /// the saved spec verbatim (chart type + axis picks + title) and only fetches
    /// the source's columns off-thread to repopulate the toolbar's axis-cycle
    /// options. The render is then driven by the SAME `run_plot_query` path the
    /// Visualize flow uses, so the data flow is identical.
    ///
    /// `spec.source` is a single quoted identifier (saved from a live spec);
    /// `describe_table` needs the bare name, so we reduce it via
    /// [`bare_table_name`].
    pub(crate) fn show_chart_with_spec(
        &mut self,
        spec: crate::charts::spec::ChartSpec,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // Seed the panel from the saved spec, preserving axis picks. Columns are
        // filled in once `describe_table` returns (below); the plot renders then.
        self.chart_panel_visible = true;
        self.chart_panel.source = Some(spec.source.clone());
        self.chart_panel.spec = spec.clone();
        self.chart_panel.columns = Vec::new();
        self.chart_panel.data = None;
        self.chart_panel.error = None;

        let bare = bare_table_name(&spec.source);
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let cols = engine
                .describe_table(&bare, None)
                .await
                .map(|cs| {
                    cs.into_iter()
                        .map(|c| (c.name, c.data_type))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        ws.chart_panel.columns = cols;
                        ws.run_plot_query(cx);
                    });
                });
            } else {
                tracing::warn!(
                    "show_chart_with_spec: no MainThreadDispatcher installed; chart bind dropped"
                );
            }
        });
        cx.notify();
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
                                ws.refresh_catalog(cx);
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
            "",
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
        if crate::grid::edit_ops::mutation_blocked(self.read_only) {
            return;
        }
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
                            Ok(_) => {
                                ws.refresh_completion_snapshot(cx);
                                ws.refresh_catalog(cx);
                            }
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
        self.open_name_prompt_with(
            "Save query as…",
            "",
            NamePromptIntent::SaveQuery,
            window,
            cx,
        );
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
    /// `initial` seeds the name field (editable). Pass `""` for the flows that
    /// start blank (Save query / Save as table); the Save-chart flow passes the
    /// generated default name (P9a-2).
    ///
    /// Needs `&mut Window` because `NamePrompt::new` builds an `InputState`
    /// (single-line name field) eagerly.
    fn open_name_prompt_with(
        &mut self,
        title: impl Into<gpui::SharedString>,
        initial: impl Into<gpui::SharedString>,
        intent: NamePromptIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::name_prompt::{NamePrompt, NamePromptEvent};
        let prompt = cx.new(|cx| NamePrompt::new(title, initial, window, cx));
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
                Some(NamePromptIntent::SaveChart) => {
                    self.save_named_chart(name, cx);
                }
                Some(NamePromptIntent::Nl2SqlPrompt) => {
                    self.spawn_ai_nl2sql(name, cx);
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

        // Any connection action dismisses a prior Test-connection message.
        self.connections.clear_md_test_result();

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
            // Test connection: same precheck as Connect, but spawns the probe
            // that records a transient pass/fail message.
            ConnectionsEvent::TestMd => {
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
                        self.spawn_md_test(token, cx);
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

    /// Disconnect MotherDuck: a SOFT disconnect — flip the manager to
    /// Disconnected and drop the persisted md attachment, but DO NOT `DETACH`.
    /// In workspace mode `DETACH` persists to the account's saved MotherDuck
    /// workspace (the db moves to "Detached Databases", needing manual
    /// re-attach), so a local disconnect must not mutate the user's cloud
    /// workspace. The in-session attachment lingers harmlessly until the window
    /// closes; dat0 simply stops surfacing it, and a later Connect is idempotent
    /// (the engine arm skips a redundant ATTACH). Shared by Disconnect + Forget.
    fn disconnect_md(&mut self, cx: &mut Context<Self>) {
        use crate::connections::ConnectionStatus;
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
                                // Populate the catalog Cloud group immediately (md dbs just attached).
                                ws.refresh_catalog(cx);
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

    /// Spawn the async MotherDuck "Test connection" probe. Identical engine
    /// bridge to [`spawn_md_connect`] (idempotent workspace-mode ATTACH with the
    /// stored token), but additionally records a transient pass/fail message via
    /// `set_md_test_result` so the panel can confirm the probe ran — the status
    /// pill alone cannot signal "still OK" when already Connected. The token is
    /// moved straight into `run_connect` and never logged.
    ///
    /// [`spawn_md_connect`]: Self::spawn_md_connect
    fn spawn_md_test(&mut self, token: String, cx: &mut Context<Self>) {
        let engine = self.engine();
        let engine_for_list = engine.clone();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            let status = crate::connections::connect::run_connect(engine, token).await;
            let connected = matches!(status, crate::connections::ConnectionStatus::Connected);
            let message = crate::connections::connect::test_result_message(&status);
            let dbs = if connected {
                crate::connections::connect::list_databases(engine_for_list).await
            } else {
                Vec::new()
            };
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| {
                            ws.connections.set_md_status(status.clone());
                            if connected {
                                ws.connections.set_md_databases(dbs.clone());
                                // Persist the md attachment (idempotent) so a
                                // recover re-attaches it — matches spawn_md_connect.
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
                                // Populate the catalog Cloud group immediately (md dbs just attached).
                                ws.refresh_catalog(cx);
                            }
                            // Set the message AFTER status (set_md_status never
                            // touches md_test_result).
                            ws.connections.set_md_test_result(message);
                            cx.notify();
                        });
                    }
                });
            } else {
                tracing::warn!("spawn_md_test: no MainThreadDispatcher installed; result dropped");
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
        let prompt = cx
            .new(|cx| NamePrompt::new(dat0_i18n::t("connections.md.token_prompt"), "", window, cx));
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

    // ── AI panel (P9c-1 T9) ────────────────────────────────────────────────

    /// Open the on-disk settings store (`config_dir/settings.toml`). Returns
    /// `None` (logging) when the config dir is unavailable — callers skip the
    /// persist rather than crash. The API KEY is never routed through this store
    /// (it lives only in the keychain).
    fn ai_settings_store() -> Option<crate::settings::store::SettingsStore> {
        match crate::platform::config_dir() {
            Ok(dir) => Some(crate::settings::store::SettingsStore::with_path(
                dir.join("settings.toml"),
            )),
            Err(e) => {
                tracing::warn!(
                    ?e,
                    "ai_settings_store: config_dir unavailable; AI settings not persisted"
                );
                None
            }
        }
    }

    /// Toggle the left-dock AI panel. On open, hydrate the draft state from the
    /// persisted `AiSettings` and probe the keychain for whether a key is set for
    /// the selected provider (the key value itself is never read into state).
    pub(crate) fn toggle_ai_panel(&mut self, cx: &mut gpui::Context<Self>) {
        self.ai_panel_visible = !self.ai_panel_visible;
        if self.ai_panel_visible {
            self.hydrate_ai_panel();
        }
        cx.notify();
    }

    /// Load the AI-panel draft from persisted settings + keychain key-presence.
    /// Never reads the key value — only whether a key exists for the provider.
    fn hydrate_ai_panel(&mut self) {
        let settings = Self::ai_settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        let provider = settings
            .provider
            .as_deref()
            .and_then(crate::ai::Provider::from_id);
        let key_set = provider
            .and_then(|p| {
                use crate::ai::key_store::KeyStore as _;
                crate::ai::key_store::KeychainKeyStore::new()
                    .ok()
                    .and_then(|ks| ks.get(p).ok())
                    .flatten()
            })
            .is_some();
        self.ai_panel = crate::ai::panel::AiPanel {
            provider,
            key_set,
            model: settings.model,
            enabled: settings.enabled,
            advanced_override: settings.advanced_override,
            include_sample_rows: settings.include_sample_rows,
            test_result: None,
        };
    }

    /// Mutate the persisted `AiSettings` in place via the atomic settings-write
    /// path (load → mutate → save). The API KEY is never a field here, so it can
    /// never reach settings.toml. Logs + skips on any store error.
    fn update_ai_settings(&self, f: impl FnOnce(&mut crate::ai::AiSettings)) {
        let Some(store) = Self::ai_settings_store() else {
            return;
        };
        let mut settings = match store.load_or_default() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "update_ai_settings: load failed; change not persisted");
                return;
            }
        };
        f(&mut settings.ai);
        if let Err(e) = store.save(&settings) {
            tracing::warn!(?e, "update_ai_settings: save failed; change not persisted");
        }
    }

    /// Show the first-use AI privacy notice exactly once, then persist the ack so it
    /// never reappears (D5 / R17 transparency). Idempotent: gated on the persisted
    /// `privacy_ack`. Banner is text-only (no action buttons — D-021).
    fn maybe_show_ai_privacy_banner(&self) {
        let ack = Self::ai_settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai.privacy_ack)
            .unwrap_or(false);
        if crate::ai::settings::should_show_privacy_banner(ack) {
            crate::error_ux::banner::push(crate::error_ux::banner::Banner {
                title: dat0_i18n::t("ai.privacy.title"),
                body: dat0_i18n::t("ai.privacy.body"),
                link: None,
                primary: None,
                secondary: None,
                kind: crate::error_ux::banner::BannerKind::Info,
                dismissible: true,
            });
            self.update_ai_settings(|s| s.privacy_ack = true);
        }
    }

    /// Handle one AI-panel button event. Mirrors [`Self::handle_connections_event`]:
    /// config changes persist to settings.toml (NEVER the key), the key writes to
    /// the keychain, and Test-connection runs `ai::transport::test_connection`
    /// async (off the GPUI main thread), recording a transient result.
    pub(crate) fn handle_ai_panel_event(
        &mut self,
        ev: crate::ai::panel::AiPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::ai::panel::AiPanelEvent;

        // Any config action dismisses a prior Test-connection message.
        self.ai_panel.test_result = None;

        match ev {
            AiPanelEvent::SelectProvider(p) => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.provider = Some(p);
                let id = p.id().to_string();
                self.update_ai_settings(|s| s.provider = Some(id));
                // Re-probe the keychain for whether THIS provider has a key set.
                use crate::ai::key_store::KeyStore as _;
                self.ai_panel.key_set = crate::ai::key_store::KeychainKeyStore::new()
                    .ok()
                    .and_then(|ks| ks.get(p).ok())
                    .flatten()
                    .is_some();
                cx.notify();
            }
            // Empty string = open the entry prompt; a non-empty value (re-dispatched
            // from the prompt's Confirm) writes the key to the keychain.
            AiPanelEvent::SetKey(value) => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                if value.is_empty() {
                    self.open_ai_entry_prompt(AiEntryKind::Key, window, cx);
                } else {
                    let Some(provider) = self.ai_panel.provider else {
                        return; // No provider selected → nothing to key.
                    };
                    use crate::ai::key_store::KeyStore as _;
                    match crate::ai::key_store::KeychainKeyStore::new()
                        .and_then(|ks| ks.set(provider, &value))
                    {
                        Ok(()) => {
                            // Reflect "key set" WITHOUT retaining the key value.
                            self.ai_panel.key_set = true;
                        }
                        Err(e) => {
                            // The message must not contain the key (it doesn't —
                            // KeychainKeyStore errors never embed the secret).
                            self.ai_panel.test_result =
                                Some(crate::ai::panel::test_result_message(false, &e.to_string()));
                        }
                    }
                    cx.notify();
                }
            }
            AiPanelEvent::SetModel(value) => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                if value.is_empty() {
                    self.open_ai_entry_prompt(AiEntryKind::Model, window, cx);
                } else {
                    self.ai_panel.model = value.clone();
                    self.update_ai_settings(|s| s.model = value);
                    cx.notify();
                }
            }
            AiPanelEvent::ToggleEnabled => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.enabled = !self.ai_panel.enabled;
                let v = self.ai_panel.enabled;
                self.update_ai_settings(|s| s.enabled = v);
                // Show privacy notice on first enable (idempotent: gated by persisted ack).
                if v {
                    self.maybe_show_ai_privacy_banner();
                }
                cx.notify();
            }
            AiPanelEvent::ToggleAdvancedOverride => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.advanced_override = !self.ai_panel.advanced_override;
                let v = self.ai_panel.advanced_override;
                self.update_ai_settings(|s| s.advanced_override = v);
                cx.notify();
            }
            AiPanelEvent::ToggleIncludeSampleRows => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.include_sample_rows = !self.ai_panel.include_sample_rows;
                let v = self.ai_panel.include_sample_rows;
                self.update_ai_settings(|s| s.include_sample_rows = v);
                cx.notify();
            }
            AiPanelEvent::ForgetKey => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                if let Some(provider) = self.ai_panel.provider {
                    use crate::ai::key_store::KeyStore as _;
                    if let Ok(ks) = crate::ai::key_store::KeychainKeyStore::new() {
                        let _ = ks.forget(provider);
                    }
                }
                self.ai_panel.key_set = false;
                cx.notify();
            }
            AiPanelEvent::TestConnection => {
                self.maybe_show_ai_privacy_banner();
                self.spawn_ai_test(cx);
            }
        }
        // Keep the NL→SQL chip gate in sync after any AI config mutation.
        self.push_ai_ready_to_console(cx);
    }

    /// Whether AI features are ready to use (enabled + key set + model configured).
    /// Gates the NL→SQL chip and the spawn preamble.
    fn ai_ready(&self) -> bool {
        self.ai_panel.enabled && self.ai_panel.key_set && !self.ai_panel.model.is_empty()
    }

    /// Push the current `ai_ready()` state into the SQL console (if built).
    /// Called after any AI config mutation to keep the chip gated correctly.
    fn push_ai_ready_to_console(&mut self, cx: &mut Context<Self>) {
        let ready = self.ai_ready();
        if let Some(console) = &self.sql_console {
            console.update(cx, |c, _cx| c.ai_ready = ready);
        }
    }

    /// Spawn the async AI Test-connection probe. Reads the key from the keychain
    /// (never logged, never held in state), loads the persisted `AiSettings`, and
    /// runs `ai::transport::test_connection` (which carries the SSRF + schema-only
    /// guarantees) off the GPUI main thread. The transient pass/fail is written
    /// back on the main thread via the registry dispatcher — mirrors
    /// [`Self::spawn_md_test`].
    fn spawn_ai_test(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(provider) = self.ai_panel.provider else {
            self.ai_panel.test_result = Some(crate::ai::panel::test_result_message(
                false,
                &dat0_i18n::t("ai.test.no_provider"),
            ));
            cx.notify();
            return;
        };
        // Resolve the key + settings on the main thread BEFORE spawning so the
        // task captures only owned `Send` values. The key is moved straight into
        // the task and dropped when it ends; it is never logged.
        use crate::ai::key_store::KeyStore as _;
        let key = match crate::ai::key_store::KeychainKeyStore::new()
            .ok()
            .and_then(|ks| ks.get(provider).ok())
            .flatten()
        {
            Some(k) => k,
            None => {
                self.ai_panel.test_result = Some(crate::ai::panel::test_result_message(
                    false,
                    &dat0_i18n::t("ai.test.no_key"),
                ));
                cx.notify();
                return;
            }
        };
        let cfg = Self::ai_settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        // Supersede guard (mirrors `chart_load_id`): bump before spawning so that
        // any config change that arrives while the request is in flight invalidates
        // this result.
        self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
        let load_id = self.ai_test_load_id;
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            let outcome = crate::ai::transport::test_connection(provider, &key, &cfg).await;
            // Drop the key as early as possible (it is no longer needed).
            drop(key);
            let message = crate::ai::panel::test_result_message(outcome.ok, &outcome.message);
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| {
                            // Supersede: a config change arrived while we were in
                            // flight → the result is stale; drop it.
                            if ws.ai_test_load_id != load_id {
                                tracing::debug!(
                                    "spawn_ai_test: stale result discarded \
                                     (load_id={load_id}, current={})",
                                    ws.ai_test_load_id
                                );
                                return;
                            }
                            ws.ai_panel.test_result = Some(message);
                            cx.notify();
                        });
                    }
                });
            } else {
                tracing::warn!("spawn_ai_test: no MainThreadDispatcher installed; result dropped");
            }
        });
        cx.notify();
    }

    /// Spawn an NL→SQL streaming request. Mirrors [`spawn_ai_test`]'s preamble
    /// exactly, then streams deltas into the console's NL preview strip via
    /// per-delta main-thread dispatches guarded by `ai_stream_load_id`.
    ///
    /// R17 safety:
    /// - `sample_rows: None` — NL→SQL never sends row data.
    /// - Schema built from `catalog_tables` via `build_schema_context` (names +
    ///   types only; surrogate `__dat0_rowid` dropped by `SchemaCaps::default()`).
    /// - Guard: `ai_stream_load_id` supersede check inside every dispatched closure.
    fn spawn_ai_nl2sql(&mut self, prompt: String, cx: &mut gpui::Context<Self>) {
        use crate::ai::key_store::KeyStore as _;
        let Some(provider) = self.ai_panel.provider else {
            return;
        };
        let key = match crate::ai::key_store::KeychainKeyStore::new()
            .ok()
            .and_then(|ks| ks.get(provider).ok())
            .flatten()
        {
            Some(k) => k,
            None => return, // ai_ready gate prevents this; belt-and-suspenders
        };
        let cfg = Self::ai_settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        if cfg.model.is_empty() {
            return;
        }
        // Build schema-only context from the cached catalog (R17: names+types only).
        let (schema, note) = crate::ai::schema_ctx::build_schema_context(
            &self.catalog_tables,
            crate::ai::schema_ctx::SchemaCaps::default(),
        );
        let mut user_prompt = prompt.clone();
        if let Some(note) = note {
            user_prompt.push_str("\n\n(");
            user_prompt.push_str(&note);
            user_prompt.push(')');
        }
        let req = crate::ai::request::AiRequest {
            model: cfg.model.clone(),
            system: Some(crate::ai::prompt::nl_to_sql_system().to_string()),
            schema,
            prompt: user_prompt,
            sample_rows: None, // R17: NL→SQL never sends row data
            max_tokens: 1024,
        };

        self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
        let load_id = self.ai_stream_load_id;
        if let Some(console) = &self.sql_console {
            console.update(cx, |c, cx| c.begin_nl_preview(prompt.clone(), cx));
        }
        let ws_weak = cx.entity().downgrade();
        let ws_weak_finish = ws_weak.clone();

        tokio::spawn(async move {
            let result = crate::ai::transport::send_stream(provider, &key, &cfg, &req, |delta| {
                let text = delta.to_string();
                let ws_weak_delta = ws_weak.clone();
                if let Some(d) = crate::window_registry::dispatcher() {
                    let _ = d.dispatch(move |app: &mut gpui::App| {
                        if let Some(ws) = ws_weak_delta.upgrade() {
                            ws.update(app, |ws, cx| {
                                if ws.ai_stream_load_id != load_id {
                                    return; // stale → drop
                                }
                                if let Some(console) = &ws.sql_console {
                                    console.update(cx, |c, cx| c.push_nl_delta(&text, cx));
                                }
                            });
                        }
                    });
                }
            })
            .await;
            drop(key);
            let err = result.err().map(|e| e.to_string());
            if let Some(d) = crate::window_registry::dispatcher() {
                let _ = d.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak_finish.upgrade() {
                        ws.update(app, |ws, cx| {
                            if ws.ai_stream_load_id != load_id {
                                return;
                            }
                            if let Some(console) = &ws.sql_console {
                                console.update(cx, |c, cx| c.finish_nl_preview(err, cx));
                            }
                        });
                    }
                });
            }
        });
        cx.notify();
    }

    /// Stream a plain-language explanation of the active-tab SQL buffer into the
    /// Explain side panel (P9c-2 T7). Mirrors `spawn_ai_nl2sql` exactly, with:
    /// - prompt = the whole active buffer SQL (read on main thread before spawn);
    /// - system: `explain_system()`;
    /// - `begin_explain`/`push_explain_delta`/`finish_explain` instead of NL variants;
    /// - `max_tokens: 1024`; `sample_rows: None` (R17 invariant).
    ///
    /// Reuses the single `ai_stream_load_id` counter (no second counter added).
    ///
    /// R17 safety:
    /// - `sample_rows: None` — Explain never sends row data.
    /// - Schema built from `catalog_tables` via `build_schema_context`.
    /// - Guard: `ai_stream_load_id` supersede check inside every dispatched closure.
    fn spawn_ai_explain(&mut self, cx: &mut gpui::Context<Self>) {
        use crate::ai::key_store::KeyStore as _;
        let Some(provider) = self.ai_panel.provider else {
            return;
        };
        let key = match crate::ai::key_store::KeychainKeyStore::new()
            .ok()
            .and_then(|ks| ks.get(provider).ok())
            .flatten()
        {
            Some(k) => k,
            None => return, // ai_ready gate prevents this; belt-and-suspenders
        };
        let cfg = Self::ai_settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        if cfg.model.is_empty() {
            return;
        }

        // Read the active SQL on the main thread BEFORE spawning (no Send across
        // the tokio boundary; the read needs &App which we have here).
        let sql = match &self.sql_console {
            Some(c) => c.read(cx).active_sql_and_cursor(cx).0,
            None => return,
        };
        if sql.trim().is_empty() {
            return;
        }

        // Build schema-only context from the cached catalog (R17: names+types only).
        let (schema, note) = crate::ai::schema_ctx::build_schema_context(
            &self.catalog_tables,
            crate::ai::schema_ctx::SchemaCaps::default(),
        );
        // The Explain prompt IS the SQL; schema truncation note appended to the
        // prompt text (not the schema field), per R17 design.
        let mut explain_prompt = sql.clone();
        if let Some(note) = note {
            explain_prompt.push_str("\n\n(");
            explain_prompt.push_str(&note);
            explain_prompt.push(')');
        }
        let req = crate::ai::request::AiRequest {
            model: cfg.model.clone(),
            system: Some(crate::ai::prompt::explain_system().to_string()),
            schema,
            prompt: explain_prompt,
            sample_rows: None, // R17: Explain never sends row data
            max_tokens: 1024,
        };

        self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
        let load_id = self.ai_stream_load_id;
        if let Some(console) = &self.sql_console {
            console.update(cx, |c, cx| c.begin_explain(sql, cx));
        }
        let ws_weak = cx.entity().downgrade();
        let ws_weak_finish = ws_weak.clone();

        tokio::spawn(async move {
            let result = crate::ai::transport::send_stream(provider, &key, &cfg, &req, |delta| {
                let text = delta.to_string();
                let ws_weak_delta = ws_weak.clone();
                if let Some(d) = crate::window_registry::dispatcher() {
                    let _ = d.dispatch(move |app: &mut gpui::App| {
                        if let Some(ws) = ws_weak_delta.upgrade() {
                            ws.update(app, |ws, cx| {
                                if ws.ai_stream_load_id != load_id {
                                    return; // stale → drop
                                }
                                if let Some(console) = &ws.sql_console {
                                    console.update(cx, |c, cx| {
                                        c.push_explain_delta(&text, cx);
                                    });
                                }
                            });
                        }
                    });
                }
            })
            .await;
            drop(key);
            let err = result.err().map(|e| e.to_string());
            if let Some(d) = crate::window_registry::dispatcher() {
                let _ = d.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak_finish.upgrade() {
                        ws.update(app, |ws, cx| {
                            if ws.ai_stream_load_id != load_id {
                                return;
                            }
                            if let Some(console) = &ws.sql_console {
                                console.update(cx, |c, cx| c.finish_explain(err, cx));
                            }
                        });
                    }
                });
            }
        });
        cx.notify();
    }

    /// Open the AI key/model entry modal (reuses
    /// [`NamePrompt`](crate::view::name_prompt::NamePrompt)). On Confirm the entered
    /// value is re-dispatched as the corresponding non-empty `SetKey`/`SetModel`
    /// event (which performs the keychain write / settings save). For a key entry
    /// the value never touches panel state until it is written to the keychain, and
    /// is never echoed back into a field.
    fn open_ai_entry_prompt(
        &mut self,
        kind: AiEntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::name_prompt::{NamePrompt, NamePromptEvent};
        let label = match kind {
            AiEntryKind::Key => dat0_i18n::t("ai.key.prompt"),
            AiEntryKind::Model => dat0_i18n::t("ai.model.prompt"),
        };
        let prompt = cx.new(|cx| NamePrompt::new(label, "", window, cx));
        let sub = cx.subscribe_in(
            &prompt,
            window,
            move |ws: &mut Self, _prompt, ev: &NamePromptEvent, window, cx| match ev {
                NamePromptEvent::Confirm(value) => {
                    let value = value.clone();
                    // Close the prompt first.
                    ws.ai_entry_prompt = None;
                    ws.ai_entry_prompt_sub = None;
                    if value.is_empty() {
                        cx.notify();
                        return;
                    }
                    let ev = match kind {
                        AiEntryKind::Key => crate::ai::panel::AiPanelEvent::SetKey(value),
                        AiEntryKind::Model => crate::ai::panel::AiPanelEvent::SetModel(value),
                    };
                    ws.handle_ai_panel_event(ev, window, cx);
                }
                NamePromptEvent::Cancel => {
                    ws.ai_entry_prompt = None;
                    ws.ai_entry_prompt_sub = None;
                    cx.notify();
                }
            },
        );
        self.ai_entry_prompt_sub = Some(sub);
        self.ai_entry_prompt = Some(prompt);
        cx.notify();
    }

    // ─── P11a T3: Hero open helpers ──────────────────────────────────────────

    /// Materialize and open a sample dataset (P11a T3).
    ///
    /// For bundled variants (`BundledCsv` / `BundledSqlite`): extracts bytes
    /// to `$state_root/samples/<dest>` (idempotent) then feeds the path to the
    /// `handle_drop` → data-source pipeline.  For `Remote` (NYC taxi): reuses
    /// `fetch_remote` + `fetch_failed_banner`.  Mirrors `drop_listener`'s
    /// `cx.spawn` + view-refresh pattern.  Wired by T4 hero buttons.
    pub(crate) fn open_sample_kind(
        &mut self,
        kind: crate::sample_data::SampleKind,
        cx: &mut Context<Self>,
    ) {
        use crate::sample_data::SampleKind;
        let Some(state_root) = crate::window_registry::state_root() else {
            crate::error_ux::push(crate::error_ux::Banner::error(
                "Cannot open sample",
                "App state directory not initialised",
            ));
            return;
        };
        let session = Arc::clone(&self.session);
        match kind {
            SampleKind::BundledCsv {
                bytes,
                dest_filename,
            }
            | SampleKind::BundledSqlite {
                bytes,
                dest_filename,
            } => {
                let path = match crate::sample_data::ensure_bundled_extracted(
                    state_root,
                    bytes,
                    dest_filename,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        crate::error_ux::push(crate::error_ux::Banner::error(
                            "Sample extract failed",
                            e.to_string(),
                        ));
                        return;
                    }
                };
                cx.spawn(async move |weak_shell, async_cx| {
                    let outcomes = handle_drop(vec![path], session).await;
                    Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
                })
                .detach();
            }
            SampleKind::Remote {
                url,
                sha256,
                dest_filename,
                ..
            } => {
                let state_root = state_root.to_owned();
                cx.spawn(
                    async move |weak_shell, async_cx| match crate::sample_data::fetch_remote(
                        url,
                        sha256,
                        &state_root,
                        dest_filename,
                    )
                    .await
                    {
                        Ok(path) => {
                            let outcomes = handle_drop(vec![path], session).await;
                            Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
                        }
                        Err(ref e) => {
                            crate::error_ux::push(crate::sample_data::fetch_failed_banner(url, e));
                        }
                    },
                )
                .detach();
            }
        }
    }

    /// Open a recent workspace or package entry (P11a T3).
    ///
    /// - `Workspace` entries use `open_workspace_at` (opens / focuses the
    ///   workspace window).
    /// - `Package` entries use `open_package_at` (read-only Inspect window).
    ///
    /// Wired by T4 hero recents list.  `Context<Self>` derefs to `App` so the
    /// free-function calls below compile without an explicit cast.
    pub(crate) fn open_recent_entry(
        &mut self,
        entry: crate::recents::RecentEntry,
        cx: &mut Context<Self>,
    ) {
        use crate::recents::RecentEntry;
        let path = entry.path().to_owned();
        match entry {
            RecentEntry::Workspace { .. } => open_workspace_at(cx, path),
            RecentEntry::Package { .. } => open_package_at(cx, path),
        }
    }

    /// Show the native file picker and open the chosen file (P11a T3).
    ///
    /// Equivalent to dropping a file onto the shell: `prompt_for_paths`
    /// → `handle_drop` → data-source refresh.  Mirrors `drop_listener`.
    ///
    /// Wired by T4 hero "Open file…" button.
    pub(crate) fn open_file_picker(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let session = Arc::clone(&self.session);
        cx.spawn(async move |weak_shell, async_cx| {
            let path = match rx.await {
                Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
                _ => return,
            };
            let outcomes = handle_drop(vec![path], session).await;
            Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
        })
        .detach();
    }

    /// Shared post-[`handle_drop`] outcome routing used by the three hero open
    /// helpers (P11a T3).
    ///
    /// Mirrors the inner `cx.spawn` body of `drop_listener` exactly:
    /// partitions outcomes into wizard requests and registered tables, opens
    /// any import wizard dialogs, then promotes the last registered table into
    /// the active data source with a full view refresh (`set_data_source` +
    /// `refresh_catalog` + `cx.notify()`).
    async fn route_drop_outcomes(
        outcomes: Vec<DropOutcome>,
        weak_shell: gpui::WeakEntity<WorkspaceShell>,
        async_cx: &mut gpui::AsyncApp,
    ) {
        let mut wizard_requests: Vec<(std::path::PathBuf, crate::import_wizard::SniffSummary)> =
            Vec::new();
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
                                let quoted = format!("\"{}\"", table_name.replace('"', "\"\""));
                                view.view_model = Some(ViewModel::new(table_name.clone(), quoted));
                                view.set_data_source(Arc::new(ds));
                                view.refresh_catalog(cx);
                                cx.notify();
                            });
                        });
                    }
                    Err(e) => {
                        tracing::warn!("hero open: GridDataSource::new failed: {e}");
                    }
                }
            }
        }
    }
}

/// Which AI field the entry modal is collecting (P9c-1 T9).
#[derive(Debug, Clone, Copy)]
enum AiEntryKind {
    Key,
    Model,
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

    /// Get (lazily creating, once) the stable focus handle for hero button `id`.
    /// Handles live on the persistent `WorkspaceShell` (not the transient
    /// `EmptyState`, which is rebuilt every render), so a focused hero control
    /// keeps focus across the harness's forced re-render (Slice 6).
    fn hero_focus_handle(&mut self, id: &'static str, cx: &mut gpui::App) -> gpui::FocusHandle {
        self.hero_focus
            .entry(id)
            .or_insert_with(|| cx.focus_handle())
            .clone()
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

        // PD-021 banner host: drain any globally-stashed banners into this
        // window's live list, then build an OWNED host element. Computing
        // `banner_host` here (after the `&mut self.banners` drain, before the
        // builder chain) keeps the `self.banners.iter()` borrow from outliving
        // the later `&mut self` mutations in this render.
        crate::error_ux::banner::merge_pending(&mut self.banners);
        let banner_host: Option<gpui::AnyElement> = (!self.banners.is_empty()).then(|| {
            gpui::div()
                .flex()
                .flex_col()
                .gap_1()
                .p_1()
                .children(
                    self.banners
                        .iter()
                        .map(|b| crate::error_ux::banner::render_banner(b).into_any_element()),
                )
                .into_any_element()
        });

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
                    Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
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
                // One config_dir() call feeds both recents and the
                // first_run_done read. On any error (config dir unavailable
                // OR settings parse failure) both default conservatively:
                // recents=empty, first_run_done=true (suppresses tour).
                let (recents_empty, first_run_done) = match crate::platform::config_dir() {
                    Ok(cfg) => {
                        let re = Recents::with_path(cfg.join("recents.json"))
                            .list()
                            .is_empty();
                        let frd = crate::settings::store::SettingsStore::with_path(
                            cfg.join("settings.toml"),
                        )
                        .load_or_default()
                        .map(|s| s.first_run_done)
                        .unwrap_or(true); // suppress tour on load error
                        (re, frd)
                    }
                    Err(_) => (true, true),
                };

                // One-shot auto-open: schedule the tour exactly once per
                // process. `tour_auto_shown` is set SYNCHRONOUSLY before
                // scheduling so that subsequent render frames (which
                // re-enter this branch before `first_run_done` persists)
                // cannot re-queue a second open. The dispatcher hop defers
                // `onboarding::open` out of the render frame, mirroring the
                // `about::open` pattern (`window_registry::dispatcher()` +
                // `dispatcher.dispatch`).
                if !self.tour_auto_shown && crate::empty_state::should_auto_tour(first_run_done) {
                    self.tour_auto_shown = true;
                    if let Some(dispatcher) = crate::window_registry::dispatcher() {
                        let _ = dispatcher.dispatch(|cx: &mut gpui::App| {
                            crate::onboarding::open(cx);
                        });
                    }
                }

                // Pre-register the stable per-hero-button focus handles on the
                // persistent shell, then hand them down to the transient
                // `EmptyState` (which must NOT mint focus handles — it is rebuilt
                // every frame, so a fresh handle each render would lose focus on
                // the harness's forced re-render). Slice 6. Registering all five
                // fixed ids unconditionally is fine — `HeroHandles::get` is only
                // invoked by whichever branch actually renders (`sample_column`
                // looks up `hero-open-file-samples`, `recents_column` looks up
                // `hero-open-file-recents`; only one of the two ever runs per
                // frame), so both branches always find their handles pre-registered.
                let hero_ids: [&'static str; 5] = [
                    "hero-take-tour",
                    "hero-open-demo",
                    "hero-open-file-samples",
                    "hero-open-file-recents",
                    "recents-list",
                ];
                let mut map = std::collections::HashMap::new();
                for id in hero_ids {
                    map.insert(id, self.hero_focus_handle(id, cx));
                }
                for entry in crate::sample_data::entries() {
                    let id = crate::empty_state::sample_static_id(&entry.kind);
                    map.insert(id, self.hero_focus_handle(id, cx));
                }
                let hero = crate::empty_state::HeroHandles { map };
                EmptyState::new(recents_empty, first_run_done, self.recents_active)
                    .render(&hero, cx)
            }
        };

        // Slice 6 Task 3: is a REAL grid mounted this frame (as opposed to the
        // "Loading grid…" placeholder or the empty-state hero)? Mirrors the
        // `body` match's own "real Table mount" guard above exactly, so the
        // shell only becomes Tab-reachable while there is actually a grid to
        // navigate into — the empty-state hero has its OWN tab stops (Tasks
        // 1/1b), and turning the shell root into an extra, unlabeled tab stop
        // while the hero is showing would insert an unexpected stop into
        // `hero_tab_cycle_visits_every_button`'s asserted DOM-order cycle.
        let grid_visible = matches!(
            (self.data_source.as_ref(), self.table_state.as_ref()),
            (Some(ds), Some(_)) if !ds.is_empty()
        );

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

        // AI key/model entry overlay (P9c-1 T9). Mounted by `open_ai_entry_prompt`;
        // emits `NamePromptEvent` routed via the stored `ai_entry_prompt_sub`
        // subscription (Confirm → re-dispatch SetKey/SetModel, Cancel → dismiss).
        // Same top-centre placement as the other modals.
        let ai_entry_prompt_overlay: Option<gpui::AnyElement> =
            self.ai_entry_prompt.as_ref().map(|p| {
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

        // Slice 6 Task 3: make the shell root a genuine Tab stop, but ONLY
        // while `grid_visible` (real a11y fix — Tab must reach the grid so
        // the arrow keys below have somewhere to land; must NOT apply while
        // the empty-state hero is showing, per the module note above).
        //
        // Tab-index/tab-stop metadata MUST be set on the HANDLE itself, not
        // the element: `track_focus` marks this an EXPLICIT tracked handle,
        // and gpui's paint pass only copies an element's `.tab_index()` onto
        // an AUTO-created handle (div.rs `tracked_focus_handle.is_none()`
        // guard) — never onto one already supplied via `track_focus`. See
        // `a11y/mod.rs`'s `FocusStopExt` doc comment for the same T0 finding.
        // `tab_stop`/`tab_index` write into the handle's shared `FocusRef`
        // (keyed by `FocusId`), so any clone of `self.focus_handle` observes
        // the same update — explicitly setting `tab_stop(grid_visible)` on
        // EVERY render (rather than only ever setting it `true`) keeps the
        // flag correct if a workspace is later closed back to the hero
        // within the same window (data source cleared → `grid_visible`
        // flips back to `false`).
        let shell_focus_handle = self
            .focus_handle
            .clone()
            .tab_index(0)
            .tab_stop(grid_visible);

        // Catalog-tree slice: the panel container's stable focus handle (one
        // tab stop for the whole panel). Hoisted here — `hero_focus_handle`
        // needs `&mut self`, unavailable inside the `.children(..)` closures.
        let catalog_fh = self.hero_focus_handle("catalog-tree", cx);

        div()
            .id("workspace-shell")
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .track_focus(&shell_focus_handle)
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
            // ── Catalog panel toggle (P6a T7) ─────────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::CatalogToggle, _window, cx| {
                    ws.catalog_panel_visible = !ws.catalog_panel_visible;
                    // Refresh on open so the dock always shows fresh tables.
                    ws.refresh_catalog(cx);
                    // Persist the dock visibility (session v8 `ui`).
                    ws.persist_dock_ui();
                    cx.notify();
                },
            ))
            // ── Inspector panel toggle (P6a T9) ───────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::InspectorToggle, _window, cx| {
                    ws.inspector_panel_visible = !ws.inspector_panel_visible;
                    // Persist the dock visibility (session v8 `ui`).
                    ws.persist_dock_ui();
                    cx.notify();
                },
            ))
            // ── Charts panel toggle (P9a T7) ──────────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::ChartVisualize, _window, cx| {
                    ws.toggle_chart_panel(cx);
                },
            ))
            // ── AI panel toggle (P9c-1 T9) ────────────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::AiPanelToggle, _window, cx| {
                    ws.toggle_ai_panel(cx);
                },
            ))
            .on_key_down(key_handler)
            .on_click(click_to_focus)
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
            .on_drop::<ExternalPaths>(drop_listener)
            .children(banner_host)
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
                    // Catalog dock first → order is Catalog | Connections | body.
                    .children(self.catalog_panel_visible.then(|| {
                        div()
                            .w_64()
                            .border_r_1()
                            .child(crate::catalog::panel::render_catalog(
                                &self.catalog_tree,
                                &self.catalog_collapsed,
                                self.catalog_active,
                                &catalog_fh,
                                cx,
                            ))
                    }))
                    .children(self.connections_panel_visible.then(|| {
                        div().w_64().border_r_1().child(
                            crate::connections::panel::render_connections(&self.connections, cx),
                        )
                    }))
                    // AI panel left dock (P9c-1 T9) → … | Connections | AI | body.
                    .children(self.ai_panel_visible.then(|| {
                        div()
                            .w_64()
                            .border_r_1()
                            .child(crate::ai::panel::render_ai_panel(&self.ai_panel, cx))
                    }))
                    .child(div().flex_1().child(body))
                    // Inspector right dock last → Catalog | Connections | body | Inspector.
                    .children(self.inspector_panel_visible.then(|| {
                        div()
                            .w_72()
                            .border_l_1()
                            .child(crate::inspector::panel::render_inspector(
                                &self.inspector,
                                self.inspector_projection(),
                                cx,
                            ))
                    }))
                    // Charts right dock (P9a T7) → … | Inspector | Charts.
                    .children(self.chart_panel_visible.then(|| {
                        div()
                            .w(gpui::px(560.0))
                            .border_l_1()
                            .flex()
                            .flex_col()
                            .child(self.render_chart_toolbar(cx))
                            .child(crate::charts::panel::render_chart_body(
                                &self.chart_panel,
                                self.chart_image.clone(),
                                (520.0, 360.0),
                            ))
                    })),
            )
            .children(popover_overlay)
            .children(editor_overlay)
            .children(export_overlay)
            .children(name_prompt_overlay)
            .children(ai_entry_prompt_overlay)
            .children(saved_picker_overlay)
            .children(md_token_prompt_overlay)
            // Mount gpui-component's overlay layers (P7c T8). `Root::render`
            // paints ONLY `self.view`; it does NOT auto-mount the sheet/dialog
            // layers, so without these two lines `open_sheet_at` (the Recovery
            // Sheet) and `open_dialog` (the P7b conflict / same-machine modals +
            // the T6 live-refresh confirm) set their `active_*` state but paint
            // NOTHING. Pattern mirrors gpui-component's own `story/src/lib.rs`.
            // Both return `Option<impl IntoElement>` → `.children(...)`.
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
    }
}

// UAT (Charts save/persist/lineage slice) T0: test-only shims exposing the
// `pub(crate)` chart/inspector/catalog state needed by `tests/chart_uat_window.rs`
// (an integration-test crate, so it cannot see `pub(crate)` fields directly).
// Identity-gated behind `a11y-capture` — zero surface in release builds.
// Placed BEFORE any `#[cfg(test)] mod` in this file: clippy's
// `items-after-test-module` (under `-D warnings`) rejects items that follow a
// test module.
#[cfg(feature = "a11y-capture")]
impl WorkspaceShell {
    pub fn chart_bind_for_test(&mut self, source: String, cols: Vec<(String, String)>) {
        self.chart_panel.bind(source, cols);
        self.chart_panel_visible = true;
    }
    pub fn chart_set_axes_for_test(
        &mut self,
        chart_type: crate::charts::spec::ChartType,
        x: Option<String>,
        y: Option<String>,
        title: String,
    ) {
        self.chart_panel.spec.chart_type = chart_type;
        self.chart_panel.spec.x = x;
        self.chart_panel.spec.y = y;
        self.chart_panel.spec.title = title;
    }
    pub fn chart_visible_for_test(&self) -> bool {
        self.chart_panel_visible
    }
    pub fn chart_spec_for_test(&self) -> crate::charts::spec::ChartSpec {
        self.chart_panel.spec.clone()
    }
    pub fn save_named_chart_for_test(&mut self, name: String, cx: &mut Context<Self>) {
        self.save_named_chart(name, cx);
    }
    pub fn seed_catalog_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>) {
        self.catalog_tables = tables;
    }
    pub fn catalog_active_for_test(&self) -> usize {
        self.catalog_active
    }
    pub fn catalog_collapsed_for_test(&self) -> Vec<String> {
        let mut v: Vec<String> = self.catalog_collapsed.iter().cloned().collect();
        v.sort();
        v
    }
    /// Build the catalog tree DIRECTLY from seeded fakes and show the catalog dock.
    /// Bypasses `refresh_catalog`'s off-thread `get_tables` (window.rs:2999), which
    /// would clobber the fakes with the empty test engine's real (empty) tables.
    /// Seed an `md:`-origin `TableInfo` to populate the "Cloud" group.
    pub fn seed_catalog_tree_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>) {
        self.catalog_tree = crate::catalog::CatalogTree::build(&tables);
        self.catalog_panel_visible = true;
    }
    /// Show the Connections dock and hand back the `ConnectionManager` so the test
    /// can drive `set_md_status` / `set_md_test_result` / `set_md_databases` (all
    /// already `pub`). No live connection, token, or keychain touched.
    pub fn open_connections_for_test(&mut self) -> &mut crate::connections::ConnectionManager {
        self.connections_panel_visible = true;
        &mut self.connections
    }
    /// Build + show the SQL console, then seed the timing chip's elapsed + routing
    /// so the chip renders its routing suffix without a real query run. The console
    /// is lazily built by `toggle_sql_console` (needs `&mut Window`); `set_last_elapsed`
    /// (sql_console.rs:340) sets `last_elapsed_ms` + `last_routing`, which is all the
    /// chip's render gate `(running == false, Some(ms))` needs.
    pub fn seed_routing_chip_for_test(
        &mut self,
        ms: u64,
        routing: crate::connections::routing::Routing,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sql_console_visible {
            self.toggle_sql_console(window, cx);
        }
        if let Some(console) = self.sql_console.clone() {
            console.update(cx, |c, cx| c.set_last_elapsed(ms, routing, cx));
        }
    }
    pub fn seed_lineage_target_for_test(&mut self, name: String, cx: &mut Context<Self>) {
        self.inspector.set_target(name);
        self.recompute_lineage();
        self.inspector_panel_visible = true;
        cx.notify();
    }
    pub fn open_saved_chart_for_test(
        &mut self,
        name: String,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_saved_chart(name, window, cx);
    }
    /// Slice 6 Task 3: the grid's live active cell, read straight off the
    /// shell's own `SelectionModel` — there is no separate `GridView` entity;
    /// `selection` lives directly on `WorkspaceShell` (see the field above),
    /// lazily built once a non-empty data source is mounted (`render`).
    pub fn grid_active_cell_for_test(&self) -> crate::grid::selection::CellCoord {
        self.selection
            .as_ref()
            .expect(
                "grid_active_cell_for_test called with no SelectionModel mounted \
                 (no non-empty data source bound yet?)",
            )
            .active()
    }
    /// Test oracle for recents-list arrow nav (mirrors `grid_active_cell_for_test`).
    #[cfg(feature = "a11y-capture")]
    pub fn recents_active_for_test(&self) -> usize {
        self.recents_active
    }
}

#[cfg(test)]
mod tests {
    use super::{bare_table_name, paths_from_open_urls};
    use std::path::PathBuf;

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

    #[test]
    fn bare_table_name_strips_quotes_and_schema() {
        // ViewModel::base_table() is quoted + may be schema-qualified.
        assert_eq!(bare_table_name("\"main\".\"orders\""), "orders");
        // Bare quoted name (no schema qualifier).
        assert_eq!(bare_table_name("\"orders\""), "orders");
        // Already bare/unquoted — identity.
        assert_eq!(bare_table_name("orders"), "orders");
        // Embedded dots only appear as the schema separator in this layer, so
        // the last segment is the table; quotes are trimmed from both ends.
        assert_eq!(bare_table_name("\"my_db\".\"main\".\"sales\""), "sales");
    }

    use super::{axis_field, axis_required, cycle_axis, set_axis_field};
    use crate::charts::spec::{AxisRole, ChartSpec, ChartType};

    fn spec(t: ChartType) -> ChartSpec {
        ChartSpec {
            chart_type: t,
            source: "\"t\"".into(),
            x: None,
            y: None,
            group: None,
            color: None,
            title: String::new(),
        }
    }

    #[test]
    fn required_axis_cycles_over_options_only() {
        let opts = vec!["a".to_string(), "b".to_string()];
        // None → first; a → b; b → wrap to a. Required never returns None.
        assert_eq!(cycle_axis(None, &opts, true), Some("a".into()));
        assert_eq!(cycle_axis(Some("a"), &opts, true), Some("b".into()));
        assert_eq!(cycle_axis(Some("b"), &opts, true), Some("a".into()));
        // Stale pick (not in opts) resets to the first option.
        assert_eq!(cycle_axis(Some("zzz"), &opts, true), Some("a".into()));
        // No options → None even when required (nothing to pick).
        assert_eq!(cycle_axis(None, &[], true), None);
    }

    #[test]
    fn optional_axis_passes_through_none() {
        let opts = vec!["a".to_string(), "b".to_string()];
        // None → a → b → None → a (None is a real step for optional dims).
        assert_eq!(cycle_axis(None, &opts, false), Some("a".into()));
        assert_eq!(cycle_axis(Some("a"), &opts, false), Some("b".into()));
        assert_eq!(cycle_axis(Some("b"), &opts, false), None);
    }

    #[test]
    fn value_axis_maps_to_the_field_each_type_reads() {
        // BoxPlot reads its value from spec.y; Heatmap from spec.color
        // (matches charts/query.rs build_plot_sql).
        let mut bx = spec(ChartType::BoxPlot);
        set_axis_field(&mut bx, AxisRole::Value, Some("amt".into()));
        assert_eq!(bx.y.as_deref(), Some("amt"));
        assert_eq!(bx.color, None);
        assert_eq!(axis_field(&bx, AxisRole::Value), Some("amt"));

        let mut hm = spec(ChartType::Heatmap);
        set_axis_field(&mut hm, AxisRole::Value, Some("cnt".into()));
        assert_eq!(hm.color.as_deref(), Some("cnt"));
        assert_eq!(hm.y, None);
        assert_eq!(axis_field(&hm, AxisRole::Value), Some("cnt"));
    }

    #[test]
    fn required_axes_classification() {
        assert!(axis_required(AxisRole::X));
        assert!(axis_required(AxisRole::Y));
        assert!(axis_required(AxisRole::Value));
        assert!(!axis_required(AxisRole::Group));
        assert!(!axis_required(AxisRole::Color));
    }
}

#[cfg(test)]
mod live_refresh_tests {
    //! Pure decision tests for the live re-import flow (P7c T6). The clickable
    //! Dialog (`confirm_discard`) and the engine round-trip are UAT — these cover
    //! the two pure gates: (1) whether `split_replayable().has_dropped()` drives
    //! the confirm prompt, and (2) the schema-drift column-existence guard.
    use super::partition_replay_on_drift;
    use dat0_engine::transform::{
        CellEdit, RowKey, Scalar, SortDirection, SortKey, Transformation, split_replayable,
    };

    fn one_cell_edit() -> Transformation {
        Transformation::Edit {
            cells: vec![CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "amount".into(),
                value: Scalar::Int(42),
            }],
        }
    }

    fn sort_on(col: &str) -> Transformation {
        Transformation::Sort {
            keys: vec![SortKey {
                column: col.into(),
                direction: SortDirection::Asc,
            }],
        }
    }

    /// The confirm dialog must fire iff the stack carries rowid-keyed ops
    /// (`Edit`/`RowDelete`) — those can't survive a re-CTAS. A column-keyed-only
    /// stack (e.g. a lone Sort) refreshes silently.
    #[test]
    fn refresh_needs_confirm_only_when_rowid_ops_present() {
        // One real cell edit + a sort → has dropped (Edit), confirm required.
        let mixed = vec![one_cell_edit(), sort_on("amount")];
        let split = split_replayable(&mixed);
        assert!(
            split.has_dropped(),
            "an Edit in the stack must require confirm"
        );
        assert_eq!(split.dropped_edits, 1);
        assert_eq!(split.dropped_deletes, 0);
        // The Sort survives into the replayable set.
        assert_eq!(split.replayable, vec![sort_on("amount")]);

        // Sort-only stack → nothing dropped, no confirm.
        let pure = vec![sort_on("amount")];
        let split = split_replayable(&pure);
        assert!(
            !split.has_dropped(),
            "a column-keyed-only stack refreshes without a prompt"
        );
    }

    /// The schema-drift guard: a Filter/Sort referencing a column the re-imported
    /// file no longer has → drop to bare base (empty ops) + drift flag. Present
    /// columns replay unchanged.
    #[test]
    fn schema_drift_lands_on_bare_base_when_column_missing() {
        let columns = vec!["id".to_string(), "amount".to_string()];

        // Sort on a present column → kept, no drift.
        let (ops, drifted) = partition_replay_on_drift(vec![sort_on("amount")], &columns);
        assert!(!drifted);
        assert_eq!(ops, vec![sort_on("amount")]);

        // Filter on a now-missing column → drift, land on bare base.
        let filter_missing = Transformation::Filter {
            column: "removed_col".into(),
            op: dat0_engine::transform::FilterOp::IsNotEmpty,
            value: dat0_engine::transform::FilterValue::None,
        };
        let (ops, drifted) = partition_replay_on_drift(vec![filter_missing], &columns);
        assert!(drifted, "a filter on a dropped column is schema drift");
        assert!(ops.is_empty(), "drift lands on the bare base (no ops)");

        // Sort key on a missing column → drift too.
        let (ops, drifted) = partition_replay_on_drift(vec![sort_on("gone")], &columns);
        assert!(drifted);
        assert!(ops.is_empty());
    }

    /// Display-only projection ops never reach the executed SQL, so a stale
    /// column reference in them is NOT drift (the grid fold ignores unknowns).
    #[test]
    fn projection_ops_referencing_missing_columns_are_not_drift() {
        let columns = vec!["id".to_string()];
        let rename_stale = Transformation::Rename {
            column: "old_name".into(), // not in `columns`
            to: "shiny".into(),
        };
        let (ops, drifted) = partition_replay_on_drift(vec![rename_stale.clone()], &columns);
        assert!(!drifted, "display-only ops can't cause an engine error");
        assert_eq!(ops, vec![rename_stale]);
    }
}
