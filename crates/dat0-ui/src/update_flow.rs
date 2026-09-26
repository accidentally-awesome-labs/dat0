//! Checking for updates (PD-023, step 5.7).
//!
//! Help → Check for Updates was built disabled, and nothing checked at
//! launch: the check (`update::check`), the prompt (`update_ui`) and the
//! installer (`update_ui::perform_install`) were all here, with nothing
//! calling them. [`check`] runs the check off the window's thread and shows
//! its answer in the update prompt: every answer when the user asked, only a
//! found update when the app checks by itself at launch
//! ([`update_ui::should_show`]). Install replaces dat0 where it can and opens
//! the Releases page where it cannot.

use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::update::manifest::ArtifactEntry;
use dat0_core::update::{self, AvailableUpdate};
use dat0_i18n::t;

use crate::components::modals::{ModalOutcome, ModalReply};
use crate::components::update_ui::{self, InstallOutcome, UpdateState};
use crate::state::{Modal, Workspace};

/// What a check found: an update, nothing newer, or why it failed.
pub type Found = anyhow::Result<Option<AvailableUpdate>>;

/// Check for an update against the release manifest, signed with the key
/// dat0 carries. `is_manual`: the user asked, so every answer is shown.
pub fn check(ws: Workspace, is_manual: bool) {
    check_with(ws, is_manual, || {
        update::check::fetch_update(
            update::MANIFEST_URL,
            update::MANIFEST_SIG_URL,
            update::manifest::EMBEDDED_PUBKEY,
            dat0_core::about::build_info::BuildInfo::current().version,
        )
    });
}

/// [`check`], with the fetch given: a test passes one that answers without
/// the network.
pub fn check_with(ws: Workspace, is_manual: bool, fetch: impl FnOnce() -> Found + Send + 'static) {
    if is_manual {
        show(ws, UpdateState::Checking, true);
    }
    spawn(async move {
        let state = match tokio::task::spawn_blocking(fetch).await {
            Ok(Ok(Some(found))) => UpdateState::Available {
                version: found.version,
                artifact: found.artifact,
            },
            Ok(Ok(None)) => UpdateState::UpToDate,
            Ok(Err(e)) => UpdateState::Failed(format!("{e:#}")),
            Err(e) => UpdateState::Failed(e.to_string()),
        };
        if update_ui::should_show(&state, is_manual) {
            show(ws, state, is_manual);
        }
    });
}

/// The check at launch: once per process, when the settings allow it.
pub fn use_launch_check(ws: Workspace) {
    use_hook(move || {
        static CHECKED: std::sync::Once = std::sync::Once::new();
        let auto = dat0_core::platform::config_dir()
            .ok()
            .and_then(|dir| {
                dat0_core::settings::store::SettingsStore::with_path(dir.join("settings.toml"))
                    .load_or_default()
                    .ok()
            })
            .is_none_or(|s| s.update_auto_check);
        if update_ui::should_check_on_launch(auto) {
            CHECKED.call_once(|| check(ws, false));
        }
    });
}

/// Show `state` in the update prompt. Another dialog up keeps its place, and
/// the answer is a banner instead.
fn show(ws: Workspace, state: UpdateState, is_manual: bool) {
    let mut modal = ws.modal;
    if !matches!(&*modal.peek(), None | Some(Modal::Update { .. })) {
        ws.push_banner(Banner::info(update_ui::title(&state)));
        return;
    }
    modal.set(Some(Modal::Update {
        state,
        is_manual,
        reply: ModalReply::new(move |outcome: ModalOutcome| {
            if let ModalOutcome::Install(artifact) = outcome {
                install(ws, artifact);
            }
        }),
    }));
}

/// Install `artifact` off the window's thread. A swap that succeeds ends in a
/// relaunch; one that fails, and a nudge to the Releases page, come back.
fn install(ws: Workspace, artifact: ArtifactEntry) {
    spawn(async move {
        let outcome =
            tokio::task::spawn_blocking(move || update_ui::perform_install(&artifact)).await;
        let failed = match outcome {
            Ok(InstallOutcome::Nudged) => return,
            Ok(InstallOutcome::Failed(why)) => why,
            Err(e) => e.to_string(),
        };
        ws.push_banner(Banner::error(t("update.failed"), failed));
    });
}
