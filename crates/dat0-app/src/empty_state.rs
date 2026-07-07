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

// UAT Gap 2: `.a11y(id, role, label)` on the hero's clickable elements. Under
// the `a11y-capture` feature it emits an AccessKit node + chains
// `debug_selector`; in release it is an identity no-op (`AccessRole` resolves to
// the feature-off stub enum, so this import compiles in both states).
use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};

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

/// Stable `&'static str` id for a sample card (UAT Gap 2). `debug_bounds`
/// (and `.a11y`'s click-id side-map) require `&'static str`, but the sample
/// catalog is built at call time via [`crate::sample_data::entries`], so the
/// card loop cannot just intern the runtime index. Instead each card's
/// `SampleKind` discriminant maps to a fixed id: the exhaustive match is safe
/// because the catalog currently has exactly one entry per variant (Iris is
/// the only `BundledCsv`, Chinook the only `BundledSqlite`, NYC taxi the only
/// `Remote`). If a future catalog adds a second entry of the same variant,
/// this match must grow a finer discriminant (e.g. on `dest_filename`).
pub(crate) fn sample_static_id(kind: &crate::sample_data::SampleKind) -> &'static str {
    use crate::sample_data::SampleKind;
    match kind {
        SampleKind::BundledCsv { .. } => "hero-sample-iris",
        SampleKind::BundledSqlite { .. } => "hero-sample-chinook",
        SampleKind::Remote { .. } => "hero-sample-nyc-taxi",
    }
}

/// Stable hero-button focus handles, passed down from the persistent
/// `WorkspaceShell` (the `EmptyState` is transient — it must not mint handles).
/// Slice 6: keyboard-nav / focus reachability.
pub struct HeroHandles {
    pub map: std::collections::HashMap<&'static str, gpui::FocusHandle>,
}
impl HeroHandles {
    pub fn get(&self, id: &'static str) -> &gpui::FocusHandle {
        self.map
            .get(id)
            .expect("hero handle pre-registered in WorkspaceShell::render")
    }
}

/// View model for the empty-state hero. `recents_empty=true` shows the
/// sample-data picker; `false` shows the recents list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyState {
    pub recents_empty: bool,
    pub first_run_done: bool,
    /// Active-row index for the recents list (from `WorkspaceShell.recents_active`).
    /// Drives the active-row ring; the arrow handler mutates the shell field.
    pub recents_active: usize,
}

