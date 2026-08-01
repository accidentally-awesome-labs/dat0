//! Cmd-Shift-P command palette — the MODEL half (UI redesign B4).
//!
//! This module holds everything the palette knows that does not need a
//! `Window`: which registry descriptors are fit to show, how they are ranked
//! against a query, the key bindings, and the `&mut App` entry point that asks
//! the focused workspace to mount the view. The view itself — the `InputState`,
//! the results listbox, the render — is [`crate::view::command_palette`].
//!
//! The split is what keeps ranking unit-testable with no `Window` at all, the
//! same reason B1 extracted `overlay::next_index`.
//!
//! # Filter shape
//!
//! [`filter`] is a fuzzy subsequence match, case-insensitive, against
//! [`ActionDescriptor::title`], preserving registry-iteration order. Its
//! signature is pinned by `tests/command_palette.rs` and is deliberately left
//! alone; ordering and visibility are layered on top by [`visible_items`],
//! because `ActionRegistry::iter` snapshots a `HashMap` and would otherwise
//! reshuffle the list between frames.

use crate::actions::registry::{ActionDescriptor, ActionRegistry};

/// Key context carried by the palette's root element. Every stop inside the
/// modal sits below it, which is what lets [`PaletteUp`]/[`PaletteDown`] match
/// from the query field AND from the results list.
pub const PALETTE_CONTEXT: &str = "CommandPalette";

gpui::actions!(dat0_palette, [PaletteUp, PaletteDown]);

/// Registered but never shown in the palette. Each entry is dead for a reason a
/// no-arg invocation cannot fix:
///
/// - `file.open`, `theme.toggle`, `recents.show`, `sample_data.retry_taxi` —
///   the dispatch body is a `tracing` breadcrumb; there is nothing to run.
/// - `view.set_value` needs a `Scalar` and `view.delete_column` needs a
///   `col_ix`; the context menu passes them through a direct closure
///   (`edit_actions.rs`). A fuzzy search box has neither.
///
/// Showing these would repeat the greyed-out-menu-item defect PRs #59/#60 fixed.
pub const HIDDEN: &[&str; 6] = &[
    "file.open",
    "theme.toggle",
    "recents.show",
    "sample_data.retry_taxi",
    "view.set_value",
    "view.delete_column",
];

/// Shown, but the registry closure is a breadcrumb: these need a `&mut Window`
/// that `DispatchFn`'s `Fn(&mut App)` cannot supply.
/// `WorkspaceShell::run_palette_action` runs them instead — the palette is a
/// modal inside the window, so it HAS the `Window` the boot-time closure lacks.
pub const WINDOW_ROUTED: &[&str; 7] = &[
    "console.toggle",
    "sql.new_tab",
    "sql.save_query",
    "sql.load_query",
    "sql.history",
    "sql.save_as_table",
    "view.save_as_table",
];

/// Bind ⌘⇧P / ⌃⇧P and the palette-scoped arrows.
///
/// MUST be called by production (`run_app`) **and** by every test binary's
/// `init_components` — the harness calls only `gpui_component::init`, so a
/// prod-only binding is invisible to tests and a green suite can hide a dead
/// production key path (the carve-out #7 lesson, and the same rule
/// `overlay::register_modal_keys` carries). The B4 T0 gate confirmed the chord
/// is genuinely unbound in a bare test app.
///
/// The arrows are dat0 actions under [`PALETTE_CONTEXT`] rather than an
/// interception of upstream's `MoveUp`/`MoveDown`. With focus on the results
/// list the "Input" key context is absent from the stack, so those upstream
/// actions are never produced at all and anything keyed on them is dead from
/// that stop — measured, see `view::command_palette`'s module docs.
pub fn register_command_palette_keys(cx: &mut gpui::App) {
    #[cfg(target_os = "macos")]
    let open_ks = "cmd-shift-p";
    #[cfg(not(target_os = "macos"))]
    let open_ks = "ctrl-shift-p";
    cx.bind_keys([
        gpui::KeyBinding::new(open_ks, crate::menu_macos::OpenCommandPalette, None),
        gpui::KeyBinding::new("up", PaletteUp, Some(PALETTE_CONTEXT)),
        gpui::KeyBinding::new("down", PaletteDown, Some(PALETTE_CONTEXT)),
    ]);
    cx.on_action(|_a: &crate::menu_macos::OpenCommandPalette, cx: &mut gpui::App| open(cx));
}

/// Filter actions by a fuzzy subsequence match against the title.
/// Case-insensitive; preserves registry-iteration order.
pub fn filter(reg: &ActionRegistry, query: &str) -> Vec<ActionDescriptor> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return reg.iter().collect();
    }
    reg.iter()
        .filter(|d| subsequence_match(&d.title.to_lowercase(), &q))
        .collect()
}

/// Returns `true` when every char in `needle` appears in `haystack` in
/// order (not necessarily contiguously). Both inputs are expected to be
/// already lowercased by the caller. ASCII-insensitive comparison guards
/// the edge case where the caller passes through a single uppercase
/// letter accidentally.
fn subsequence_match(haystack: &str, needle: &str) -> bool {
    let mut hay = haystack.chars();
    for c in needle.chars() {
        match hay.find(|h| h.eq_ignore_ascii_case(&c)) {
            Some(_) => continue,
            None => return false,
        }
    }
    true
}

