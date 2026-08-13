//! The update surface: check, prompt, install.
//!
//! # The gate that matters is `is_manual`
//!
//! The same check runs two ways. When the user picks "Check for Updates" they
//! are owed an answer, including "nothing to do" and "that failed". When the
//! app checks by itself at launch, only a *found* update is worth a modal —
//! a background check that pops "you're up to date" on every cold start is
//! noise, and one that pops "update check failed" because the network was down
//! is worse. [`should_show`] is that rule, and it is the whole of it.
//!
//! # Install is not one path
//!
//! Whether dat0 can replace itself depends on whether the install root is
//! writable — a `.app` in `/Applications` usually is, one inside a read-only
//! DMG or a system-managed directory is not. [`prompt_action_for`] decides:
//! `Swap` downloads, verifies, applies and relaunches; `Nudge` opens the
//! Releases page and lets the user do it. There is no third option and no
//! silent failure.

use dioxus::prelude::*;

use dat0_core::update::manifest::ArtifactEntry;
use dat0_core::update::{apply, download};

use crate::a11y::AccessRole;

/// Dismissable. Every state here is either informational or a "Later" away
/// from being dismissed anyway.
pub const SCRIM_DISMISSABLE: bool = true;

/// Whether to fire the background update check at launch.
///
/// A policy seam: `auto_check` passes straight through today, but this is the
/// one place a future condition (last-check timestamp, channel opt-in) goes
/// without touching either caller.
pub fn should_check_on_launch(auto_check: bool) -> bool {
    auto_check
}

/// How an available update gets installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPath {
    /// The install root is writable: download, apply and relaunch in-process.
    Swap,
    /// The install root is read-only: open the Releases page instead.
    Nudge,
}

/// Choose the install path from the writability of the install root.
///
/// `writable` must be evaluated off the main thread — [`apply::is_writable`]
/// probes by writing a file.
pub fn prompt_action_for(writable: bool) -> InstallPath {
    if writable {
        InstallPath::Swap
    } else {
        InstallPath::Nudge
    }
}

/// What the update flow has to say right now.
#[derive(Debug, Clone)]
pub enum UpdateState {
    /// A manual check is in flight.
    Checking,
    /// The check came back with nothing newer.
    UpToDate,
    /// The check itself failed — network, signature, or manifest.
    Failed(String),
    /// A newer release exists.
    Available {
        version: String,
        artifact: ArtifactEntry,
    },
}

/// [`ArtifactEntry`] is a deserialised manifest row and derives no `PartialEq`.
/// Its identity is its URL plus its digest, which is precisely what changes
/// when the artifact does, so comparing those two is not an approximation.
impl PartialEq for UpdateState {
    fn eq(&self, other: &Self) -> bool {
        use UpdateState::*;
        match (self, other) {
            (Checking, Checking) | (UpToDate, UpToDate) => true,
            (Failed(a), Failed(b)) => a == b,
            (
                Available {
                    version: va,
                    artifact: aa,
                },
                Available {
                    version: vb,
                    artifact: ab,
                },
            ) => va == vb && aa.url == ab.url && aa.sha256 == ab.sha256,
            _ => false,
        }
    }
}

/// Whether this state deserves a modal.
///
/// Only a found update interrupts a background check. Everything else is
/// feedback the user asked for by checking manually.
pub fn should_show(state: &UpdateState, is_manual: bool) -> bool {
    match state {
        UpdateState::Available { .. } => true,
        UpdateState::Checking | UpdateState::UpToDate | UpdateState::Failed(_) => is_manual,
    }
}

/// The header title the modal host should render above [`UpdatePrompt`].
pub fn title(state: &UpdateState) -> String {
    match state {
        UpdateState::Checking => dat0_i18n::t("update.checking"),
        UpdateState::UpToDate => dat0_i18n::t("update.up_to_date"),
        UpdateState::Failed(msg) => format!("{}: {}", dat0_i18n::t("update.failed"), msg),
        UpdateState::Available { version, .. } => {
            format!("{} {}", dat0_i18n::t("update.available"), version)
        }
    }
}

#[derive(Clone, PartialEq, Props)]
pub struct UpdatePromptProps {
    pub state: UpdateState,
    /// True when the user chose "Check for Updates" themselves.
    pub is_manual: bool,
    /// Begin the install. The host runs [`perform_install`] off the main
    /// thread — it downloads, applies and (on success) never returns.
    pub on_install: EventHandler<ArtifactEntry>,
    pub on_close: EventHandler<()>,
}