impl EmptyState {
    pub fn new(recents_empty: bool, first_run_done: bool, recents_active: usize) -> Self {
        Self {
            recents_empty,
            first_run_done,
            recents_active,
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
        hero: &HeroHandles,
        cx: &mut gpui::Context<crate::window::WorkspaceShell>,
    ) -> gpui::AnyElement {
        let mode = hero_mode(self.first_run_done);

        if band_visible(mode) {
            let right_col: gpui::AnyElement = if self.recents_empty {
                self.sample_column(hero, cx)
            } else {
                self.recents_column(hero, cx)
            };
            // `open_deferred` (not `open`): this click handler fires inside a
            // `window.update` of the active window, where a synchronous
            // `onboarding::open` re-enters the taken window and silently
            // no-ops. The deferred dispatcher hop runs it from a plain App
            // context after the frame (the auto-show mechanism).
            // Single source of truth for "activate the tour" — both the mouse
            // `on_click` and the keyboard `on_key_down` twin (Slice 6) call the
            // same `crate::onboarding::open_deferred`, so they cannot drift.
            let take_tour_handler = cx.listener(|_this, _ev, _window, cx| {
                crate::onboarding::open_deferred(cx);
            });
            let take_tour_key = cx.listener(|_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                crate::onboarding::open_deferred(cx);
            });
            // Slice 6: `hero-open-demo`'s keyboard twin. Unlike take-tour this
            // does NOT need the dispatcher hop — `open_demo_workspace` opens a
            // brand-new window (not a dialog on the ALREADY-taken active
            // window), so the re-entrancy hazard that forces `open_deferred`
            // does not apply here. Both handlers call the SAME production fn
            // so mouse and keyboard cannot drift.
            let open_demo_handler = cx.listener(|_this, _ev, _window, cx| {
                crate::window::open_demo_workspace(cx);
            });
            let open_demo_key = cx.listener(|_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                crate::window::open_demo_workspace(cx);
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
                        .child(
                            // Content-only locator (release no-op): `.a11y_label`
                            // emits a `Role::Label` AccessKit node under the
                            // `a11y-capture` feature so the headless UAT can find
                            // the tagline BY ITS TEXT (UAT Gap 2). It is NOT
                            // clickable (no id / no debug_selector) — content
                            // assertion only. Feature OFF → identity no-op.
                            div()
                                .flex_grow()
                                .a11y_label(AccessRole::Label, dat0_i18n::t("hero.tagline"))
                                .child(dat0_i18n::t("hero.tagline")),
                        )
                        .child(
                            div()
                                .id("hero-take-tour")
                                // Slice 6 (keyboard-nav): make this hero button a
                                // real keyboard control — a Tab stop (via `Root`'s
                                // focus_next) that takes focus, activates on
                                // Enter/Space, and paints a focus ring. Ships in
                                // release (genuine a11y fix). The stable focus
                                // handle lives on the persistent `WorkspaceShell`
                                // (passed in `hero`), NOT this transient view.
                                .focus_stop(
                                    "hero-take-tour",
                                    hero.get("hero-take-tour"),
                                    0,
                                    take_tour_key,
                                )
                                // Test-only locator (release no-op): `.a11y` both
                                // chains `debug_selector` (so the headless UAT can
                                // find this button's painted bounds via
                                // `VisualTestContext::debug_bounds` instead of a
                                // fragile hard-coded pixel) AND, under the
                                // `a11y-capture` feature, emits an AccessKit
                                // Button node so kittest can locate it by label
                                // (UAT Gap 2). Feature OFF → identity no-op.
                                .a11y(
                                    "hero-take-tour",
                                    AccessRole::Button,
                                    dat0_i18n::t("hero.take_tour"),
                                )
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
                                // Slice 6: real Tab stop + Enter/Space activation,
                                // same pattern as `hero-take-tour` above.
                                .focus_stop(
                                    "hero-open-demo",
                                    hero.get("hero-open-demo"),
                                    0,
                                    open_demo_key,
                                )
                                .a11y(
                                    "hero-open-demo",
                                    AccessRole::Button,
                                    dat0_i18n::t("hero.demo.cta"),
                                )
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
                self.sample_column(hero, cx)
            } else {
                self.recents_column(hero, cx)
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
            .child(dat0_i18n::t("hero.drop_zone"))
    }

    /// Right column when there are no recents: 3 wired sample-data buttons
    /// (Iris CSV, Chinook SQLite, NYC taxi remote) plus "Open file…".
    ///
    /// Each clickable div is given a stable string ID — required by GPUI's
    /// `StatefulInteractiveElement` (which provides `on_click`) before an
    /// element participates in the hitbox/event system.
    ///
    /// UAT Gap 2: each card's clickable container carries a stable
    /// `.a11y(static_id, AccessRole::Button, title)` (the dynamic
    /// `hero-sample-{i}` id above is fine for `on_click`'s hitbox but
    /// `debug_bounds`/`debug_selector` need a `&'static str` — see
    /// [`sample_static_id`]) so a headless test can locate (and assert) a
    /// specific card by role+title without a hard-coded pixel offset. The
    /// title's text is already the Button node's label, so no separate
    /// content-only Label node is emitted (that would duplicate the text and
    /// make a role-agnostic `has_label(title)` panic on two matches).
    /// Feature OFF (release) → identity no-op.
    fn sample_column(
        &self,
        hero: &HeroHandles,
        cx: &mut gpui::Context<crate::window::WorkspaceShell>,
    ) -> gpui::AnyElement {
        let mut col = div()
            .flex()
            .flex_col()
            .child(div().child(dat0_i18n::t("hero.samples_label")));

        for (i, entry) in crate::sample_data::entries().into_iter().enumerate() {
            let kind = entry.kind.clone();
            let title = entry.title;
            let subtitle = entry.subtitle;
            let id = gpui::SharedString::from(format!("hero-sample-{i}"));
            let static_id = sample_static_id(&kind);
            // Slice 6: a second clone feeds the keyboard twin below — `kind`
            // itself is moved into `handler`'s closure exactly as before.
            let kind_for_key = kind.clone();
            let handler = cx.listener(move |this, _ev, _window, cx| {
                this.open_sample_kind(kind.clone(), cx);
            });
            let key_handler = cx.listener(move |this, _ev: &gpui::KeyDownEvent, _window, cx| {
                this.open_sample_kind(kind_for_key.clone(), cx);
            });
            col = col.child(
                div()
                    .id(id)
                    .flex()
                    .flex_col()
                    // Slice 6: real Tab stop + Enter/Space activation, same
                    // static id the `.a11y` node below (and the oracle) uses.
                    .focus_stop(static_id, hero.get(static_id), 0, key_handler)
                    .a11y(static_id, AccessRole::Button, title)
                    .child(div().child(title))
                    .child(div().child(subtitle))
                    .on_click(handler),
            );
        }

        let open_handler = cx.listener(|this, _ev, _window, cx| {
            this.open_file_picker(cx);
        });
        let open_key_handler = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            this.open_file_picker(cx);
        });
        col.child(
            div()
                .id("hero-open-file-samples")
                // Slice 6: real Tab stop + Enter/Space activation, same
                // pattern as the sample cards above.
                .focus_stop(
                    "hero-open-file-samples",
                    hero.get("hero-open-file-samples"),
                    0,
                    open_key_handler,
                )
                .a11y(
                    "hero-open-file-samples",
                    AccessRole::Button,
                    dat0_i18n::t("hero.open_file"),
                )
                .child(dat0_i18n::t("hero.open_file"))
                .on_click(open_handler),
        )
        .into_any_element()
    }

    /// Right column when recents exist: a clickable list of recent paths,
    /// then "Open file…".
    ///
    /// Slice 6 Task 1b: the fixed-id "Open file…" button (`hero-open-file-recents`)
    /// is wired to a real Tab stop, the same pattern `sample_column` uses for
    /// `hero-open-file-samples`. The dynamic `hero-recent-{i}` list rows are
    /// intentionally left un-wired (out of scope for this task).
    fn recents_column(
        &self,
        hero: &HeroHandles,
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
        let len = recent_entries.len();
        // Clamp the persisted index to the current list (a recent may have been
        // removed since the last nav) so the active-row ring never points off
        // the end. `recents_column` only renders when the list is non-empty
        // (`recents_empty == false`), so `len >= 1` here; the guard is defensive.
        let active = self.recents_active.min(len.saturating_sub(1));

        // Enter/Space activate: open whichever row the active-index selects, via
        // the SAME `open_recent_entry` path a row's `on_click` uses (mouse and
        // keyboard cannot drift — Slice-6 rule). `focus_stop` wires this to
        // Enter/Space internally.
        let entries_for_enter = recent_entries.clone();
        let activate = cx.listener(move |this, _ev: &gpui::KeyDownEvent, _window, cx| {
            if let Some(entry) = active_recent(&entries_for_enter, this.recents_active) {
                this.open_recent_entry(entry, cx);
            }
        });
        // ↑/↓ move the active-index. This is a SECOND `on_key_down` chained after
        // `focus_stop` (gpui pushes key-down listeners, so both fire); `len` is
        // captured for the down-clamp.
        let arrows = cx.listener(move |this, ev: &gpui::KeyDownEvent, _window, cx| {
            match ev.keystroke.key.as_str() {
                "down" => {
                    this.recents_active = (this.recents_active + 1).min(len.saturating_sub(1))
                }
                "up" => this.recents_active = this.recents_active.saturating_sub(1),
                _ => return,
            }
            cx.notify();
        });

        // The list is ONE tab stop (`focus_stop` on this container); arrows move
        // within it. The `.a11y` twin carries the SAME "recents-list" id so the
        // focus oracle can name the focused list by its label text.
        let mut list = div()
            .flex()
            .flex_col()
            .focus_stop("recents-list", hero.get("recents-list"), 0, activate)
            .on_key_down(arrows)
            .a11y(
                "recents-list",
                AccessRole::Button,
                dat0_i18n::t("hero.recent_label"),
            )
            .child(div().child(dat0_i18n::t("hero.recent_label")));

        for (i, entry) in recent_entries.into_iter().enumerate() {
            let label = entry.path().display().to_string();
            let id = gpui::SharedString::from(format!("hero-recent-{i}"));
            let handler = cx.listener(move |this, _ev, _window, cx| {
                this.open_recent_entry(entry.clone(), cx);
            });
            let mut row = div().id(id).child(label).on_click(handler);
            if i == active {
                row = row
                    .border_2()
                    .border_color(gpui::rgb(crate::a11y::FOCUS_RING));
            }
            list = list.child(row);
        }

        // The "Open file…" button remains a SEPARATE tab stop after the list
        // (unchanged from Slice 6, moved below the list container).
        let open_handler = cx.listener(|this, _ev, _window, cx| {
            this.open_file_picker(cx);
        });
        let open_key_handler = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            this.open_file_picker(cx);
        });
        let open_button = div()
            .id("hero-open-file-recents")
            .focus_stop(
                "hero-open-file-recents",
                hero.get("hero-open-file-recents"),
                0,
                open_key_handler,
            )
            .a11y(
                "hero-open-file-recents",
                AccessRole::Button,
                dat0_i18n::t("hero.open_file"),
            )
            .child(dat0_i18n::t("hero.open_file"))
            .on_click(open_handler);

        div()
            .flex()
            .flex_col()
            .child(list)
            .child(open_button)
            .into_any_element()
    }
}

/// The recent entry the active-index currently selects, or `None` if the list
/// is empty or the index is out of range. Pure — the unit-testable core of the
/// recents-list keyboard activation (mirrors Slice-4's `resolve_relaunch_action`
/// pure seam). The heavy file-open (`WorkspaceShell::open_recent_entry`) is NOT
/// exercised here.
fn active_recent(
    entries: &[crate::recents::RecentEntry],
    active: usize,
) -> Option<crate::recents::RecentEntry> {
    entries.get(active).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_can_be_constructed() {
        let e = EmptyState::new(true, false, 0);
        assert!(e.recents_empty);
        let e2 = EmptyState::new(false, true, 0);
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
