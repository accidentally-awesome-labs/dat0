//! Empty-state hero (P3b T7, wired P11a T4, enriched band P11a T5).
//!
//! Rendered by [`crate::window::WorkspaceShell::render`] when the session
//! has no open tabs — the "first launch / cleared workspace" hero.
//!
//! ## Plain mode (returning users)
//!
//! Two columns:
//! - **Left (`drop_zone`, flex-grow):** "Drop a file to start" affordance.
//! - **Right (fixed 280 px):** sample-data picker when recents are empty
//!   (3 wired buttons + "Open file…"), or the recents list once the user has
//!   opened a file before (each entry wired + "Open file…").
//!
//! ## Enriched mode (first run only — `HeroMode::Enriched`)
//!
//! An additional value-prop band is rendered ABOVE the two-column hero:
//! - Tagline + "[Take a tour ›]" button (top-right) — wired in T7.
//! - Featured "Try the demo workspace" card with "[▶ Open demo.dat0]" — wired
//!   in T9.
//! - "Or start from a sample:" heading demoting the existing 3 sample entries.
//!
//! P11a T4 wires all previously-inert buttons to the T3 shell helpers
//! (`open_sample_kind`, `open_recent_entry`, `open_file_picker`), closing
//! the P3b dead-skeleton debt.  Render is UAT-verified, not unit-tested —
//! see the in-file smoke test for the structural guard.
//!
//! GPUI note: `on_click` lives on `StatefulInteractiveElement` (not
//! `InteractiveElement`), so every clickable element must have an `.id(…)`
//! assigned first to become `Stateful<Div>`.

use gpui::{IntoElement, ParentElement, Styled, div, prelude::*, px};

/// Which hero variant to render. `Enriched` (first run only) adds the
/// value-prop band + featured demo CTA above the base hero; `Plain` is the
/// wired P3 hero with no band and no auto-popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroMode {
    Enriched,
    Plain,
}

/// First-run only ⇒ enriched. (`onboarding-v1.md` §3 state machine.)
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

/// Returns `true` iff the first-run value-prop band should be rendered
/// above the base two-column hero.  Only `HeroMode::Enriched` shows the
/// band; `HeroMode::Plain` renders the base hero with no band.
pub fn band_visible(mode: HeroMode) -> bool {
    matches!(mode, HeroMode::Enriched)
}

/// View model for the empty-state hero. `recents_empty=true` shows the
/// sample-data picker; `false` shows the recents list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyState {
    pub recents_empty: bool,
    pub first_run_done: bool,
}

impl EmptyState {
    pub fn new(recents_empty: bool, first_run_done: bool) -> Self {
        Self {
            recents_empty,
            first_run_done,
        }
    }