#[component]
pub fn UpdatePrompt(props: UpdatePromptProps) -> Element {
    // Latches once Install is pressed. The install runs off-thread and, when
    // it succeeds, ends in a relaunch — so the honest final state of this
    // dialog is "downloading", not a closed window.
    let mut installing = use_signal(|| false);

    if !should_show(&props.state, props.is_manual) {
        // The gate lives here as well as at the call site, so a host that
        // mounts unconditionally still cannot turn a silent background check
        // into a pop-up.
        return rsx! {};
    }

    let heading = title(&props.state);
    let body = if installing() {
        dat0_i18n::t("update.downloading")
    } else {
        heading.clone()
    };

    rsx! {
        div {
            class: "d0-update",
            "data-a11y-id": "update",

            p {
                class: "d0-body",
                "data-a11y-id": "update-body",
                role: AccessRole::Label.aria(),
                "aria-label": "{body}",
                "{body}"
            }

            div { class: "d0-update-actions",
                if let UpdateState::Available { artifact, .. } = &props.state {
                    button {
                        class: "d0-btn is-ghost",
                        "data-a11y-id": "update-later",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("update.later"),
                        disabled: installing(),
                        onclick: move |_| props.on_close.call(()),
                        {dat0_i18n::t("update.later")}
                    }
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "update-install",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("update.install_restart"),
                        disabled: installing(),
                        onclick: {
                            let artifact = artifact.clone();
                            move |_| {
                                installing.set(true);
                                props.on_install.call(artifact.clone());
                            }
                        },
                        {dat0_i18n::t("update.install_restart")}
                    }
                } else {
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "update-ok",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("common.ok"),
                        onclick: move |_| props.on_close.call(()),
                        {dat0_i18n::t("common.ok")}
                    }
                }
            }
        }
    }
}

/// The result of an install attempt that came back.
///
/// There is no `Succeeded`: a successful swap ends in
/// [`apply::relaunch`], which replaces the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The install root was not writable (or could not be located), so the
    /// Releases page was opened instead.
    Nudged,
    /// Download, verification or apply failed. The message is user-facing.
    Failed(String),
}

/// Download, verify, apply and relaunch — or nudge.
///
/// **Blocking. Must be run off the UI thread.** On the `Swap` path this
/// downloads an artifact, verifies its digest, replaces the install and execs
/// the new binary, so on success it does not return at all.
///
/// A failed apply deletes the download before reporting: leaving a verified
/// artifact next to the install is how a half-applied update gets picked up by
/// something else later.
pub fn perform_install(artifact: &ArtifactEntry) -> InstallOutcome {
    let Some(install) = apply::install_root() else {
        tracing::warn!("update: install_root() returned None; falling back to nudge");
        crate::components::about::open_releases_page();
        return InstallOutcome::Nudged;
    };

    match prompt_action_for(apply::is_writable(&install)) {
        InstallPath::Nudge => {
            tracing::info!("update: install root not writable; opening browser");
            crate::components::about::open_releases_page();
            InstallOutcome::Nudged
        }
        InstallPath::Swap => {
            // Download beside the install root so the final rename is
            // same-filesystem — a cross-device rename is not atomic, and a
            // non-atomic swap of a running app is how you get half an app.
            let dest_dir = install
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir);

            tracing::info!("update: downloading artifact…");
            let downloaded =
                match download::download_verified(artifact, &dest_dir, |done, total| {
                    tracing::trace!(done, total, "update: download progress");
                }) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!(error = %e, "update: download failed");
                        return InstallOutcome::Failed(e.to_string());
                    }
                };

            tracing::info!("update: applying…");
            if let Err(e) = apply::apply_update(&install, &downloaded) {
                tracing::error!(error = %e, "update: apply failed");
                let _ = std::fs::remove_file(&downloaded);
                return InstallOutcome::Failed(e.to_string());
            }

            tracing::info!("update: relaunching…");
            apply::relaunch(&install); // never returns on success
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> ArtifactEntry {
        ArtifactEntry {
            url: "https://example.invalid/dat0.tar.gz".into(),
            sha256: "0".repeat(64),
            size: 1,
        }
    }

    fn available() -> UpdateState {
        UpdateState::Available {
            version: "9.9.9".into(),
            artifact: artifact(),
        }
    }

    #[test]
    fn a_background_check_only_speaks_when_it_found_something() {
        assert!(!should_show(&UpdateState::Checking, false));
        assert!(!should_show(&UpdateState::UpToDate, false));
        assert!(!should_show(&UpdateState::Failed("no route".into()), false));
        assert!(should_show(&available(), false));
    }

    #[test]
    fn a_manual_check_always_answers() {
        for s in [
            UpdateState::Checking,
            UpdateState::UpToDate,
            UpdateState::Failed("no route".into()),
            available(),
        ] {
            assert!(should_show(&s, true), "{s:?} must be visible when manual");
        }
    }

    #[test]
    fn the_failure_title_carries_the_reason() {
        let t = title(&UpdateState::Failed("signature mismatch".into()));
        assert!(t.contains("signature mismatch"), "{t}");
        assert!(t.contains(&dat0_i18n::t("update.failed")), "{t}");
    }

    #[test]
    fn the_available_title_carries_the_version() {
        assert!(title(&available()).contains("9.9.9"));
    }

    #[test]
    fn writability_picks_the_install_path() {
        assert_eq!(prompt_action_for(true), InstallPath::Swap);
        assert_eq!(prompt_action_for(false), InstallPath::Nudge);
    }

    #[test]
    fn launch_check_follows_the_setting() {
        assert!(should_check_on_launch(true));
        assert!(!should_check_on_launch(false));
    }
}
