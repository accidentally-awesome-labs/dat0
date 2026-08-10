//! The empty-state hero: what a window shows when it has no open tabs.
//!
//! Two modes. **Plain** is the product statement over a two-column body — the
//! drop zone on the left, samples or recents on the right. **Enriched** (first
//! run only) prepends a value-prop band: the headline with a *Take a tour*
//! button, the featured demo workspace with the app's one amber CTA, and an
//! "or start from a sample" heading demoting the sample list.
//!
//! # The copy is present tense, deliberately
//!
//! The marketing page this design comes from carries honesty labels — "concept
//! render", "designed, not shipped", "in development". Inside the shipped app
//! they are false, so they are absent, and
//! `hero_copy_never_claims_the_product_is_unshipped` is the gate that keeps
//! them absent the next time someone copies that page verbatim.
//!
//! # State the component owns
//!
//! `recents_active` is local. Under GPUI it lived on the persistent
//! `WorkspaceShell` because the hero was a transient view rebuilt every frame;
//! a Dioxus component keeps its own signals across renders, and the hero only
//! exists while the window has no tabs — the moment it does, the index is
//! meaningless. Nothing else needs to read it, so nothing else holds it.

use dioxus::prelude::*;

use dat0_core::recents::RecentEntry;
use dat0_core::sample_data::{SampleKind, entries};

use crate::a11y::AccessRole;

/// Which hero variant to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroMode {
    Enriched,
    Plain,
}

/// First run ⇒ enriched.
pub fn hero_mode(first_run_done: bool) -> HeroMode {
    if first_run_done {
        HeroMode::Plain
    } else {
        HeroMode::Enriched
    }
}

/// The carousel auto-opens exactly once, on the very first run.
pub fn should_auto_tour(first_run_done: bool) -> bool {
    !first_run_done
}

/// Whether the first-run value-prop band renders above the base hero.
pub fn band_visible(mode: HeroMode) -> bool {
    matches!(mode, HeroMode::Enriched)
}

/// Stable `data-a11y-id` for a sample card.
///
/// The catalog is built at call time, so a card cannot intern its index; each
/// `SampleKind` discriminant maps to a fixed id instead. The exhaustive match is
/// safe because the catalog has exactly one entry per variant (Iris is the only
/// `BundledCsv`, Chinook the only `BundledSqlite`, NYC taxi the only `Remote`).
/// A second entry of the same variant would need a finer discriminant here —
/// `sample_ids_are_unique_per_entry` fails first if that happens.
pub fn sample_static_id(kind: &SampleKind) -> &'static str {
    match kind {
        SampleKind::BundledCsv { .. } => "hero-sample-iris",
        SampleKind::BundledSqlite { .. } => "hero-sample-chinook",
        SampleKind::Remote { .. } => "hero-sample-nyc-taxi",
    }
}

/// The recent entry the active index selects, or `None` when the index has
/// outrun the list.
///
/// Pure, and the reason keyboard activation and the active-row ring cannot
/// disagree: both read this.
pub fn active_recent(entries: &[RecentEntry], active: usize) -> Option<RecentEntry> {
    entries.get(active).cloned()
}

/// Every i18n key the hero's own copy renders.
///
/// A list rather than a scan so the honesty gate walks the whole surface, not
/// whichever key someone remembered. Sample and recent labels are excluded:
/// they name data, not the product.
pub const HERO_COPY_KEYS: &[&str] = &[
    "hero.title",
    "hero.subtitle",
    "hero.lead",
    "hero.drop",
    "hero.privacy",
    "hero.demo.heading",
    "hero.demo.cta",
    "hero.samples.heading",
    "hero.take_tour",
];

#[derive(Clone, PartialEq, Props)]
pub struct EmptyStateProps {
    /// The shell's cached `recents.json` snapshot. Empty ⇒ the sample picker
    /// shows instead of the recents list.
    #[props(default)]
    pub recents: Vec<RecentEntry>,
    /// From `settings.toml`. `false` ⇒ the first-run band.
    pub first_run_done: bool,
    /// The window's session is still opening. Swaps the drop copy for a
    /// placeholder — the hero stays a live drop target, it just stops promising
    /// the "no waiting" it cannot deliver for another few hundred milliseconds.
    #[props(default = false)]
    pub booting: bool,
    pub on_open_sample: EventHandler<SampleKind>,
    pub on_open_recent: EventHandler<RecentEntry>,
    pub on_open_file: EventHandler<()>,
    pub on_take_tour: EventHandler<()>,
    pub on_open_demo: EventHandler<()>,
}

