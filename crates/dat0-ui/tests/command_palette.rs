//! Command-palette behaviour.
//!
//! The ranking itself is unit-tested in `dat0_core::command_palette`; what is
//! proven here is everything the *surface* owns and the model cannot: that a
//! keystroke reaches the ring through the real keymap, that Enter names the row
//! the ring is actually on, that a narrowing query resets the ring rather than
//! leaving it pointing at a different command, and that the list is windowed —
//! the palette lists every registered action, and this was the app's only
//! `uniform_list`.
//!
//! Arrows are driven as real `ArrowUp`/`ArrowDown` keystrokes through
//! `keys::Cascade`, not by calling a mover, because the palette's arrows are
//! rows in `dat0_core::keymap` and a green test over a dead key path is exactly
//! what the GPUI suite's `arrows_*` pair existed to prevent.

mod support;

use std::cell::RefCell;
use std::sync::Arc;

use dioxus::prelude::*;
use futures::channel::mpsc::UnboundedReceiver;

use dat0_core::actions::registry::{
    ActionDescriptor, ActionGroup, ActionId, ActionRegistry, DispatchFn,
};
use dat0_core::events::{AppEvent, AppEvents};
use dat0_ui::components::command_palette::CommandPalette;
use dat0_ui::state::Workspace;
use support::{Harness, Key, Modifiers};

thread_local! {
    /// The registry and bus the host component picks up. A thread-local rather
    /// than a prop because the palette reads both from context, exactly as the
    /// shell provides them — a prop would test a wiring production does not
    /// have. Each `#[test]` owns its thread, so each owns its probe.
    static PROBE: RefCell<Option<(ActionRegistry, AppEvents)>> = const { RefCell::new(None) };
}

/// Mount the palette over a probe registry, already open.
///
/// Returns the harness and the bus receiver, which is how a test observes what
/// a dispatch actually named.
fn mount(reg: ActionRegistry) -> (Harness, UnboundedReceiver<AppEvent>) {
    let (events, rx) = AppEvents::channel();
    PROBE.with(|p| *p.borrow_mut() = Some((reg, events)));
    (Harness::new(Host, ()), rx)
}

#[component]
fn Host() -> Element {
    let ws = Workspace::provide();
    let (reg, events) = PROBE.with(|p| p.borrow().clone().expect("a probe was installed"));
    use_context_provider(|| reg);
    use_context_provider(|| events);
    // Once, on mount: writing this every render would re-dirty the tree forever
    // and the harness would report a hang.
    use_hook(move || {
        let mut ws = ws;
        ws.palette.set(true);
    });

    let open = *ws.palette.read();
    rsx! {
        // The gate's own state, so "Escape closed it" is an assertion about the
        // workspace signal and not merely about a missing element.
        div { "data-a11y-id": "open", "{open}" }
        CommandPalette {}
    }
}

/// The dispatch every built-in uses: name the action, let the shell perform it.
/// Reproduced here rather than reached for, because `builtin::run` is
/// crate-private — and a probe that dispatched differently from a real
/// descriptor would prove nothing about the palette.
fn dispatch(id: &'static str) -> DispatchFn {
    Arc::new(move |events: &AppEvents| events.send(AppEvent::RunAction { id, window: None }))
}

fn reg_with(rows: &[(&'static str, &'static str)]) -> ActionRegistry {
    let reg = ActionRegistry::new();
    for (id, title) in rows {
        reg.register(ActionDescriptor {
            id: ActionId::from(*id),
            title: (*title).to_string(),
            group: ActionGroup::Navigation,
            dispatch: dispatch(id),
        })
        .expect("unique id");
    }
    reg
}

/// Titles whose alphabetical order differs from their score order, so a test
/// that asserts the order cannot pass with a tier collapsed.
fn three() -> ActionRegistry {
    reg_with(&[
        ("a.one", "Add Console"),
        ("a.two", "Copy Column Name"),
        ("a.three", "Console Colors"),
    ])
}

fn many(n: usize) -> ActionRegistry {
    let reg = ActionRegistry::new();
    for i in 0..n {
        reg.register(ActionDescriptor {
            id: ActionId::from(format!("probe.{i:04}")),
            title: format!("Probe Action {i:04}"),
            group: ActionGroup::Navigation,
            // Nothing in the virtualization tests runs a command.
            dispatch: Arc::new(|_| {}),
        })
        .expect("unique id");
    }
    reg
}

