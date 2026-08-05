//! The `.dat0` package surface (P8): export, open, unpack, and replay, plus
//! the demo workspace and the orphan / recovery scratch scans.
//!
//! `PACKAGE_BUDGET` caps the in-memory size a package read may claim; it
//! lives here rather than with the dock sizing consts because it is a
//! package concern, not a layout one.

use super::*;

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
pub(super) fn count_orphan_scratch(scratch_root: &std::path::Path) -> usize {
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