#[component]
pub fn EmptyState(props: EmptyStateProps) -> Element {
    let band = band_visible(hero_mode(props.first_run_done));
    let on_take_tour = props.on_take_tour;
    let on_open_demo = props.on_open_demo;

    rsx! {
        div { class: "d0-hero", "data-a11y-id": "empty-state",

            if band {
                div { class: "d0-hero-band",
                    div { class: "d0-hero-band-top",
                        {headline()}
                        button {
                            class: "d0-btn",
                            "data-a11y-id": "hero-take-tour",
                            role: AccessRole::Button.aria(),
                            "aria-label": dat0_i18n::t("hero.take_tour"),
                            tabindex: "0",
                            onclick: move |_| on_take_tour.call(()),
                            {dat0_i18n::t("hero.take_tour")}
                        }
                    }
                    div { class: "d0-hero-demo",
                        span { class: "d0-head-title", {dat0_i18n::t("hero.demo.heading")} }
                        button {
                            // The app's single amber-filled control. The ink on
                            // it comes from `--d0-ink-on-amber`, which is the
                            // same value in every theme.
                            class: "d0-cta",
                            "data-a11y-id": "hero-open-demo",
                            role: AccessRole::Button.aria(),
                            "aria-label": dat0_i18n::t("hero.demo.cta"),
                            tabindex: "0",
                            onclick: move |_| on_open_demo.call(()),
                            "▶ "
                            {dat0_i18n::t("hero.demo.cta")}
                        }
                    }
                    span { class: "d0-label", {dat0_i18n::t("hero.samples.heading")} }
                }
            } else {
                {headline()}
            }

            div { class: "d0-hero-cols",
                {drop_zone(props.booting)}
                div { class: "d0-hero-right",
                    if props.recents.is_empty() {
                        {sample_column(props.on_open_sample, props.on_open_file)}
                    } else {
                        RecentsColumn {
                            recents: props.recents.clone(),
                            on_open_recent: props.on_open_recent,
                            on_open_file: props.on_open_file,
                        }
                    }
                }
            }
        }
    }
}

/// The product statement. Rendered in both modes — this is what dat0 *is*, not
/// a first-run greeting.
///
/// Only the title carries an `aria-label`: one labelled node keeps `has_label`
/// lookups on the hero unambiguous.
fn headline() -> Element {
    rsx! {
        div { class: "d0-hero-headline",
            h1 {
                class: "d0-h1",
                "data-a11y-id": "hero-title",
                role: AccessRole::Label.aria(),
                "aria-label": dat0_i18n::t("hero.title"),
                {dat0_i18n::t("hero.title")}
            }
            p { class: "d0-h2 is-muted", {dat0_i18n::t("hero.subtitle")} }
            p { class: "d0-lead is-muted", {dat0_i18n::t("hero.lead")} }
        }
    }
}

/// The drop affordance plus the privacy line.
///
/// `hero.privacy` is green — the same green the status bar's `egress 0 B` uses,
/// because it is the same claim. While booting the drop copy is replaced by a
/// placeholder, but the privacy line stays: it is true in every state, and it is
/// the claim a user watching a spinner most wants held.
fn drop_zone(booting: bool) -> Element {
    rsx! {
        div { class: "d0-hero-drop", "data-a11y-id": "hero-drop",
            if booting {
                span {
                    class: "d0-mono is-muted",
                    "data-a11y-id": "hero-booting",
                    role: AccessRole::Label.aria(),
                    "aria-label": dat0_i18n::t("session.booting"),
                    {dat0_i18n::t("session.booting")}
                }
                span { class: "d0-skel", style: "width: 320px;" }
                span { class: "d0-skel is-secondary", style: "width: 220px;" }
            } else {
                p { class: "d0-body", {dat0_i18n::t("hero.drop")} }
            }
            span { class: "d0-mono is-ok", {dat0_i18n::t("hero.privacy")} }
        }
    }
}

/// Right column when there are no recents: the three sample datasets, then
/// "Open file…".
fn sample_column(
    on_open_sample: EventHandler<SampleKind>,
    on_open_file: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "d0-hero-list", "data-a11y-id": "hero-samples",
            span { class: "d0-label", {dat0_i18n::t("hero.samples_label")} }
            for entry in entries() {
                {
                    let id = sample_static_id(&entry.kind);
                    let kind = entry.kind.clone();
                    rsx! {
                        button {
                            key: "{id}",
                            class: "d0-hero-card",
                            "data-a11y-id": "{id}",
                            role: AccessRole::Button.aria(),
                            // The title is the card's accessible name; no extra
                            // labelled node, or a role-agnostic lookup for the
                            // title matches twice.
                            "aria-label": "{entry.title}",
                            tabindex: "0",
                            onclick: move |_| on_open_sample.call(kind.clone()),
                            span { class: "d0-head-title", "{entry.title}" }
                            span { class: "d0-mono is-muted", "{entry.subtitle}" }
                        }
                    }
                }
            }
            {open_file_button("hero-open-file-samples", on_open_file)}
        }
    }
}

