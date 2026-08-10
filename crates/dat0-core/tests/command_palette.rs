//! The command palette's model, driven by the REAL built-in registry.
//!
//! `dat0_core::command_palette`'s own unit tests answer the algorithm questions
//! (subsequence, score tiers, hidden ids) over synthetic registries. What they
//! cannot answer is whether the algorithm still finds anything once it is
//! pointed at the actions dat0 actually ships: a rename in `builtin.rs`, a new
//! entry in `HIDDEN`, or an i18n key that stopped resolving would leave every
//! one of those unit tests green and the palette empty.
//!
//! Ported from `dat0-app/tests/command_palette.rs`, whose subject moved to
//! `dat0-core` in Phase 5. Toolkit-free on both sides — there is no UI in any
//! of it, so there is no VirtualDom here either.

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::command_palette::{HIDDEN, filter, visible_items};

fn builtins() -> ActionRegistry {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("the built-ins register");
    reg
}

#[test]
fn the_shipped_registry_has_enough_actions_to_be_worth_a_palette() {
    // Spec §P3 exit #4. A palette over two commands is a menu with extra steps.
    let reg = builtins();
    assert!(
        reg.count() >= 5,
        "the palette needs at least five actions, got {}",
        reg.count()
    );
}

#[test]
fn a_word_from_a_real_command_finds_it() {
    let titles: Vec<String> = filter(&builtins(), "new")
        .into_iter()
        .map(|d| d.title)
        .collect();
    assert!(
        titles.contains(&"New Window".to_string()),
        "typing a word of a shipped command must find it; got {titles:?}"
    );
}

#[test]
fn an_empty_query_matches_every_registered_action() {
    let reg = builtins();
    assert_eq!(filter(&reg, "").len(), reg.count());
}

/// The fuzzy half, against a real title rather than a fixture: initials are how
/// a keyboard user reaches a command they know the name of.
#[test]
fn initials_match_a_real_command() {
    let titles: Vec<String> = filter(&builtins(), "nw")
        .into_iter()
        .map(|d| d.title)
        .collect();
    assert!(
        titles.contains(&"New Window".to_string()),
        "'nw' must fuzzy-match 'New Window'; got {titles:?}"
    );
}

/// The chart export commands are the only keyboard path to chart export — the
/// buttons live in a pane header, which is not a tab stop. They are
/// deliberately NOT in `HIDDEN`: that list is for actions dead by construction,
/// whereas these work whenever a chart is rendered.
#[test]
fn chart_export_stays_reachable_from_the_palette() {
    let ids: Vec<String> = visible_items(&builtins(), "export chart")
        .into_iter()
        .map(|d| d.id.as_str().to_string())
        .collect();

    for id in ["chart.export.png", "chart.export.svg"] {
        assert!(
            ids.contains(&id.to_string()),
            "{id} must be reachable from the palette; got {ids:?}"
        );
    }
}

/// A stale id in `HIDDEN` hides nothing while reading as if it does — which is
/// how a real command silently stays out of the palette after a rename.
///
/// The GPUI original walked `HIDDEN` *and* `WINDOW_ROUTED`. The second list is
/// gone: every descriptor now posts the same `AppEvent::RunAction` and the
/// shell's router performs it, so there is no set of ids the palette has to
/// special-case.
#[test]
fn every_hidden_id_names_a_real_registered_action() {
    let reg = builtins();
    for id in HIDDEN {
        assert!(
            reg.contains(id),
            "{id} is hidden but not registered — stale"
        );
    }
}

/// …and hiding is not accidentally hiding everything.
#[test]
fn hiding_removes_exactly_the_hidden_ids() {
    let reg = builtins();
    let visible: Vec<String> = visible_items(&reg, "")
        .into_iter()
        .map(|d| d.id.as_str().to_string())
        .collect();

    assert_eq!(
        visible.len(),
        reg.count() - HIDDEN.len(),
        "the palette must drop the hidden ids and nothing else"
    );
    for id in HIDDEN {
        assert!(!visible.contains(&(*id).to_string()), "{id} surfaced");
    }
}
