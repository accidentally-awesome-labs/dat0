//! The command palette's model: which registry descriptors are fit to show,
//! and how they rank against a query.
//!
//! Toolkit-free on purpose. The GPUI build already split this from its view so
//! ranking could be unit-tested with no `Window`; the split survives the move
//! to Dioxus for the same reason, and now the two renderers cannot disagree
//! about what the palette contains.
//!
//! # Filter shape
//!
//! [`filter`] is a fuzzy subsequence match, case-insensitive, against
//! [`ActionDescriptor::title`], preserving registry-iteration order. Ordering
//! and visibility are layered on top by [`visible_items`], because
//! `ActionRegistry::iter` snapshots a `HashMap` and would otherwise reshuffle
//! the list between frames — and a reshuffle between the keystroke and the
//! Enter runs a different command than the one the ring was on.
//!
//! # What did NOT come across from GPUI
//!
//! The old module also carried `WINDOW_ROUTED`: seven ids whose registry
//! closure was a breadcrumb because `DispatchFn`'s `Fn(&mut App)` could not
//! reach a `Window`, so the shell had to special-case them. That distinction is
//! gone — every descriptor now posts the same [`AppEvent::RunAction`] and the
//! shell's router performs it — so a second list of ids to keep in step would
//! be pure drift.
//!
//! [`AppEvent::RunAction`]: crate::events::AppEvent::RunAction

use crate::actions::registry::{ActionDescriptor, ActionRegistry};

/// Registered but never shown in the palette. Each entry is dead for a reason a
/// no-arg invocation cannot fix:
///
/// - `view.set_value` needs a `Scalar` and `view.delete_column` needs a
///   `col_ix`; the grid's context menu passes both through the coordinate it
///   right-clicked. A fuzzy search box has neither, and always will not.
/// - `recents.show` opens the palette, and a palette row that reopens the
///   palette is a mirror facing a mirror.
///
/// `file.open`, `theme.toggle` and `sample_data.retry_taxi` used to be here
/// too, hidden because the shell had nowhere to route them and an entry that
/// runs nothing is the greyed-out-menu-item defect PRs #59/#60 fixed. The
/// Dioxus shell's router claims all three, and `dat0-ui`'s `action_routing`
/// gate now fails the build if any listed command stops being claimed — so the
/// condition this list was written under no longer holds for them.
pub const HIDDEN: &[&str; 3] = &["recents.show", "view.set_value", "view.delete_column"];

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
/// signature is pinned by its callers and its tests.
pub fn rank(title: &str, query: &str) -> Option<u8> {
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
pub fn visible_items(reg: &ActionRegistry, query: &str) -> Vec<ActionDescriptor> {
    let mut scored: Vec<(u8, ActionDescriptor)> = filter(reg, query)
        .into_iter()
        .filter(|d| !HIDDEN.contains(&d.id.as_str()))
        .filter_map(|d| rank(&d.title, query).map(|s| (s, d)))
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.title.cmp(&b.title)));
    scored.into_iter().map(|(_, d)| d).collect()
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
        // `recents.show` rather than `theme.toggle`: the latter was unhidden
        // once the Dioxus shell's router grew an arm for it, and a test whose
        // exemplar is no longer an example of the thing proves nothing.
        let reg = reg_with(&[
            ("recents.show", "Show Recents"),
            ("window.new", "New Window"),
        ]);
        let ids: Vec<String> = visible_items(&reg, "")
            .into_iter()
            .map(|d| d.id.as_str().to_string())
            .collect();
        assert_eq!(ids, vec!["window.new".to_string()]);
    }

    /// A stale id in [`HIDDEN`] hides nothing and reads as if it does, which is
    /// how a real command silently stays out of the palette after a rename.
    #[test]
    fn every_hidden_id_is_a_real_registered_action() {
        let reg = ActionRegistry::new();
        crate::actions::builtin::register_all(&reg).expect("built-ins register");
        for id in HIDDEN {
            assert!(reg.contains(id), "{id} is hidden but not registered");
        }
    }
}
