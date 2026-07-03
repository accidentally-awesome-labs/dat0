//! Update prompt UI — off-thread fetch, dispatcher post-back, install/restart
//! or browser-nudge flow, launch-check gating, and "Check for Updates" menu
//! action handler.
//!
//! The pure helpers (`should_check_on_launch`, `prompt_action_for`) are unit-
//! tested. The GPUI flow (`run_update_flow`) is UAT-owed: it drives real network,
//! file I/O, and GPUI dialogs — none of which are reachable from the headless
//! test runner. See the UAT-owed list in the task report.

use gpui::App;

use crate::a11y::{A11yExt as _, AccessRole};

// ---------------------------------------------------------------------------
// Pure helpers (TDD-gated)
// ---------------------------------------------------------------------------

/// Whether to fire the background update check at launch.
///
/// A thin policy seam: currently `auto_check` is returned directly, but the
/// function gives us a single point to add future conditions (e.g., last-check
/// timestamp, beta-opt-in) without touching callers.
pub fn should_check_on_launch(auto_check: bool) -> bool {
    auto_check
}

/// Decision returned by `prompt_action_for`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPath {
    /// We can write to the install root → download + apply + relaunch in-process.
    Swap,
    /// Install root is read-only → open the browser Releases page instead.
    Nudge,
}

/// Choose the install path based on writability of the install root.
///
/// `writable` = `apply::is_writable(&apply::install_root().unwrap_or_default())`.
/// Callers should evaluate this off the main thread and pass the result here.
pub fn prompt_action_for(writable: bool) -> InstallPath {
    if writable {
        InstallPath::Swap
    } else {
        InstallPath::Nudge
    }
}

// ---------------------------------------------------------------------------
// GPUI flow
// ---------------------------------------------------------------------------

/// The human-facing GitHub Releases page opened by the Nudge path.
const RELEASES_PAGE_URL: &str = "https://github.com/accidentally-awesome-labs/dat0/releases/latest";