/// Match quality, higher is better. `None` = no match at all.
///
/// 3 = the title starts with the query; 2 = some word in the title starts with
/// it; 1 = the query is a subsequence, which is all [`filter`] itself requires.
/// Split out from `filter` rather than folded into it because `filter`'s
/// signature is pinned by `tests/command_palette.rs`.
fn rank(title: &str, query: &str) -> Option<u8> {
    let t = title.to_lowercase();
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(1);
    }
    if t.starts_with(&q) {
        return Some(3);
    }
    if t.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&q))
    {
        return Some(2);
    }
    subsequence_match(&t, &q).then_some(1)
}

/// The palette's data source: everything matching `query`, minus [`HIDDEN`], in
/// a DETERMINISTIC order (score descending, then title ascending).
///
/// `ActionRegistry::iter` snapshots a `HashMap`, so without this sort the list
/// would reshuffle between frames and Enter would run a different command than
/// the one the ring was on.
pub fn visible_items(reg: &ActionRegistry, query: &str) -> Vec<ActionDescriptor> {
    let mut scored: Vec<(u8, ActionDescriptor)> = filter(reg, query)
        .into_iter()
        .filter(|d| !HIDDEN.contains(&d.id.as_str()))
        .filter_map(|d| rank(&d.title, query).map(|s| (s, d)))
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.title.cmp(&b.title)));
    scored.into_iter().map(|(_, d)| d).collect()
}

/// Ask the focused workspace to mount the palette on its next frame.
///
/// Rewritten in B4 T3 — until then this is the P3b breadcrumb.
pub fn open(_app: &mut gpui::App) {
    tracing::info!("command_palette::open invoked — mount lands in B4 T3");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};
    use std::sync::Arc;

    fn reg_with(titles: &[(&str, &str)]) -> ActionRegistry {
        let reg = ActionRegistry::new();
        for (id, title) in titles {
            reg.register(ActionDescriptor {
                id: ActionId::from(*id),
                title: (*title).to_string(),
                group: ActionGroup::Navigation,
                keybinding: None,
                dispatch: Arc::new(|_| {}),
            })
            .expect("unique id");
        }
        reg
    }

    #[test]
    fn subsequence_match_basic() {
        assert!(subsequence_match("new window", "new"));
        assert!(subsequence_match("new window", "nw"));
        assert!(subsequence_match("new window", "ndw"));
        assert!(!subsequence_match("new window", "xyz"));
        assert!(!subsequence_match("abc", "abcd"));
    }

    #[test]
    fn subsequence_match_case_insensitive_path() {
        // Free-form sanity check that uppercase singletons still match.
        assert!(subsequence_match("new window", "NW"));
    }

    #[test]
    fn subsequence_match_empty_needle_is_trivially_true() {
        assert!(subsequence_match("anything", ""));
    }

    #[test]
    fn rank_prefers_prefix_then_word_boundary_then_subsequence() {
        assert_eq!(rank("Cancel Import", "can"), Some(3));
        assert_eq!(rank("Toggle SQL Console", "con"), Some(2), "word-boundary");
        assert_eq!(rank("Toggle SQL Console", "tsc"), Some(1), "subsequence");
        assert_eq!(rank("New Window", "xyz"), None);
        // "Cancel Import" holds no `n` after its only `o`, so it is not even a
        // subsequence match — the first draft of the test below assumed it was.
        assert_eq!(rank("Cancel Import", "con"), None);
    }

    /// Every title here sorts alphabetically in a DIFFERENT order than it
    /// scores, so the assertion fails if any two score tiers collapse into one.
    /// The first draft used titles whose alphabetical order happened to match
    /// the score order, and it stayed green with the word-boundary tier
    /// deliberately broken — a test that could not fail.
    #[test]
    fn visible_items_orders_by_score_then_title() {
        let reg = reg_with(&[
            ("a.one", "Add Console"),
            ("a.two", "Copy Column Name"),
            ("a.three", "Console Colors"),
        ]);
        let titles: Vec<String> = visible_items(&reg, "con")
            .into_iter()
            .map(|d| d.title)
            .collect();
        assert_eq!(
            titles,
            vec![
                "Console Colors".to_string(),   // 3: prefix
                "Add Console".to_string(),      // 2: word boundary
                "Copy Column Name".to_string(), // 1: subsequence (c-o-…-n)
            ],
            "alphabetical order would be Add / Console / Copy — so this only \
             passes if all three tiers are distinct"
        );
    }

    #[test]
    fn empty_query_lists_everything_visible_alphabetically() {
        let reg = reg_with(&[("a.z", "Zebra"), ("a.a", "Apple")]);
        let titles: Vec<String> = visible_items(&reg, "")
            .into_iter()
            .map(|d| d.title)
            .collect();
        assert_eq!(titles, vec!["Apple".to_string(), "Zebra".to_string()]);
    }

    #[test]
    fn hidden_ids_never_surface() {
        let reg = reg_with(&[
            ("theme.toggle", "Toggle Theme"),
            ("window.new", "New Window"),
        ]);
        let ids: Vec<String> = visible_items(&reg, "")
            .into_iter()
            .map(|d| d.id.as_str().to_string())
            .collect();
        assert_eq!(ids, vec!["window.new".to_string()]);
    }

    #[test]
    fn hidden_and_window_routed_are_disjoint() {
        for id in HIDDEN {
            assert!(
                !WINDOW_ROUTED.contains(id),
                "{id} is both hidden and routed — one of the two lists is wrong"
            );
        }
    }
}
