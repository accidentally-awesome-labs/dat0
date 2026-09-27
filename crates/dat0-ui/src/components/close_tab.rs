//! Confirm-discard for closing a data tab.
//!
//! A tab's cell edits and deleted rows live in its view, not in its table, and
//! the view closes with the tab (PD-038). That loss is confirmed, with the
//! counts, and only that: a view of sorts and filters closes at once, for the
//! reason Live Refresh gives — a prompt on every close trains the click that
//! skips the one that matters.

use dioxus::prelude::*;

use crate::a11y::AccessRole;

/// Not dismissable by the scrim.
///
/// Cancel and "Close anyway" are not the same outcome, and a stray click
/// outside a confirmation must not resolve to either. Escape and the host's ✕
/// mean Cancel; nothing else decides.
pub const SCRIM_DISMISSABLE: bool = false;

/// The header title the modal host should render above [`CloseTabConfirm`].
pub fn title() -> String {
    dat0_i18n::t("tab.close.confirm.title")
}

/// The explanation: which tab, and the exact counts of what will be lost.
pub fn body(tab: &str, edits: usize, deletes: usize) -> String {
    dat0_i18n::t("tab.close.confirm.body")
        .replace("{tab}", tab)
        .replace("{edits}", &edits.to_string())
        .replace("{deletes}", &deletes.to_string())
}

#[derive(Clone, PartialEq, Props)]
pub struct CloseTabConfirmProps {
    /// The tab's title, as its strip shows it.
    pub tab: String,
    /// Cells edited in the tab's view.
    pub edits: usize,
    /// Rows deleted in the tab's view.
    pub deletes: usize,
    /// Close the tab, losing the edits.
    pub on_confirm: EventHandler<()>,
    /// Keep the tab and its edits.
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn CloseTabConfirm(props: CloseTabConfirmProps) -> Element {
    let body = body(&props.tab, props.edits, props.deletes);

    rsx! {
        div { class: "d0-confirm", "data-a11y-id": "close-tab",

            p {
                class: "d0-body",
                "data-a11y-id": "close-tab-body",
                role: AccessRole::Label.aria(),
                "aria-label": "{body}",
                "{body}"
            }

            div { class: "d0-confirm-actions",
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "close-tab-cancel",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("common.cancel"),
                    onclick: move |_| props.on_cancel.call(()),
                    {dat0_i18n::t("common.cancel")}
                }
                button {
                    class: "d0-btn is-primary",
                    "data-a11y-id": "close-tab-confirm",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("tab.close.confirm.continue"),
                    onclick: move |_| props.on_confirm.call(()),
                    {dat0_i18n::t("tab.close.confirm.continue")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_names_the_tab_and_both_counts() {
        let b = body("sales.csv", 3, 7);
        assert!(b.contains("sales.csv"), "{b}");
        assert!(b.contains('3') && b.contains('7'), "{b}");
        assert!(
            !b.contains("{tab}") && !b.contains("{edits}") && !b.contains("{deletes}"),
            "{b}"
        );
    }
}