/// Kick the update check off-thread.  Called from:
/// - The `CheckForUpdates` menu action handler (manual trigger, `is_manual=true`).
/// - `run_app` on cold start when `should_check_on_launch` returns `true`
///   (automatic background check, `is_manual=false`).
///
/// Network + file-system work runs on a `std::thread::spawn` thread (ureq is
/// blocking; must never run on the GPUI main thread).  Results are posted back
/// via the process-wide `MainThreadDispatcher`, mirroring `about::open` and
/// `on_open_urls`.
///
/// Flow:
///
/// 1. Off-thread: `check::fetch_update` → `Option<AvailableUpdate>`.
/// 2. `None` (up-to-date): post back "up to date" dialog (silent on background check).
/// 3. `Some`: post back prompt dialog ("Install & Restart" / "Later").
///    - "Install & Restart": check writability → Swap (download+apply+relaunch)
///      or Nudge (open browser).
///    - "Later": close dialog, no-op.
///
/// The `is_manual` flag gates user-visible feedback for checking/up-to-date/failed
/// states so the background launch-check stays silent unless an update is found.
pub fn run_update_flow(cx: &mut App, is_manual: bool) {
    // On the manual path, show an immediate "checking…" dialog so the user
    // knows something is happening.  Background path stays silent.
    if is_manual {
        show_alert_dialog(cx, dat0_i18n::t("update.checking"));
    }

    std::thread::spawn(move || {
        let current = crate::about::build_info::BuildInfo::current().version;
        let result = crate::update::check::fetch_update(
            crate::update::MANIFEST_URL,
            crate::update::MANIFEST_SIG_URL,
            crate::update::manifest::EMBEDDED_PUBKEY,
            current,
        );

        let Some(d) = crate::window_registry::dispatcher() else {
            tracing::warn!("update::ui: dispatcher not installed; cannot post result");
            return;
        };

        match result {
            Err(e) => {
                tracing::debug!(error = %e, "update check failed (non-fatal)");
                let msg = e.to_string();
                let _ = d.dispatch(move |cx: &mut App| {
                    show_error_banner(cx, is_manual, &msg);
                });
            }
            Ok(None) => {
                tracing::debug!("update check: already up to date");
                let _ = d.dispatch(move |cx: &mut App| {
                    show_up_to_date(cx, is_manual);
                });
            }
            Ok(Some(update)) => {
                tracing::info!(version = %update.version, "update available");
                let _ = d.dispatch(move |cx: &mut App| {
                    show_update_prompt(cx, update);
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Main-thread rendering helpers (called via dispatcher post-back)
// ---------------------------------------------------------------------------

/// Present an "up to date" notification.  Silent on the background launch-check
/// (`is_manual=false`); shown only when the user explicitly chose "Check for Updates".
fn show_up_to_date(cx: &mut App, is_manual: bool) {
    if is_manual {
        show_alert_dialog(cx, dat0_i18n::t("update.up_to_date"));
    }
}

/// Present an error dialog when the network/verify step failed.
///
/// On the manual path (`is_manual=true`) the user clicked "Check for Updates"
/// and MUST see that it failed — silence would be confusing.  On the background
/// path, a failed launch-check is non-fatal and we stay silent (same policy as
/// `about::open`'s Err branch).
fn show_error_banner(cx: &mut App, is_manual: bool, msg: &str) {
    if is_manual {
        let title = format!("{}: {}", dat0_i18n::t("update.failed"), msg);
        show_alert_dialog(cx, title);
    } else {
        tracing::debug!(error = %msg, "update::ui: background check failed (non-fatal, silent)");
    }
}

/// Present the "Update available" prompt with Install & Restart / Later.
fn show_update_prompt(cx: &mut App, update: crate::update::AvailableUpdate) {
    use gpui::{AnyView, ParentElement as _, Window, div};
    use gpui_component::WindowExt as _;
    use gpui_component::dialog::{Dialog, DialogButtonProps};

    let title = format!("{} {}", dat0_i18n::t("update.available"), update.version);
    let install_restart = dat0_i18n::t("update.install_restart");
    let later = dat0_i18n::t("update.later");

    // Clone artifact before moving into the closure.
    let artifact = update.artifact.clone();

    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, move |_root: AnyView, window: &mut Window, cx| {
            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                let artifact_for_ok = artifact.clone();
                dialog
                    .title(title.clone())
                    // Test-only content seam: carry the "Update available:
                    // {version}" line (which otherwise lives only in the
                    // a11y-invisible title) as an `.a11y_label` node so the
                    // headless UAT can assert the version. Identity no-op in
                    // release; the visible "downloading…" placeholder is unchanged.
                    .child(
                        div()
                            .child(dat0_i18n::t("update.downloading"))
                            .a11y_label(AccessRole::Label, title.clone()),
                    )
                    .confirm()
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text(install_restart.clone())
                            .cancel_text(later.clone()),
                    )
                    .on_ok(move |_ev, _window, _cx| {
                        // Spawn the install on a background thread to avoid blocking the UI.
                        let artifact = artifact_for_ok.clone();
                        std::thread::spawn(move || {
                            perform_install(artifact);
                        });
                        true // close dialog
                    })
                    .on_cancel(|_ev, _window, _cx| true) // "Later" — just close
            });
        });
    } else {
        tracing::warn!("update::ui: no active window; cannot show update prompt");
    }
}

/// Show a simple alert dialog (single OK button).
fn show_alert_dialog(cx: &mut App, title: String) {
    use gpui::{AnyView, ParentElement as _, Window, div};
    use gpui_component::WindowExt as _;
    use gpui_component::dialog::Dialog;

    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, move |_root: AnyView, window: &mut Window, cx| {
            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                dialog
                    .title(title.clone())
                    // Test-only content seam: an inert child that emits the
                    // title text as an `.a11y_label` node so the headless UAT
                    // harness can read it. Covers `checking` / `up_to_date` /
                    // `failed` (all route through `show_alert_dialog`). Identity
                    // no-op in release. `has_active_dialog` already asserts
                    // presence; this makes the CONTENT assertable.
                    .child(div().a11y_label(AccessRole::Label, title.clone()))
                    .alert()
                    .on_ok(|_ev, _w, _cx| true)
            });
        });
    } else {
        tracing::debug!("update::ui: no active window for alert dialog");
    }
}