/// Type into the query field. `oninput` is what the browser sends; the value
/// attribute follows from state, never the other way round.
fn type_query(h: &mut Harness, text: &str) {
    let field = h.by_a11y_id("palette-query").expect("the query field");
    h.dispatch(
        field,
        "input",
        dioxus::html::SerializedFormData::new(text.to_string(), Vec::new()),
    );
}

/// A key at the palette panel — where the handler lives, and where a keystroke
/// from the focused field would bubble to.
fn key(h: &mut Harness, k: Key) {
    h.key_at("palette", k, Modifiers::empty());
}

fn row_ids(h: &Harness) -> Vec<usize> {
    let mut out: Vec<usize> = h
        .dom()
        .walk()
        .into_iter()
        .filter_map(|k| {
            h.dom()
                .get(k)
                .attr("data-a11y-id")?
                .strip_prefix("palette-row-")
                .and_then(|n| n.parse().ok())
        })
        .collect();
    out.sort_unstable();
    out
}

fn row_titles(h: &Harness) -> Vec<String> {
    row_ids(h)
        .into_iter()
        .map(|i| {
            let k = h.by_a11y_id(&format!("palette-row-{i}")).unwrap();
            h.attr(k, "aria-label").unwrap_or_default()
        })
        .collect()
}

fn selected(h: &Harness) -> Option<usize> {
    row_ids(h).into_iter().find(|i| {
        let k = h.by_a11y_id(&format!("palette-row-{i}")).unwrap();
        h.attr(k, "aria-selected").as_deref() == Some("true")
    })
}

fn is_open(h: &Harness) -> bool {
    h.text_of(h.by_a11y_id("open").expect("the gate probe")) == "true"
}

// ── the list ────────────────────────────────────────────────────────────────

#[test]
fn an_open_palette_lists_every_visible_action_alphabetically() {
    let (h, _rx) = mount(three());
    assert_eq!(
        row_titles(&h),
        vec!["Add Console", "Console Colors", "Copy Column Name"]
    );
    assert_eq!(selected(&h), Some(0), "the ring starts on the first row");
}

#[test]
fn typing_filters_and_reranks() {
    let (mut h, _rx) = mount(reg_with(&[
        ("a.one", "Add Console"),
        ("a.two", "Copy Column Name"),
        ("a.three", "Console Colors"),
        ("a.four", "Zebra"),
    ]));

    type_query(&mut h, "con");

    assert_eq!(
        row_titles(&h),
        vec![
            // 3: prefix, 2: word boundary, 1: subsequence (c-o-…-n). The
            // alphabetical order is Add / Console / Copy, so this only holds if
            // the score tiers survived the trip through the component.
            "Console Colors",
            "Add Console",
            "Copy Column Name",
        ],
        "the query must re-rank, not merely filter"
    );
}

#[test]
fn a_narrowing_keystroke_resets_the_ring_to_the_first_row() {
    // Row 2 of the old list is a different command than row 2 of the new one,
    // and Enter would run it.
    let (mut h, _rx) = mount(three());
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::ArrowDown);
    assert_eq!(selected(&h), Some(2));

    type_query(&mut h, "con");
    assert_eq!(selected(&h), Some(0));
}

#[test]
fn no_matches_shows_the_empty_state_and_no_rows() {
    let (mut h, mut rx) = mount(three());
    type_query(&mut h, "zzzz");

    assert!(row_ids(&h).is_empty());
    let empty = h.by_a11y_id("palette-empty").expect("the empty state");
    assert_eq!(h.text_of(empty), dat0_i18n::t("palette.no_results"));

    // And Enter on nothing runs nothing, rather than the row the ring pointed
    // at before the query narrowed.
    key(&mut h, Key::Enter);
    assert!(rx.try_recv().is_err(), "an empty list has nothing to run");
    assert!(is_open(&h), "a no-op Enter must not dismiss the palette");
}

// ── keyboard ────────────────────────────────────────────────────────────────

