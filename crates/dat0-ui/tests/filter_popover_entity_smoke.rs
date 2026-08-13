//! What the filter popover is, before anything drives it: an operator surface
//! for every column type, an existing filter reopened intact, every value
//! widget wired, and exactly one outcome per close.
//!
//! The GPUI suite this replaces was shaped by a limitation, not by the
//! subject. `InputState::new` and `SelectState::new` both needed a `&mut
//! Window`, so `FilterPopoverEntity` lazy-built its widgets on the first
//! `render()` and no headless test could reach them. What was left was
//! construction ("does `new` panic"), direct calls to the state machine, and a
//! count of stored `Subscription` handles standing in for "are the five widget
//! callbacks alive". A `<select>` and four `<input>`s need none of that: the
//! widgets are in the tree on the first pass, so the proxy assertions become
//! real ones.
//!
//! `tests/views_a.rs` already drives the popover's *behaviour* — the operator
//! list per type, `can_apply` gating, ranges, regex validation, the IN list,
//! and `Clear`'s `pre_populated` flag. This file is the rest: the shapes that
//! suite does not reach, and the wiring guarantees the subscription count was
//! standing in for.

mod support;

use dioxus::prelude::*;

use dat0_core::view::filter_popover::{ColumnType, Outcome};
use dat0_engine::{FilterOp, FilterValue, Scalar, Transformation};
use dat0_ui::components::filter_popover::FilterPopover;
use support::{Harness, Key, Modifiers};

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    column_type: ColumnType,
    existing: Option<Transformation>,
    candidates: Vec<String>,
}

/// Records every outcome, so "exactly one" is a countable assertion rather
/// than a hope.
#[component]
fn Host(props: HostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        FilterPopover {
            column: "price".to_string(),
            column_type: props.column_type,
            existing: props.existing.clone(),
            at: (0.0, 0.0),
            candidates: props.candidates.clone(),
            total_distinct: props.candidates.len() as u64,
            on_outcome: move |o: Outcome| {
                log.write().push(match o {
                    Outcome::Apply(t) => format!("apply {t:?}"),
                    Outcome::Cancel => "cancel".to_string(),
                    Outcome::Clear { pre_populated } => format!("clear {pre_populated}"),
                });
            },
        }
        div { "data-a11y-id": "log", "{log.read().join(\"|\")}" }
        div { "data-a11y-id": "log-count", "{log.read().len()}" }
    }
}

fn popover(column_type: ColumnType) -> Harness {
    Harness::new(
        Host,
        HostProps {
            column_type,
            existing: None,
            candidates: Vec::new(),
        },
    )
}

fn typed(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

/// A press. Coordinates are zeroed: the harness has no layout, so any other
/// value would be a fiction.
fn press() -> dioxus::html::SerializedMouseData {
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    let c = Coordinates::new(
        ScreenPoint::new(0.0, 0.0),
        ClientPoint::new(0.0, 0.0),
        ElementPoint::new(0.0, 0.0),
        PagePoint::new(0.0, 0.0),
    );
    dioxus::html::SerializedMouseData::new(
        Some(MouseButton::Primary),
        MouseButton::Primary.into(),
        c,
        Modifiers::empty(),
    )
}

fn log(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("log").unwrap())
}

fn count(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("log-count").unwrap())
}

/// The operator labels, in presentation order.
fn options(h: &Harness) -> Vec<String> {
    let select = h.by_a11y_id("filter-op").expect("the operator select");
    h.dom()
        .get(select)
        .children
        .iter()
        .map(|c| h.text_of(*c))
        .collect()
}

fn pick_op(h: &mut Harness, label_key: &str) {
    let ix = options(h)
        .iter()
        .position(|l| *l == dat0_i18n::t(label_key))
        .unwrap_or_else(|| panic!("no operator labelled {label_key}"));
    let select = h.by_a11y_id("filter-op").unwrap();
    h.dispatch(select, "change", typed(&ix.to_string()));
}

// ── every column type has a usable popover ──────────────────────────────────