    /// Build the hero element.
    ///
    /// In **Plain** mode this is the unchanged two-column base hero.
    /// In **Enriched** mode (first run) an additional value-prop band is
    /// prepended above the two-column hero.  The band contains:
    ///
    /// - A tagline + "[Take a tour]" button (no-op until T7).
    /// - A featured "Try the demo workspace" card with "[▶ Open demo.dat0]"
    ///   (no-op until T9).
    /// - An "Or start from a sample:" heading above the existing entries.
    ///
    /// Returns `AnyElement` because the two column branches (samples vs.
    /// recents) produce different concrete element types that must be widened
    /// to a common type, and callers fold this branch alongside the `Table`
    /// branch via `AnyElement`.
    ///
    /// `cx` is `&mut Context<WorkspaceShell>` so button click handlers can
    /// use `cx.listener(...)` to reach the shell's action helpers directly.
    pub fn render(
        &self,
        cx: &mut gpui::Context<crate::window::WorkspaceShell>,
    ) -> gpui::AnyElement {
        let mode = hero_mode(self.first_run_done);

        if band_visible(mode) {
            let right_col: gpui::AnyElement = if self.recents_empty {
                self.sample_column(cx)
            } else {
                self.recents_column(cx)
            };
            let take_tour_handler = cx.listener(|_this, _ev, _window, _cx| {
                // TODO(T7): dispatch TakeTour
            });
            let open_demo_handler = cx.listener(|_this, _ev, _window, _cx| {
                // TODO(T9): dispatch OpenDemoWorkspace
            });
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    // Tagline row: tagline text (left) + tour button (right).
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(div().flex_grow().child(dat0_i18n::t("hero.tagline")))
                        .child(
                            div()
                                .id("hero-take-tour")
                                .child(dat0_i18n::t("hero.take_tour"))
                                .on_click(take_tour_handler),
                        ),
                )
                .child(
                    // Featured demo card.
                    div()
                        .flex()
                        .flex_col()
                        .child(div().child(dat0_i18n::t("hero.demo.heading")))
                        .child(
                            div()
                                .id("hero-open-demo")
                                .child(dat0_i18n::t("hero.demo.cta"))
                                .on_click(open_demo_handler),
                        ),
                )
                .child(
                    // "Or start from a sample:" sub-heading above sample list.
                    div().child(dat0_i18n::t("hero.samples.heading")),
                )
                .child(
                    // Base two-column hero below.
                    div()
                        .flex_grow()
                        .flex()
                        .flex_row()
                        .child(self.drop_zone())
                        .child(div().w(px(280.)).child(right_col)),
                )
                .into_any_element()
        } else {
            let right_col: gpui::AnyElement = if self.recents_empty {
                self.sample_column(cx)
            } else {
                self.recents_column(cx)
            };
            div()
                .size_full()
                .flex()
                .flex_row()
                .child(self.drop_zone())
                .child(div().w(px(280.)).child(right_col))
                .into_any_element()
        }
    }

    fn drop_zone(&self) -> impl IntoElement {
        div()
            .flex_grow()
            .flex()
            .items_center()
            .justify_center()
            .child("Drop a file to start")
    }

    /// Right column when there are no recents: 3 wired sample-data buttons
    /// (Iris CSV, Chinook SQLite, NYC taxi remote) plus "Open file…".
    ///
    /// Each clickable div is given a stable string ID — required by GPUI's
    /// `StatefulInteractiveElement` (which provides `on_click`) before an
    /// element participates in the hitbox/event system.
    fn sample_column(
        &self,
        cx: &mut gpui::Context<crate::window::WorkspaceShell>,
    ) -> gpui::AnyElement {
        let mut col = div().flex().flex_col().child(div().child("Samples"));

        for (i, entry) in crate::sample_data::entries().into_iter().enumerate() {
            let kind = entry.kind.clone();
            let title = entry.title;
            let subtitle = entry.subtitle;
            let id = gpui::SharedString::from(format!("hero-sample-{i}"));
            let handler = cx.listener(move |this, _ev, _window, cx| {
                this.open_sample_kind(kind.clone(), cx);
            });
            col = col.child(
                div()
                    .id(id)
                    .flex()
                    .flex_col()
                    .child(div().child(title))
                    .child(div().child(subtitle))
                    .on_click(handler),
            );
        }

        let open_handler = cx.listener(|this, _ev, _window, cx| {
            this.open_file_picker(cx);
        });
        col.child(
            div()
                .id("hero-open-file-samples")
                .child("Open file…")
                .on_click(open_handler),
        )
        .into_any_element()
    }

    /// Right column when recents exist: a clickable list of recent paths,
    /// then "Open file…".
    fn recents_column(
        &self,
        cx: &mut gpui::Context<crate::window::WorkspaceShell>,
    ) -> gpui::AnyElement {
        let recent_entries: Vec<crate::recents::RecentEntry> =
            if let Ok(cfg) = crate::platform::config_dir() {
                crate::recents::Recents::with_path(cfg.join("recents.json"))
                    .list()
                    .to_vec()
            } else {
                vec![]
            };

        let mut col = div().flex().flex_col().child(div().child("Recent"));

        for (i, entry) in recent_entries.into_iter().enumerate() {
            let label = entry.path().display().to_string();
            let id = gpui::SharedString::from(format!("hero-recent-{i}"));
            let handler = cx.listener(move |this, _ev, _window, cx| {
                this.open_recent_entry(entry.clone(), cx);
            });
            col = col.child(div().id(id).child(label).on_click(handler));
        }

        let open_handler = cx.listener(|this, _ev, _window, cx| {
            this.open_file_picker(cx);
        });
        col.child(
            div()
                .id("hero-open-file-recents")
                .child("Open file…")
                .on_click(open_handler),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_can_be_constructed() {
        let e = EmptyState::new(true, false);
        assert!(e.recents_empty);
        let e2 = EmptyState::new(false, true);
        assert!(!e2.recents_empty);
    }

    #[test]
    fn enriched_only_before_first_run_done() {
        assert_eq!(hero_mode(false), HeroMode::Enriched);
        assert_eq!(hero_mode(true), HeroMode::Plain);
    }

    #[test]
    fn auto_tour_only_before_first_run_done() {
        assert!(should_auto_tour(false));
        assert!(!should_auto_tour(true));
    }

    #[test]
    fn band_visible_enriched_only() {
        assert!(
            band_visible(HeroMode::Enriched),
            "band must be visible in Enriched mode"
        );
        assert!(
            !band_visible(HeroMode::Plain),
            "band must NOT be visible in Plain mode"
        );
    }

    /// Structural guard: the hero wires exactly 3 sample buttons.  If
    /// `sample_data::entries()` changes count or kind-order, the render
    /// code in `sample_column` will dispatch wrong actions — catch it here.
    ///
    /// Note: no `#[gpui::test]` render-smoke test exists in this crate
    /// (no precedent at the pinned GPUI 0.2.2 rev), so the actual render
    /// is verified via manual UAT rather than an automated harness.
    #[test]
    fn sample_buttons_cover_all_entries() {
        let entries = crate::sample_data::entries();
        assert_eq!(entries.len(), 3, "hero expects exactly 3 sample entries");
        assert!(
            matches!(
                entries[0].kind,
                crate::sample_data::SampleKind::BundledCsv { .. }
            ),
            "first sample must be BundledCsv (Iris)"
        );
        assert!(
            matches!(
                entries[1].kind,
                crate::sample_data::SampleKind::BundledSqlite { .. }
            ),
            "second sample must be BundledSqlite (Chinook)"
        );
        assert!(
            matches!(
                entries[2].kind,
                crate::sample_data::SampleKind::Remote { .. }
            ),
            "third sample must be Remote (NYC taxi)"
        );
    }
}