/// Perform the actual install on a background thread (already off main).
///
/// Determines writability → `InstallPath::Swap` or `::Nudge` → executes.
/// On `Swap`: download + `apply_update` + `relaunch` (does not return on success).
/// On `Nudge`: post back to main thread to open the browser.
fn perform_install(artifact: crate::update::manifest::ArtifactEntry) {
    let install = match crate::update::apply::install_root() {
        Some(p) => p,
        None => {
            tracing::warn!("update::ui: install_root() returned None; falling back to nudge");
            post_nudge();
            return;
        }
    };

    let writable = crate::update::apply::is_writable(&install);
    match prompt_action_for(writable) {
        InstallPath::Nudge => {
            tracing::info!("update: install root not writable; opening browser");
            post_nudge();
        }
        InstallPath::Swap => {
            // Download into a temp dir adjacent to the install root so rename
            // is same-filesystem (required for atomic swap on macOS).
            let dest_dir = match install.parent() {
                Some(p) => p.to_path_buf(),
                None => std::env::temp_dir(),
            };

            tracing::info!("update: downloading artifact…");

            // Post "downloading" feedback to main thread (non-blocking).
            if let Some(d) = crate::window_registry::dispatcher() {
                let _ = d.dispatch(|cx: &mut App| {
                    let _ = cx; // future: update a progress indicator
                });
            }

            let downloaded = match crate::update::download::download_verified(
                &artifact,
                &dest_dir,
                |done, total| {
                    tracing::trace!(done, total, "update: download progress");
                },
            ) {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, "update: download failed");
                    if let Some(d) = crate::window_registry::dispatcher() {
                        let msg = e.to_string();
                        let _ = d.dispatch(move |cx: &mut App| {
                            // perform_install is always user-initiated (Install & Restart click)
                            show_error_banner(cx, true, &msg);
                        });
                    }
                    return;
                }
            };

            tracing::info!("update: applying…");
            if let Err(e) = crate::update::apply::apply_update(&install, &downloaded) {
                tracing::error!(error = %e, "update: apply failed");
                let _ = std::fs::remove_file(&downloaded);
                if let Some(d) = crate::window_registry::dispatcher() {
                    let msg = e.to_string();
                    let _ = d.dispatch(move |cx: &mut App| {
                        // perform_install is always user-initiated (Install & Restart click)
                        show_error_banner(cx, true, &msg);
                    });
                }
                return;
            }

            tracing::info!("update: relaunching…");
            crate::update::apply::relaunch(&install); // does not return on success
        }
    }
}

/// Post a closure to the main thread that opens the browser Releases page.
fn post_nudge() {
    if let Some(d) = crate::window_registry::dispatcher() {
        let _ = d.dispatch(|_cx: &mut App| {
            if let Err(e) = crate::platform::open_url(RELEASES_PAGE_URL) {
                tracing::warn!(error = %e, "update::ui: open releases page failed");
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Unit tests — pure helpers only (GPUI flow is UAT-owed)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- should_check_on_launch -------------------------------------------

    #[test]
    fn should_check_on_launch_true_when_enabled() {
        assert!(should_check_on_launch(true));
    }

    #[test]
    fn should_check_on_launch_false_when_disabled() {
        assert!(!should_check_on_launch(false));
    }

    // ---- prompt_action_for -----------------------------------------------

    #[test]
    fn prompt_action_writable_yields_swap() {
        assert_eq!(prompt_action_for(true), InstallPath::Swap);
    }

    #[test]
    fn prompt_action_not_writable_yields_nudge() {
        assert_eq!(prompt_action_for(false), InstallPath::Nudge);
    }
}

/// Test-only shims: drive the main-thread render helpers directly (bypassing the
/// off-thread `run_update_flow`/`perform_install`) so the a11y harness can assert
/// each dialog's content, the `is_manual` gating, and dismissal. Feature-gated →
/// zero release footprint.
#[cfg(feature = "a11y-capture")]
pub fn show_alert_dialog_for_test(cx: &mut App, title: String) {
    show_alert_dialog(cx, title);
}

#[cfg(feature = "a11y-capture")]
pub fn show_up_to_date_for_test(cx: &mut App, is_manual: bool) {
    show_up_to_date(cx, is_manual);
}

#[cfg(feature = "a11y-capture")]
pub fn show_error_banner_for_test(cx: &mut App, is_manual: bool, msg: &str) {
    show_error_banner(cx, is_manual, msg);
}

#[cfg(feature = "a11y-capture")]
pub fn show_update_prompt_for_test(cx: &mut App, update: crate::update::AvailableUpdate) {
    show_update_prompt(cx, update);
}
