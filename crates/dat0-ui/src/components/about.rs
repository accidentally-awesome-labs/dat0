//! The About box.
//!
//! Build identity, licence, acknowledgements and the "is there a newer
//! release" line. The rows themselves are unchanged — they come from
//! [`dat0_core::about::summary_lines`], because what About *says* is data and
//! only the chrome moved.
//!
//! The release check keeps the GPUI ordering, which is load-bearing: the box
//! paints immediately with "you're on the latest version" and flips to the
//! nudge + Download only when a strictly newer tag comes back. A modal that
//! awaits the network before its first paint is a modal that hangs whenever
//! GitHub is slow, and About is the one surface a user opens *because* they
//! suspect something is wrong.

use dioxus::prelude::*;

use dat0_core::about::build_info::BuildInfo;
use dat0_core::about::{RELEASES_PAGE_URL, summary_lines};
use dat0_core::update::{LATEST_RELEASE_API, fetch_latest, newer_than};

use crate::a11y::AccessRole;

/// The host's scrim may dismiss this. About is informational; losing it costs
/// the user nothing.
pub const SCRIM_DISMISSABLE: bool = true;

/// The header title the modal host should render above [`About`].
pub fn title() -> String {
    dat0_i18n::t("about.title")
}

#[derive(Clone, PartialEq, Props)]
pub struct AboutProps {
    /// A strictly-newer release tag, when the caller already knows one. Leave
    /// `None` and the component finds out for itself.
    #[props(default)]
    pub newer: Option<String>,
    /// Run the background release check.
    ///
    /// Defaults to the shipping behaviour. Turn it off when the caller has
    /// already resolved `newer`, or in a headless harness — the fetch is a
    /// blocking `ureq` call handed to `spawn_blocking`, which panics outright
    /// with no Tokio runtime under it.
    #[props(default = true)]
    pub check_latest: bool,
    pub on_close: EventHandler<()>,
}

#[component]
pub fn About(props: AboutProps) -> Element {
    let mut newer = use_signal(|| props.newer.clone());
    let check = props.check_latest;

    use_future(move || async move {
        // A caller-supplied answer wins: re-asking GitHub would only be able
        // to agree, and it would spend a network round trip doing it.
        if !check || newer.peek().is_some() {
            return;
        }
        let found = tokio::task::spawn_blocking(|| fetch_latest(LATEST_RELEASE_API).ok())
            .await
            .ok()
            .flatten();
        match found {
            Some(tag) if newer_than(BuildInfo::current().version, &tag) => newer.set(Some(tag)),
            Some(_) => {}
            // Non-fatal by design: a failed check must never turn the About
            // box into an error report.
            None => tracing::debug!("about: update check failed"),
        }
    });

    let newer = newer.read().clone();
    let lines = summary_lines(&BuildInfo::current(), newer.as_deref());
    // The whole body as one accessible name, matching what the GPUI dialog
    // announced — a reader should hear the version block as a unit, not as six
    // unrelated fragments.
    let body = lines.join("\n");

    rsx! {
        div { class: "d0-about", "data-a11y-id": "about",

            div {
                class: "d0-about-lines",
                "data-a11y-id": "about-body",
                role: AccessRole::Label.aria(),
                "aria-label": "{body}",
                for (i, line) in lines.iter().enumerate() {
                    div { key: "{i}", class: "d0-mono", "{line}" }
                }
            }

            div { class: "d0-about-actions",
                if newer.is_some() {
                    // `confirm()` in the GPUI build: Cancel beside a relabelled
                    // OK. Download opens the human Releases page — dat0 never
                    // self-updates from About; that is `update_ui`'s job.
                    button {
                        class: "d0-btn is-ghost",
                        "data-a11y-id": "about-cancel",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("common.cancel"),
                        onclick: move |_| props.on_close.call(()),
                        {dat0_i18n::t("common.cancel")}
                    }
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "about-download",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("about.update.download"),
                        onclick: move |_| {
                            open_releases_page();
                            props.on_close.call(());
                        },
                        {dat0_i18n::t("about.update.download")}
                    }
                } else {
                    // `alert()`: one button, nothing to decide.
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "about-ok",
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

/// Open the human Releases page in the user's browser.
///
/// Separate from the click handler so the URL has exactly one definition and
/// the update surface can reuse it for its Nudge path.
pub fn open_releases_page() {
    if let Err(e) = dat0_core::platform::open_url(RELEASES_PAGE_URL) {
        tracing::warn!(error = %e, "about: open releases page failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nudge_line_appears_only_with_a_newer_tag() {
        let b = BuildInfo::current();
        let up_to_date = summary_lines(&b, None).join("\n");
        let stale = summary_lines(&b, Some("v99.0.0")).join("\n");

        assert!(up_to_date.contains(&dat0_i18n::t("about.update.current")));
        assert!(!up_to_date.contains(&dat0_i18n::t("about.update.available")));
        assert!(stale.contains("v99.0.0"));
    }
}