/// The GPUI `new_entity_constructs_for_all_column_types` guarded against a
/// column type that panicked on construction. The failure mode that matters is
/// a column type that opens a popover with nothing in it, which is what this
/// asserts instead — and it covers Date and Timestamp, which no other suite
/// mounts.
#[test]
fn no_column_type_is_left_unfilterable() {
    for ct in [
        ColumnType::Numeric,
        ColumnType::String,
        ColumnType::Bool,
        ColumnType::Date,
        ColumnType::Timestamp,
    ] {
        let h = popover(ct);
        assert!(h.by_a11y_id("filter-popover").is_some(), "{ct:?}");
        assert!(
            !options(&h).is_empty(),
            "{ct:?} offers no operators, so the funnel opens a dead panel"
        );
        for control in ["filter-apply", "filter-cancel", "filter-clear"] {
            assert!(
                h.by_a11y_id(control).is_some(),
                "{ct:?} is missing {control}"
            );
        }
    }
}

/// A dialog with no accessible name is an unlabelled box to a screen reader,
/// and this one is modal.
#[test]
fn the_popover_names_the_column_it_filters() {
    let h = popover(ColumnType::Numeric);
    let pop = h.by_a11y_id("filter-popover").unwrap();
    assert_eq!(h.attr(pop, "role").as_deref(), Some("dialog"));
    let label = h.attr(pop, "aria-label").unwrap_or_default();
    assert!(
        label.contains("price"),
        "the name must say which column: {label:?}"
    );
}

// ── reopening an existing filter ────────────────────────────────────────────

/// `views_a.rs` covers the string case (`Contains "abc"`). The numeric one is
/// the case the GPUI suite pinned, and it is the one where the round trip can
/// go wrong: the stored value is a `Scalar::Int`, not a string, so reopening
/// has to render it back into a text field.
#[test]
fn reopening_a_numeric_filter_shows_its_value_and_can_apply_at_once() {
    let existing = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    };
    let mut h = Harness::new(
        Host,
        HostProps {
            column_type: ColumnType::Numeric,
            existing: Some(existing),
            candidates: Vec::new(),
        },
    );

    assert_eq!(
        h.attr(h.by_a11y_id("filter-value").unwrap(), "value")
            .as_deref(),
        Some("42"),
        "the stored Int must come back as editable text"
    );
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("false"),
        "a filter that was valid when stored is valid when reopened"
    );

    // And re-applying it unchanged rebuilds the same filter.
    h.click("filter-apply");
    let emitted = log(&h);
    assert!(emitted.starts_with("apply "), "{emitted}");
    assert!(emitted.contains("Eq"), "{emitted}");
    assert!(emitted.contains("42"), "{emitted}");
}

/// The operator comes back too, not just the value — otherwise reopening a
/// `>` filter silently turns it into an `=`.
#[test]
fn reopening_a_filter_restores_its_operator() {
    let existing = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Int(7),
        },
    };
    let h = Harness::new(
        Host,
        HostProps {
            column_type: ColumnType::Numeric,
            existing: Some(existing),
            candidates: Vec::new(),
        },
    );
    let select = h.by_a11y_id("filter-op").unwrap();
    let selected: Vec<String> = h
        .dom()
        .get(select)
        .children
        .iter()
        .filter(|c| h.dom().get(**c).attr("selected") == Some("true"))
        .map(|c| h.text_of(*c))
        .collect();
    assert_eq!(selected, vec![dat0_i18n::t("filter.op.gte")]);
}

// ── one outcome per close ───────────────────────────────────────────────────

/// The GPUI suite proved `Outcome::Cancel` was emitted by calling
/// `emit_outcome` directly, because no headless test could click the button.
/// Here every way out of the popover is reachable, and each must emit one
/// cancel — a close path that emits none leaves the popover stuck open, and
/// one that emits two runs the caller's dismissal twice.
#[test]
fn every_way_out_emits_exactly_one_cancel() {
    // The button.
    let mut h = popover(ColumnType::Numeric);
    h.click("filter-cancel");
    assert_eq!(log(&h), "cancel");
    assert_eq!(count(&h), "1");

    // Escape.
    let mut h = popover(ColumnType::Numeric);
    let pop = h.by_a11y_id("filter-popover").unwrap();
    h.key(pop, Key::Escape, Modifiers::empty());
    assert_eq!(log(&h), "cancel");
    assert_eq!(count(&h), "1");

    // A click outside, which lands on the dismiss layer.
    let mut h = popover(ColumnType::Numeric);
    let layer = h.by_a11y_id("filter-popover-dismiss").unwrap();
    h.dispatch(layer, "mousedown", press());
    assert_eq!(log(&h), "cancel");
    assert_eq!(count(&h), "1");
}

