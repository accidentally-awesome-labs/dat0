//! Workspace lifecycle: opening, saving, promoting a loose file into a
//! `.dat0/` workspace, and the recents list.
//!
//! Also holds the two settings loaders (`load_workspace_settings`,
//! `configured_memory_budget`) that every open path consults, and
//! `maybe_prompt_save_workspace`, the one `WorkspaceShell` method in this
//! topic — which is why the module carries both free functions and an
//! `impl` block.

use super::*;

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
pub(super) fn configured_memory_budget() -> u64 {
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

impl WorkspaceShell {
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
}
