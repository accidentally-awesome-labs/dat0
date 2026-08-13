//! The shared keymap is complete, unambiguous, and honest about what it binds.
//!
//! Moved out of the GPUI crate during the migration and retargeted at
//! `dat0_core::keymap`, where the table has lived since Phase 1. What stayed
//! behind was the half that cross-checked gpui `actions!` declarations and
//! installed live `KeyBinding`s — machinery with no successor.
//!
//! These four guarantees outlive any toolkit:
//!
//! 1. every row's `action_id` names a registered action, and its chord resolves
//! 2. every registered action is either bound or explicitly declared chord-less
//! 3. the macOS-only rows are exactly the window-management chords
//! 4. no two rows share a chord within a context

use std::collections::{BTreeMap, BTreeSet};

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::keymap::{Binding, DEFAULT_KEYMAP, chord_for};

/// Registered [`dat0_core::actions::registry::ActionId`]s that deliberately have
/// no default chord.
///
/// This list is the point of the gate: a new action that is neither bound nor
/// listed here fails `every_action_id_is_bound_or_explicitly_unbound`, so it
/// cannot ship undiscoverable by accident. Adding an id here is a decision
/// ("palette-only"), not bookkeeping.
///
/// Why each family is chord-less:
///
/// - `window.new` — the obvious guess is ⌘N and nothing binds it; a hint would
///   lie. It wants its own slice with a reachability assertion.
/// - `view.copy` … `view.delete_column` — grid editing runs on the grid's own
///   raw `on_key_down` cursor grammar (`grid/keymap.rs`), which is a modal mode
///   rather than a set of global commands and is deliberately outside
///   `DEFAULT_KEYMAP`.
/// - `sql.new_tab`, `sql.close_tab` and the P5b reuse/promotion actions — a
///   global chord would collide with the SQL editor's own text-editing keymap.
/// - everything else — menu items, panel buttons, or palette-only entries
///   (`perf.hud.toggle` is a diagnostic and would spend one of the few free
///   chords).
const UNBOUND: &[&str] = &[
    "ai.panel.open",
    "chart.export.png",
    "chart.export.svg",
    "chart.visualize",
    "file.open",
    "import.cancel",
    "live.refresh",
    "onboarding.take_tour",
    "perf.hud.toggle",
    "recents.show",
    "recovery.review",
    "sample_data.retry_taxi",
    // EN4: reachable only from the `SessionSlot::Failed` banner. A chord for a
    // state the user can only be in when nothing else works would be spent.
    "session.retry",
    "settings.open",
    "sql.close_tab",
    "sql.history",
    "sql.load_query",
    "sql.new_tab",
    "sql.save_as_table",
    "sql.save_query",
    "theme.toggle",
    "view.copy",
    "view.cut",
    "view.delete_column",
    "view.delete_rows",
    "view.fill_down",
    "view.paste",
    "view.save_as_table",
    "view.set_null",
    "view.set_value",
    "window.new",
    "workspace.open",
    "workspace.save",
];

/// (3) Every `action_id` resolves in a freshly-registered `ActionRegistry`.
///
/// A row pointing at an id that no longer exists renders no palette hint and
/// looks exactly like "this command has no shortcut".
#[test]
fn every_action_id_resolves_in_the_registry() {
    let reg = ActionRegistry::new();
    register_all(&reg).expect("register_all");

    for b in DEFAULT_KEYMAP {
        let Some(id) = b.action_id else { continue };
        assert!(
            reg.contains(id),
            "keymap row {} points at action_id {id}, which register_all does not register",
            b.action.unwrap_or("(no gpui action)")
        );
        // And the hint the palette will render actually resolves.
        assert!(
            chord_for(id).is_some(),
            "chord_for({id}) is None on this platform though the row exists"
        );
    }
}

