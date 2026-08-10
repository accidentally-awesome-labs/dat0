//! Confirm-discard for a live re-import.
//!
//! Re-importing a source file re-CTASes the base table, which regenerates the
//! `__dat0_rowid` surrogate. Row-keyed transforms — in-place cell edits and
//! row deletions — cannot be replayed against new rowids, so they are dropped.
//! Column-keyed transforms (filters, sorts, projection) survive.
//!
//! That drop is silent data loss unless it is confirmed, which is the only
//! reason this surface exists. It appears *only* when
//! `split_replayable(stack).has_dropped()` — a stack of filters and sorts
//! refreshes with no prompt at all, and adding one here would train users to
//! click through the one that matters.

use dioxus::prelude::*;

use crate::a11y::AccessRole;

/// Not dismissable by the scrim.
///
/// Cancel and "Refresh anyway" are not the same outcome, and a stray click
/// outside a confirmation must not resolve to either. Escape and the host's ✕
/// mean Cancel; nothing else decides.
pub const SCRIM_DISMISSABLE: bool = false;

/// The header title the modal host should render above [`LiveRefreshConfirm`].
pub fn title() -> String {
    dat0_i18n::t("livedata.refresh.confirm.title")
}

/// The explanation, with the exact counts of what will be lost.
///
/// Counts, not "some": a user deciding whether to lose work needs to know
/// whether it is one edit or four hundred.
pub fn body(dropped_edits: usize, dropped_deletes: usize) -> String {
    dat0_i18n::t("livedata.refresh.confirm.body")
        .replace("{edits}", &dropped_edits.to_string())
        .replace("{deletes}", &dropped_deletes.to_string())
}

#[derive(Clone, PartialEq, Props)]
pub struct LiveRefreshConfirmProps {
    /// Cell edits that will be discarded.
    pub dropped_edits: usize,
    /// Row deletions that will be discarded.
    pub dropped_deletes: usize,
    /// Proceed with the re-import, losing the row-keyed transforms.
    pub on_confirm: EventHandler<()>,
    /// Keep the edits; the file on disk stays un-re-imported.
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn LiveRefreshConfirm(props: LiveRefreshConfirmProps) -> Element {
    let body = body(props.dropped_edits, props.dropped_deletes);

    rsx! {
        div { class: "d0-confirm", "data-a11y-id": "live-refresh",

            p {
                class: "d0-body",
                "data-a11y-id": "live-refresh-body",
                role: AccessRole::Label.aria(),
                "aria-label": "{body}",
                "{body}"
            }

            div { class: "d0-confirm-actions",
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "live-refresh-cancel",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("common.cancel"),
                    onclick: move |_| props.on_cancel.call(()),
                    {dat0_i18n::t("common.cancel")}
                }
                button {
                    class: "d0-btn is-primary",
                    "data-a11y-id": "live-refresh-confirm",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("livedata.refresh.confirm.continue"),
                    onclick: move |_| props.on_confirm.call(()),
                    {dat0_i18n::t("livedata.refresh.confirm.continue")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_names_both_counts() {
        let b = body(3, 7);
        assert!(b.contains('3') && b.contains('7'), "{b}");
        assert!(!b.contains("{edits}") && !b.contains("{deletes}"), "{b}");
    }
}