/// A key that is not Escape must fall through to whatever is typing, or the
/// popover swallows the value being entered.
#[test]
fn an_ordinary_key_does_not_close_the_popover() {
    let mut h = popover(ColumnType::Numeric);
    let pop = h.by_a11y_id("filter-popover").unwrap();
    h.key(pop, Key::Character("a".into()), Modifiers::empty());
    assert_eq!(count(&h), "0");
}

#[test]
fn applying_emits_exactly_one_outcome() {
    let mut h = popover(ColumnType::Numeric);
    h.dispatch(h.by_a11y_id("filter-value").unwrap(), "input", typed("10"));
    h.click("filter-apply");
    assert_eq!(count(&h), "1");
    assert!(log(&h).starts_with("apply "), "{}", log(&h));
}

/// A disabled Apply is not a decoration: the popover refuses to build a
/// half-filled filter even if the click gets through.
#[test]
fn a_click_that_races_the_disabled_apply_emits_nothing() {
    let mut h = popover(ColumnType::Numeric);
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("true")
    );
    h.click("filter-apply");
    assert_eq!(count(&h), "0", "an empty value is not a filter");
}

// ── the widgets are wired ───────────────────────────────────────────────────

/// Replaces the GPUI `subscriptions_stored_not_dropped` guard, which counted
/// five stored `Subscription` handles because the callbacks themselves were
/// invisible and a dropped handle deregistered silently. The five are the same
/// five — operator, single value, range low, range high, list entry — but here
/// the assertion is that each widget carries a live listener rather than that
/// a counter says so. The inclusive checkbox is included because it reaches
/// the built transformation and had no subscription of its own.
#[test]
fn every_value_widget_the_operator_surfaces_is_wired() {
    // 1. The operator select, present for every operator.
    let h = popover(ColumnType::Numeric);
    assert!(h.has_listener(h.by_a11y_id("filter-op").unwrap(), "change"));

    // 2. The single value field.
    assert!(h.has_listener(h.by_a11y_id("filter-value").unwrap(), "input"));

    // 3 + 4. Both range bounds, plus the inclusive box.
    let mut h = popover(ColumnType::Numeric);
    pick_op(&mut h, "filter.op.between");
    for id in ["filter-range-lo", "filter-range-hi"] {
        assert!(
            h.has_listener(h.by_a11y_id(id).unwrap(), "input"),
            "{id} takes no input, so a typed bound never reaches the filter"
        );
    }
    assert!(h.has_listener(h.by_a11y_id("filter-range-inclusive").unwrap(), "change"));

    // 5. The IN-list entry field.
    let mut h = Harness::new(
        Host,
        HostProps {
            column_type: ColumnType::String,
            existing: None,
            candidates: vec!["alpha".into()],
        },
    );
    pick_op(&mut h, "filter.op.in");
    let entry = h.by_a11y_id("filter-list-entry").unwrap();
    assert!(h.has_listener(entry, "input"));
    assert!(
        h.has_listener(entry, "keydown"),
        "Enter is what commits a typed value into the list"
    );
}

/// Switching the operator swaps the value widgets, so a stale field cannot sit
/// under a new operator feeding it a value it will not use.
#[test]
fn changing_the_operator_replaces_the_value_widgets() {
    let mut h = popover(ColumnType::Numeric);
    assert!(h.by_a11y_id("filter-value").is_some());

    pick_op(&mut h, "filter.op.between");
    assert!(
        h.by_a11y_id("filter-value").is_none(),
        "a range operator has no single value field"
    );
    assert!(h.by_a11y_id("filter-range-lo").is_some());

    pick_op(&mut h, "filter.op.is_empty");
    assert!(h.by_a11y_id("filter-range-lo").is_none());
    assert!(
        h.by_a11y_id("filter-value").is_none(),
        "a nullary operator takes no value at all"
    );
}
