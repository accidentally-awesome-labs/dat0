//! Banners: the persistent inline notice above the grid.
//!
//! The data — [`Banner`], the process-global pending queue, `push` /
//! `drain_pending` / `merge_pending` — lives in `dat0_core::error_ux::banner`.
//! This file is the markup, the action dispatch and the dismiss control.
//!
//! # Only an error interrupts
//!
//! The GPUI render gave every banner's title `AccessRole::Alert`, which asks a
//! screen reader to interrupt whatever it was saying. That is right for the one
//! kind the app treats as terminal — a failed session, where the window *is*
//! the failure until the user retries — and wrong for the other two, which are
//! notices you read when you get to them. Info and warning are `note`; error is
//! `alert`.
//!
//! # Dismissal
//!
//! `Banner::dismissible` has always been part of the data and was never
//! rendered: the GPUI banner had no ✕, so the only way a banner left the list
//! was a code path that retracted it by title (`live_refresh`). The flag now
//! draws a dismiss button, and a non-dismissible banner still has none — an
//! error you can close before reading is worse than no error.

use dioxus::prelude::*;

use dat0_core::error_ux::banner::{Banner, BannerKind};

use crate::a11y::AccessRole;

/// The ARIA role for a severity: only the interrupting one is an alert.
fn role_of(kind: BannerKind) -> AccessRole {
    match kind {
        BannerKind::Error => AccessRole::Alert,
        BannerKind::Info | BannerKind::Warning => AccessRole::Label,
    }
}

/// The severity modifier class.
fn class_of(kind: BannerKind) -> &'static str {
    match kind {
        BannerKind::Info => "d0-banner is-info",
        BannerKind::Warning => "d0-banner is-warning",
        BannerKind::Error => "d0-banner is-error",
    }
}

/// Every banner the window is showing, oldest first.
///
/// A host rather than a bare list because the ids have to carry the slot: two
/// banners are common (a live-data change plus an export failure), and a shared
/// `banner-act-primary` id would make the second one's button unreachable to
/// both a test and a screen reader walking by id.
#[derive(Clone, PartialEq, Props)]
pub struct BannerHostProps {
    pub banners: Vec<Banner>,
    /// An action button was pressed; the payload is its registry action id.
    pub on_action: EventHandler<String>,
    /// The ✕ was pressed; the payload is the banner's index in `banners`.
    pub on_dismiss: EventHandler<usize>,
}

#[component]
pub fn BannerHost(props: BannerHostProps) -> Element {
    if props.banners.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "d0-banner-host", "data-a11y-id": "banner-host",
            for (i, b) in props.banners.iter().enumerate() {
                BannerView {
                    key: "{i}",
                    slot: i,
                    banner: b.clone(),
                    on_action: props.on_action,
                    on_dismiss: props.on_dismiss,
                }
            }
        }
    }
}

#[derive(Clone, PartialEq, Props)]
pub struct BannerViewProps {
    /// Position in the host, so every id in this banner is unique.
    #[props(default = 0)]
    pub slot: usize,
    pub banner: Banner,
    pub on_action: EventHandler<String>,
    pub on_dismiss: EventHandler<usize>,
}

#[component]
pub fn BannerView(props: BannerViewProps) -> Element {
    let b = &props.banner;
    let slot = props.slot;
    let on_action = props.on_action;
    let on_dismiss = props.on_dismiss;

    rsx! {
        div {
            class: class_of(b.kind),
            "data-a11y-id": "banner-{slot}",
            role: role_of(b.kind).aria(),
            "aria-label": "{b.title}",

            div { class: "d0-banner-text",
                span { class: "d0-banner-title", "{b.title}" }
                if !b.body.is_empty() {
                    span {
                        class: "d0-mono is-muted",
                        "data-a11y-id": "banner-{slot}-body",
                        role: AccessRole::Label.aria(),
                        "aria-label": "{b.body}",
                        "{b.body}"
                    }
                }
            }

            // The button row only exists when an action does, so a title-only
            // banner keeps its single-line shape.
            if b.primary.is_some() || b.secondary.is_some() {
                div { class: "d0-banner-actions",
                    if let Some(a) = b.primary.as_ref() {
                        {
                            let id = a.action_id.clone();
                            rsx! {
                                button {
                                    class: "d0-btn is-primary",
                                    "data-a11y-id": "banner-{slot}-act-primary",
                                    role: AccessRole::Button.aria(),
                                    "aria-label": "{a.label}",
                                    tabindex: "0",
                                    onclick: move |_| on_action.call(id.clone()),
                                    "{a.label}"
                                }
                            }
                        }
                    }
                    if let Some(a) = b.secondary.as_ref() {
                        {
                            let id = a.action_id.clone();
                            rsx! {
                                button {
                                    class: "d0-btn is-ghost",
                                    "data-a11y-id": "banner-{slot}-act-secondary",
                                    role: AccessRole::Button.aria(),
                                    "aria-label": "{a.label}",
                                    tabindex: "0",
                                    onclick: move |_| on_action.call(id.clone()),
                                    "{a.label}"
                                }
                            }
                        }
                    }
                }
            }

            if b.dismissible {
                button {
                    class: "d0-banner-dismiss",
                    "data-a11y-id": "banner-{slot}-dismiss",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("common.close"),
                    tabindex: "0",
                    onclick: move |_| on_dismiss.call(slot),
                    "✕"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_error_interrupts() {
        assert_eq!(role_of(BannerKind::Error).aria(), "alert");
        assert_eq!(role_of(BannerKind::Warning).aria(), "note");
        assert_eq!(role_of(BannerKind::Info).aria(), "note");
    }

    /// Each severity needs its own accent, or the border-left rule that
    /// distinguishes them collapses to one colour.
    #[test]
    fn each_severity_has_its_own_class() {
        let mut classes = [
            class_of(BannerKind::Info),
            class_of(BannerKind::Warning),
            class_of(BannerKind::Error),
        ];
        classes.sort_unstable();
        let before = classes.len();
        let mut dedup = classes.to_vec();
        dedup.dedup();
        assert_eq!(dedup.len(), before);
    }
}
