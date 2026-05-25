//! T6 — Command palette filter algorithm + ≥5-action invariant.
//!
//! The fuzzy-subsequence filter is the load-bearing piece (it's what
//! the user types against); the GPUI overlay view is a stub in
//! `crate::command_palette::open` mirroring the T5 stub policy
//! (Sheet/Modal mount needs `&mut Window` plumbing, deferred). Spec
//! §P3 exit #4 requires ≥5 registered actions, which the seven
//! built-ins from T3 already satisfy.

use dat0_app::actions::registry::ActionRegistry;

#[test]
fn at_least_five_actions_registered() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).unwrap();
    assert!(
        reg.count() >= 5,
        "spec §P3 exit #4 requires ≥5 actions, got {}",
        reg.count()
    );
}

#[test]
fn palette_filters_actions_by_query() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).unwrap();
    let results = dat0_app::command_palette::filter(&reg, "new");
    let titles: Vec<String> = results.iter().map(|d| d.title.clone()).collect();
    assert!(
        titles.contains(&"New Window".to_string()),
        "got: {titles:?}"
    );
}

#[test]
fn palette_empty_query_returns_all() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).unwrap();
    let results = dat0_app::command_palette::filter(&reg, "");
    assert_eq!(results.len(), reg.count());
}

#[test]
fn palette_fuzzy_matches_subsequence() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).unwrap();
    let results = dat0_app::command_palette::filter(&reg, "nw");
    let titles: Vec<String> = results.iter().map(|d| d.title.clone()).collect();
    assert!(
        titles.contains(&"New Window".to_string()),
        "fuzzy 'nw' must match 'New Window': {titles:?}"
    );
}
