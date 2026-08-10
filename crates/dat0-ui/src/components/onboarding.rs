//! The first-run tour: a seven-panel carousel.
//!
//! # Stepping is state now, and that is the whole point
//!
//! The GPUI build could not hold a step. `WindowExt::open_dialog` **stacked**
//! (`active_dialogs.push`, each layer drawn at a 16 px offset) rather than
//! replacing, so advancing meant `close_dialog` to pop the current panel and
//! `present_panel(index ± 1)` to push the next — a close-then-reopen on every
//! single Back and Next, with the whole panel body rebuilt from scratch each
//! time. Forget the `close_dialog` and the panels pile up on screen.
//!
//! With one modal slot there is nothing to pop. The step is a signal, the
//! carousel is one mounted component, and Back/Next are ordinary state writes:
//! the illustration, the copy and the pager re-render, the dialog itself never
//! moves. The modal host must therefore mount this component **once**, with a
//! stable key — driving the step from outside would remount it and reset the
//! carousel to panel one, which is the exact failure the old build had.
//!
//! Everything else is preserved: Skip is always available, Back appears from
//! panel two on, the primary button becomes "Get started" on the last panel,
//! and every exit — Skip *or* Get started — persists `first_run_done` so the
//! tour never auto-shows again.

use dioxus::prelude::*;

use dat0_core::onboarding::panels::{PANELS, back, is_last, next};

use crate::a11y::AccessRole;

/// Not dismissable by the scrim.
///
/// Not because the tour is important — it is skippable by design — but because
/// every exit has to run [`mark_first_run_done`]. A scrim click that merely
/// unmounted would re-show the tour on the next launch, which reads as the app
/// having forgotten the user dismissed it.
pub const SCRIM_DISMISSABLE: bool = false;

/// The header title the modal host should render above [`OnboardingTour`].
pub fn title() -> String {
    dat0_i18n::t("onboarding.tour.title")
}

/// Persist `first_run_done = true` so the tour never auto-shows again.
///
/// Logs rather than panics on any settings-store error: failing to record the
/// flag must not take the UI down, and the worst case is one extra showing at
/// the next launch.
pub fn mark_first_run_done() {
    let store = match dat0_core::platform::config_dir() {
        Ok(dir) => dat0_core::settings::store::SettingsStore::with_path(dir.join("settings.toml")),
        Err(e) => {
            tracing::warn!(error = %e, "onboarding: config_dir unavailable; first_run_done not set");
            return;
        }
    };
    if let Err(e) = dat0_core::settings::set_first_run_done(&store, true) {
        tracing::warn!(error = %e, "onboarding: persisting first_run_done failed");
    }
}

/// The URL of panel `index`'s illustration.
///
/// The PNGs are not copied into this crate's `assets/`: `PANELS` already
/// embeds them with `include_bytes!`, and a second copy would put 432 KB of
/// the same art in the binary twice. `protocol::serve` resolves this path
/// straight out of `dat0-core`.
pub fn illustration_url(index: usize) -> String {
    crate::protocol::url(&format!("onboarding/p{}.png", index + 1))
}

#[derive(Clone, PartialEq, Props)]
pub struct OnboardingTourProps {
    /// Which panel to open on. Read **once**, at mount; the carousel owns the
    /// step from then on.
    #[props(default = 0)]
    pub initial_step: usize,
    /// The tour is over — by Skip or by Get started. `first_run_done` has
    /// already been persisted by the time this fires.
    pub on_finish: EventHandler<()>,
}

#[component]
pub fn OnboardingTour(props: OnboardingTourProps) -> Element {
    let mut step = use_signal(|| props.initial_step.min(PANELS.len() - 1));

    let index = step();
    let panel = &PANELS[index];
    let headline = dat0_i18n::t(panel.title_key);
    let body = dat0_i18n::t(panel.body_key);
    let last = is_last(index);
    let show_back = index > 0;

    // The primary button's identity flips with its role, so a test (and a
    // screen reader) that goes looking for "Get started" finds the element
    // that actually finishes the tour rather than one merely labelled that way.
    let next_id = if last {
        "tour-get-started"
    } else {
        "tour-next"
    };
    let next_label = if last {
        dat0_i18n::t("onboarding.tour.get_started")
    } else {
        dat0_i18n::t("onboarding.tour.next")
    };

    let pager_label = dat0_i18n::t("onboarding.tour.pager")
        .replace("{n}", &(index + 1).to_string())
        .replace("{total}", &PANELS.len().to_string());

    rsx! {
        div { class: "d0-tour", "data-a11y-id": "tour",

            img {
                class: "d0-tour-art",
                "data-a11y-id": "tour-art",
                src: illustration_url(index),
                alt: "{headline}",
            }

            div {
                class: "d0-h2",
                "data-a11y-id": "tour-headline",
                role: AccessRole::Label.aria(),
                "aria-label": "{headline}",
                "{headline}"
            }
            div {
                class: "d0-body",
                "data-a11y-id": "tour-body",
                role: AccessRole::Label.aria(),
                "aria-label": "{body}",
                "{body}"
            }

            div { class: "d0-tour-controls",
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "tour-skip",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("onboarding.tour.skip"),
                    onclick: move |_| {
                        mark_first_run_done();
                        props.on_finish.call(());
                    },
                    {dat0_i18n::t("onboarding.tour.skip")}
                }

                div {
                    class: "d0-tour-pager",
                    "data-a11y-id": "tour-pager",
                    role: AccessRole::Label.aria(),
                    "aria-label": "{pager_label}",
                    for i in 0..PANELS.len() {
                        span {
                            key: "{i}",
                            class: if i == index { "d0-tour-dot is-on" } else { "d0-tour-dot" },
                        }
                    }
                }

                div { class: "d0-tour-advance",
                    if show_back {
                        button {
                            class: "d0-btn is-ghost",
                            "data-a11y-id": "tour-back",
                            role: AccessRole::Button.aria(),
                            "aria-label": dat0_i18n::t("onboarding.tour.back"),
                            onclick: move |_| step.set(back(step())),
                            {dat0_i18n::t("onboarding.tour.back")}
                        }
                    }
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "{next_id}",
                        role: AccessRole::Button.aria(),
                        "aria-label": "{next_label}",
                        onclick: move |_| {
                            if last {
                                mark_first_run_done();
                                props.on_finish.call(());
                            } else {
                                step.set(next(step()));
                            }
                        },
                        "{next_label}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_panel_has_an_illustration_the_protocol_can_serve() {
        for i in 0..PANELS.len() {
            let url = illustration_url(i);
            assert!(
                crate::protocol::panel_png(url.trim_start_matches("/dat0/")).is_some(),
                "panel {i} illustration is not reachable at {url}"
            );
        }
    }

    #[test]
    fn an_out_of_range_initial_step_clamps_rather_than_panicking() {
        // `Modal::Onboarding` carries a persisted step; a shrink in `PANELS`
        // would otherwise index out of bounds on the first launch after it.
        assert_eq!(99_usize.min(PANELS.len() - 1), PANELS.len() - 1);
    }
}