/// (4) Every registered action is either bound or explicitly declared
/// chord-less. A new action cannot ship undiscoverable by accident.
#[test]
fn every_action_id_is_bound_or_explicitly_unbound() {
    let reg = ActionRegistry::new();
    register_all(&reg).expect("register_all");

    let bound: BTreeSet<&str> = DEFAULT_KEYMAP.iter().filter_map(|b| b.action_id).collect();
    let unbound: BTreeSet<&str> = UNBOUND.iter().copied().collect();
    assert_eq!(
        unbound.len(),
        UNBOUND.len(),
        "UNBOUND has a duplicate entry"
    );

    let registered: BTreeSet<String> = reg.iter().map(|d| d.id.as_str().to_string()).collect();

    let undeclared: Vec<&String> = registered
        .iter()
        .filter(|id| !bound.contains(id.as_str()) && !unbound.contains(id.as_str()))
        .collect();
    assert!(
        undeclared.is_empty(),
        "these actions are neither in DEFAULT_KEYMAP nor in UNBOUND — give them a \
         chord or declare them palette-only: {undeclared:?}"
    );

    // Stale-under arm: an UNBOUND entry for an id that no longer exists, or that
    // has since been bound, is a lie about the current surface.
    let stale: Vec<&&str> = UNBOUND
        .iter()
        .filter(|id| !registered.contains(**id) || bound.contains(**id))
        .collect();
    assert!(
        stale.is_empty(),
        "UNBOUND entries that are no longer unbound registered actions: {stale:?}"
    );

    // Every id a keymap row claims must be registered — covered by test (3) —
    // and every bound id must NOT be in UNBOUND, covered by the stale arm.
    assert_eq!(
        bound.len(),
        7,
        "seven registry actions carry a default chord (undo, redo, export, sql.run, \
         sql.cancel, console.toggle, sidebar.toggle); found {bound:?}"
    );
}

/// The macOS-only rows are exactly the window-management chords, and every
/// other row has a chord on both platforms. Guards the `other: None` doc claim
/// from drifting into "someone forgot to fill this in".
#[test]
fn only_the_window_management_chords_are_macos_only() {
    let macos_only: Vec<&str> = DEFAULT_KEYMAP
        .iter()
        .filter(|b: &&Binding| b.other.is_none())
        .filter_map(|b| b.action)
        .collect();
    assert_eq!(
        macos_only,
        vec![
            "dat0_menu::Quit",
            "dat0_menu::CloseWindow",
            "dat0_menu::Minimize",
        ]
    );
}

/// (2) No two rows share a chord within a context. A duplicate is a binding
/// that silently never fires: gpui sorts matches by context depth and takes the
/// later registration first, so the loser is unreachable.
#[test]
fn no_duplicate_chord_within_a_context() {
    let mut seen_macos: BTreeMap<(Option<&str>, &str), &str> = BTreeMap::new();
    let mut seen_other: BTreeMap<(Option<&str>, &str), &str> = BTreeMap::new();

    for b in DEFAULT_KEYMAP {
        let key = (b.context, b.macos);
        if let Some(prev) = seen_macos.insert(key, b.action.unwrap_or("(no gpui action)")) {
            panic!(
                "macOS chord {:?} in context {:?} is bound twice: {prev} and {}",
                b.macos,
                b.context,
                b.action.unwrap_or("(no gpui action)")
            );
        }
        if let Some(other) = b.other {
            let key = (b.context, other);
            if let Some(prev) = seen_other.insert(key, b.action.unwrap_or("(no gpui action)")) {
                panic!(
                    "non-macOS chord {other:?} in context {:?} is bound twice: {prev} and {}",
                    b.context,
                    b.action.unwrap_or("(no gpui action)")
                );
            }
        }
    }
    assert_eq!(
        DEFAULT_KEYMAP.len(),
        17,
        "the SH4 migration moved 16 bindings verbatim and the GPUI→Dioxus \
         migration added ⌘B for the catalog sidebar (S1), which this shell \
         does not implement; changing the count is a behaviour change and \
         wants its own review"
    );
}