/// Right column when recents exist: the list, then "Open file…".
///
/// The list is **one** tab stop and the arrows move within it. Six recents as
/// six tab stops is how a keyboard user ends up pressing Tab a dozen times to
/// reach the button after them.
#[component]
fn RecentsColumn(
    recents: Vec<RecentEntry>,
    on_open_recent: EventHandler<RecentEntry>,
    on_open_file: EventHandler<()>,
) -> Element {
    let len = recents.len();
    let mut active = use_signal(|| 0usize);
    // A recent may have been removed since the last arrow press, so the ring
    // never points off the end.
    let selected = active().min(len.saturating_sub(1));
    let for_enter = recents.clone();

    rsx! {
        div { class: "d0-hero-list", "data-a11y-id": "hero-recents",
            div {
                class: "d0-hero-recents",
                "data-a11y-id": "recents-list",
                role: AccessRole::Button.aria(),
                "aria-label": dat0_i18n::t("hero.recent_label"),
                tabindex: "0",
                onkeydown: move |e| {
                    match e.key() {
                        Key::ArrowDown => {
                            active.set((selected + 1).min(len.saturating_sub(1)));
                        }
                        Key::ArrowUp => active.set(selected.saturating_sub(1)),
                        // Mouse and keyboard call the SAME open path through the
                        // SAME clamped index, so they cannot drift.
                        k if activates(&k) => {
                            if let Some(entry) = active_recent(&for_enter, selected) {
                                on_open_recent.call(entry);
                            }
                        }
                        _ => return,
                    }
                    e.prevent_default();
                },
                span { class: "d0-label", {dat0_i18n::t("hero.recent_label")} }
                for (i, entry) in recents.iter().enumerate() {
                    {
                        let e = entry.clone();
                        let path = entry.path().display().to_string();
                        rsx! {
                            button {
                                key: "{i}",
                                class: if i == selected { "d0-hero-recent is-active" } else { "d0-hero-recent" },
                                "data-a11y-id": "hero-recent-{i}",
                                role: AccessRole::Button.aria(),
                                "aria-label": "{path}",
                                tabindex: "-1",
                                onclick: move |_| on_open_recent.call(e.clone()),
                                "{path}"
                            }
                        }
                    }
                }
            }
            {open_file_button("hero-open-file-recents", on_open_file)}
        }
    }
}

/// Enter and Space activate; every other character key is typing.
fn activates(key: &Key) -> bool {
    matches!(key, Key::Enter) || matches!(key, Key::Character(c) if c.as_str() == " ")
}

/// "Open file…" — its own tab stop, after whichever column precedes it.
///
/// Two ids rather than one because the samples and the recents column are never
/// both mounted, and a shared id would make a test that asserts "the recents
/// column's open button" pass against the samples one.
fn open_file_button(id: &'static str, on_open_file: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: "d0-btn",
            "data-a11y-id": "{id}",
            role: AccessRole::Button.aria(),
            "aria-label": dat0_i18n::t("hero.open_file"),
            tabindex: "0",
            onclick: move |_| on_open_file.call(()),
            {dat0_i18n::t("hero.open_file")}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enriched_only_before_first_run_done() {
        assert_eq!(hero_mode(false), HeroMode::Enriched);
        assert_eq!(hero_mode(true), HeroMode::Plain);
        assert!(band_visible(HeroMode::Enriched));
        assert!(!band_visible(HeroMode::Plain));
    }

    #[test]
    fn auto_tour_only_before_first_run_done() {
        assert!(should_auto_tour(false));
        assert!(!should_auto_tour(true));
    }

    /// A hero string that renders as its own key is an unresolved i18n lookup on
    /// the first screen a new user ever sees.
    #[test]
    fn every_hero_copy_key_resolves() {
        for key in HERO_COPY_KEYS {
            let text = dat0_i18n::t(key);
            assert_ne!(&text, key, "{key} does not resolve in en.json");
            assert!(!text.is_empty(), "{key} resolves to an empty string");
        }
    }

    /// The marketing page's honesty labels describe an UNSHIPPED product. Inside
    /// the shipped app they are false.
    #[test]
    fn hero_copy_never_claims_the_product_is_unshipped() {
        for key in HERO_COPY_KEYS {
            let text = dat0_i18n::t(key).to_lowercase();
            for banned in [
                "in development",
                "concept",
                "not shipped",
                "designed, not",
                "coming soon",
            ] {
                assert!(
                    !text.contains(banned),
                    "{key} = {text:?} carries {banned:?}"
                );
            }
        }
    }

    #[test]
    fn active_recent_selects_in_range_and_none_otherwise() {
        let list = vec![
            RecentEntry::Package {
                path: std::path::PathBuf::from("/a"),
            },
            RecentEntry::Workspace {
                path: std::path::PathBuf::from("/b"),
            },
        ];
        assert_eq!(active_recent(&list, 0), Some(list[0].clone()));
        assert_eq!(active_recent(&list, 1), Some(list[1].clone()));
        assert_eq!(active_recent(&list, 2), None);
        assert_eq!(active_recent(&[], 0), None);
    }

    /// The three cards dispatch three different samples. A duplicate id would
    /// mean two cards open the same dataset and one is unreachable.
    #[test]
    fn sample_ids_are_unique_per_entry() {
        let all = entries();
        assert_eq!(all.len(), 3, "the hero renders exactly three sample cards");
        let mut ids: Vec<&str> = all.iter().map(|e| sample_static_id(&e.kind)).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 3, "each sample card needs its own id");
    }
}