#[test]
fn arrows_move_the_ring_through_the_real_keymap() {
    let (mut h, _rx) = mount(three());
    key(&mut h, Key::ArrowDown);
    assert_eq!(selected(&h), Some(1));
    key(&mut h, Key::ArrowDown);
    assert_eq!(selected(&h), Some(2));
    key(&mut h, Key::ArrowUp);
    assert_eq!(selected(&h), Some(1));
}

/// The wrap-around question, answered the way the GPUI palette answered it: a
/// list surface CLAMPS. Only a radio group wraps. If this ever flips, the ring
/// lands on a command the user did not walk to.
#[test]
fn the_ring_clamps_at_both_ends_rather_than_wrapping() {
    let (mut h, _rx) = mount(three());

    key(&mut h, Key::ArrowUp);
    assert_eq!(selected(&h), Some(0), "up from the top stays at the top");

    for _ in 0..5 {
        key(&mut h, Key::ArrowDown);
    }
    assert_eq!(
        selected(&h),
        Some(2),
        "down past the end stays on the last row"
    );
}

#[test]
fn enter_dispatches_the_action_the_ring_is_on_and_dismisses() {
    let (mut h, mut rx) = mount(three());
    // Alphabetical: Add Console / Console Colors / Copy Column Name.
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);

    match rx.try_recv() {
        Ok(AppEvent::RunAction { id, .. }) => assert_eq!(id, "a.three"),
        other => panic!("expected RunAction(a.three), got {other:?}"),
    }
    assert!(!is_open(&h), "running a command dismisses the palette");
    assert!(h.by_a11y_id("palette").is_none());
}

#[test]
fn escape_closes_without_running_anything() {
    let (mut h, mut rx) = mount(three());
    key(&mut h, Key::Escape);

    assert!(!is_open(&h));
    assert!(h.by_a11y_id("palette").is_none());
    assert!(rx.try_recv().is_err(), "Escape runs nothing");
}

#[test]
fn a_chord_the_palette_does_not_own_keeps_bubbling() {
    // ⌘Z is a global row. Swallowing it here would make Undo dead whenever the
    // palette happens to be open, and the shell root is what dispatches it.
    let (mut h, _rx) = mount(three());
    let panel = h.by_a11y_id("palette").unwrap();
    h.key(panel, Key::Character("z".into()), Modifiers::META);
    assert!(
        is_open(&h),
        "an unclaimed chord must not disturb the palette"
    );
    assert_eq!(selected(&h), Some(0));
}

// ── mouse ───────────────────────────────────────────────────────────────────

#[test]
fn clicking_a_row_runs_it() {
    let (mut h, mut rx) = mount(three());
    h.click("palette-row-2");

    match rx.try_recv() {
        Ok(AppEvent::RunAction { id, .. }) => assert_eq!(id, "a.two"),
        other => panic!("expected RunAction(a.two), got {other:?}"),
    }
    assert!(!is_open(&h));
}

#[test]
fn clicking_the_scrim_closes_the_palette() {
    let (mut h, mut rx) = mount(three());
    h.click("palette-scrim");
    assert!(!is_open(&h));
    assert!(rx.try_recv().is_err());
}

// ── virtualization ──────────────────────────────────────────────────────────

/// The palette can list every registered action; the DOM must not.
#[test]
fn the_list_windows_rather_than_rendering_every_row() {
    let (h, _rx) = mount(many(400));

    let rows = row_ids(&h);
    // 320px of list / 26px rows ≈ 13, plus 4 rows of overscan each side.
    assert!(
        rows.len() <= 24,
        "the whole list reached the DOM: {} rows",
        rows.len()
    );
    assert!(rows.contains(&0));
    assert!(
        h.by_a11y_id("palette-row-399").is_none(),
        "the last row is nowhere near the fold"
    );
}

/// …and the window follows the ring, or a keyboard user loses it off the fold.
#[test]
fn arrowing_past_the_fold_moves_the_window() {
    let (mut h, _rx) = mount(many(400));
    for _ in 0..60 {
        key(&mut h, Key::ArrowDown);
    }

    assert_eq!(selected(&h), Some(60));
    assert!(
        h.by_a11y_id("palette-row-60").is_some(),
        "the selected row must be rendered"
    );
    assert!(
        h.by_a11y_id("palette-row-0").is_none(),
        "the window scrolled, so the first row left the DOM"
    );
    assert!(row_ids(&h).len() <= 24, "the window stayed a window");
}
