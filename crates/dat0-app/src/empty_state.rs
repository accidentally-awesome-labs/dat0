//! Empty-state hero (P3b T7).
//!
//! Rendered by [`crate::window::WorkspaceShell::render`] when the session
//! has no open tabs AND the user has no recents yet — the "first launch /
//! cleared workspace" hero with two columns:
//!
//! - **Left (`drop_zone`, flex-grow):** "Drop a file to start" affordance.
//! - **Right (`recents_column`, fixed 280 px):** sample-data picker when
//!   recents are empty, or the recents list itself once the user has
//!   opened a file before.
//!
//! T7 ships the skeleton only. Sample-button click handlers (extract
//! bundled bytes via [`crate::sample_data::ensure_bundled_extracted`],
//! then mount a [`crate::grid::GridDataSource`]) are deliberately
//! deferred to a T7 follow-up so the render branch can land alongside
//! the bundled assets without dragging in the data-source plumbing.

use gpui::{IntoElement, ParentElement, Styled, div, px};

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

/// View model for the empty-state hero. `recents_empty=true` shows the
/// sample-data picker; `false` shows the recents list (still T7
/// follow-up — for the skeleton both branches render placeholder copy).
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

    /// Build the two-column hero. Returns an `AnyElement` because the
    /// caller (`WorkspaceShell::render`) folds this branch alongside the
    /// `Table` branch via `into_any_element()` (single-return-type rule
    /// for `impl IntoElement`).
    ///
    /// `_cx` is reserved for later use (sample-button click handlers
    /// need `&mut App` to schedule the bundled-extract task); kept in
    /// the signature so the T7 follow-up patch can attach handlers
    /// without churning the call site in `WorkspaceShell::render`.
    pub fn render(&self, _cx: &mut gpui::App) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .child(self.drop_zone())
            .child(self.recents_column())
    }

    fn drop_zone(&self) -> impl IntoElement {
        div()
            .flex_grow()
            .flex()
            .items_center()
            .justify_center()
            .child("Drop a file to start")
    }

    fn recents_column(&self) -> impl IntoElement {
        let body = if self.recents_empty {
            // Sample-data picker — labels only at T7. The click handler
            // (extract → mount GridDataSource) lands in a T7 follow-up.
            div().flex().flex_col().child("Samples").children(
                crate::sample_data::entries()
                    .into_iter()
                    .map(|e| div().child(e.title)),
            )
        } else {
            // Recents list — wired in a T7 follow-up once the recents
            // surface is plumbed into the view.
            div().flex().flex_col().child("Recents…")
        };

        div().w(px(280.)).child(body)
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
}
