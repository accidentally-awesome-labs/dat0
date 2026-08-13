//! The column filter popover, anchored to the header's funnel zone.
//!
//! The state machine is `dat0_core::view::filter_popover::FilterPopoverState`:
//! the operator surface per column type, the parse into a `Scalar`, the
//! `can_apply` gating and the built `Transformation::Filter` all live there and
//! are unit-tested there. This file is the widgets, and nothing here decides
//! what a valid filter is.
//!
//! # What replaced what
//!
//! GPUI needed the pure state and the widget mount in two files, because
//! `InputState::new` and `SelectState::new` both require a `&mut Window` that no
//! headless test could produce — hence `filter_popover.rs` plus
//! `filter_popover_entity.rs`, five stored `Subscription` handles, and a
//! lazy-init dance on the first render. A `<select>` and four `<input>`s need
//! none of it: the widgets are markup, the events are `oninput`/`onchange`, and
//! the whole surface is testable.
//!
//! # Outcome, not mutation
//!
//! The popover never touches the `ViewModel`. It emits [`Outcome`] — `Apply(t)`,
//! `Cancel`, `Clear { pre_populated }` — and the caller routes it to
//! `apply` / `replace_at_cursor` / filter removal, exactly as the GPUI
//! `on_outcome` subscribers did. `Cancel` is also what Escape and a click
//! outside emit, so there is one close path rather than three.

use dioxus::prelude::*;

use dat0_core::view::distinct_values::{TOP_N, banner_needed};
use dat0_core::view::filter_popover::{ColumnType, FilterPopoverState, Outcome, supported_ops_for};
use dat0_engine::{FilterOp, Transformation};

use crate::a11y::AccessRole;

/// The dropdown label for an operator. Glyph plus words, as the GPUI select had:
/// the glyph is what a returning user recognises, the words are what a new one
/// reads.
pub fn op_label(op: FilterOp) -> String {
    dat0_i18n::t(match op {
        FilterOp::Eq => "filter.op.eq",
        FilterOp::Neq => "filter.op.neq",
        FilterOp::Lt => "filter.op.lt",
        FilterOp::Lte => "filter.op.lte",
        FilterOp::Gt => "filter.op.gt",
        FilterOp::Gte => "filter.op.gte",
        FilterOp::Between => "filter.op.between",
        FilterOp::Contains => "filter.op.contains",
        FilterOp::NotContains => "filter.op.not_contains",
        FilterOp::StartsWith => "filter.op.starts_with",
        FilterOp::EndsWith => "filter.op.ends_with",
        FilterOp::In => "filter.op.in",
        FilterOp::Regex => "filter.op.regex",
        FilterOp::IsEmpty => "filter.op.is_empty",
        FilterOp::IsNotEmpty => "filter.op.is_not_empty",
        FilterOp::IsTrue => "filter.op.is_true",
        FilterOp::IsFalse => "filter.op.is_false",
    })
}

/// Which value widgets an operator needs. Derived from the operator rather than
/// stored, so the popover cannot show a range input for `Contains`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    /// Nullary: `IsEmpty`, `IsNotEmpty`, `IsTrue`, `IsFalse`.
    None,
    /// One text field.
    Single,
    /// Lower bound, upper bound, inclusive.
    Range,
    /// The distinct-value chips plus a manual-entry field.
    List,
}

pub fn value_shape(op: FilterOp) -> ValueShape {
    match op {
        FilterOp::IsEmpty | FilterOp::IsNotEmpty | FilterOp::IsTrue | FilterOp::IsFalse => {
            ValueShape::None
        }
        FilterOp::Between => ValueShape::Range,
        FilterOp::In => ValueShape::List,
        _ => ValueShape::Single,
    }
}

#[derive(Clone, PartialEq, Props)]
pub struct FilterPopoverProps {
    pub column: String,
    /// Drives the operator list and how text parses into a `Scalar`.
    pub column_type: ColumnType,
    /// The column's existing filter, when the funnel was clicked on a column
    /// that already has one. Pre-populates every field and makes `Clear`
    /// meaningful.
    #[props(default)]
    pub existing: Option<Transformation>,
    /// Client coordinates of the funnel zone that opened this, so the popover
    /// hangs off the column it filters rather than the middle of the window.
    pub at: (f64, f64),
    /// Top-N distinct values for the column, fetched by the caller. Empty until
    /// that query lands; the panel is usable either way because a value can
    /// always be typed.
    #[props(default)]
    pub candidates: Vec<String>,
    /// Total distinct count, which is what decides whether the candidate list is
    /// truncated. Not `candidates.len()`: that is capped at `TOP_N`.
    #[props(default = 0)]
    pub total_distinct: u64,
    pub on_outcome: EventHandler<Outcome>,
}

#[component]
pub fn FilterPopover(props: FilterPopoverProps) -> Element {
    // Built once per mount. The caller mounts a fresh popover per funnel click
    // (and keys it by column), so there is no stale-column state to reconcile.
    let mut state = use_signal(|| {
        let mut s = match props.existing.as_ref() {
            Some(t) => {
                FilterPopoverState::from_existing(props.column.clone(), props.column_type, t)
            }
            None => FilterPopoverState::new(props.column.clone(), props.column_type),
        };
        // `regex_valid` is derived from `value_text`, and `can_apply` reads it.
        // Leaving it unset after pre-populating an existing Regex filter would
        // mean Apply is dead until the user types a character into a pattern
        // that is already correct.
        //
        // An empty pattern compiles, so it counts as valid and applies as a
        // match-everything filter. That is `can_apply`'s rule, not this file's:
        // it is one undoable op with no data change, and the GPUI build reached
        // the same state as soon as you typed a character and deleted it.
        s.revalidate_regex();
        s
    });

    let ops = supported_ops_for(props.column_type);
    let op = state.read().op;
    let can_apply = state.read().can_apply();
    let (x, y) = props.at;
    let on_outcome = props.on_outcome;
    let ops_for_change = ops.clone();

    // Built outside the markup because each arm is a different set of widgets and
    // one of them is a component with its own state.
    let value_section = match value_shape(op) {
        ValueShape::None => rsx! {},
        ValueShape::Single => rsx! {
            input {
                class: "d0-field",
                "data-a11y-id": "filter-value",
                "aria-label": dat0_i18n::t("filter.value"),
                r#type: "text",
                value: "{state.read().value_text}",
                oninput: move |e| {
                    let mut s = state.write();
                    s.set_value_text(e.value());
                    // Only Regex validates per keystroke; every other operator
                    // has nothing to check until Apply parses the text.
                    if s.op == FilterOp::Regex {
                        s.revalidate_regex();
                    }
                },
            }
        },
        ValueShape::Range => rsx! {
            input {
                class: "d0-field",
                "data-a11y-id": "filter-range-lo",
                "aria-label": dat0_i18n::t("filter.range.lo"),
                r#type: "text",
                value: "{state.read().range_lo}",
                oninput: move |e| state.write().set_range_lo(e.value()),
            }
            input {
                class: "d0-field",
                "data-a11y-id": "filter-range-hi",
                "aria-label": dat0_i18n::t("filter.range.hi"),
                r#type: "text",
                value: "{state.read().range_hi}",
                oninput: move |e| state.write().set_range_hi(e.value()),
            }
            label { class: "d0-mono",
                input {
                    "data-a11y-id": "filter-range-inclusive",
                    r#type: "checkbox",
                    checked: state.read().range_inclusive,
                    onchange: move |e| state.write().set_range_inclusive(e.checked()),
                }
                {dat0_i18n::t("filter.range.inclusive")}
            }
        },
        ValueShape::List => rsx! {
            FilterList {
                state,
                candidates: props.candidates.clone(),
                total: props.total_distinct,
            }
        },
    };

    let regex_hint = match (op, state.read().regex_valid) {
        (FilterOp::Regex, Some(valid)) => {
            let key = if valid {
                "filter.regex.valid"
            } else {
                "filter.regex.invalid"
            };
            let class = if valid {
                "d0-mono is-ok"
            } else {
                "d0-mono is-error"
            };
            rsx! {
                span {
                    class: "{class}",
                    "data-a11y-id": "filter-regex-hint",
                    role: AccessRole::Label.aria(),
                    "aria-label": dat0_i18n::t(key),
                    {dat0_i18n::t(key)}
                }
            }
        }
        _ => rsx! {},
    };

    let title = format!("{}: {}", dat0_i18n::t("grid.filter"), props.column);

    rsx! {
        // Click-outside closes, the same way the grid's context menu does: a
        // transparent layer beneath the popover, because there is no document
        // listener to hang a global mousedown on.
        div {
            class: "d0-menu-dismiss",
            "data-a11y-id": "filter-popover-dismiss",
            onmousedown: move |_| on_outcome.call(Outcome::Cancel),
        }
        div {
            class: "d0-popover",
            "data-a11y-id": "filter-popover",
            role: AccessRole::Dialog.aria(),
            "aria-label": "{title}",
            tabindex: "0",
            autofocus: true,
            style: "left: {x}px; top: {y}px;",
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    e.stop_propagation();
                    on_outcome.call(Outcome::Cancel);
                }
            },

            span { class: "d0-label", "{props.column}" }

            select {
                class: "d0-field",
                "data-a11y-id": "filter-op",
                "aria-label": dat0_i18n::t("filter.op.label"),
                // The option value is the index into `supported_ops_for`, not
                // the operator name: the list is already ordered for display and
                // an index cannot disagree with it.
                onchange: move |e| {
                    let Ok(ix) = e.value().parse::<usize>() else {
                        return;
                    };
                    let Some(next) = ops_for_change.get(ix).copied() else {
                        return;
                    };
                    let mut s = state.write();
                    s.set_op(next);
                    // Keep the derived regex flag in step with whatever text is
                    // already in the field.
                    s.revalidate_regex();
                },
                for (ix, o) in ops.iter().enumerate() {
                    option {
                        key: "{ix}",
                        value: "{ix}",
                        selected: *o == op,
                        "{op_label(*o)}"
                    }
                }
            }

            {value_section}
            {regex_hint}

            div { class: "d0-popover-actions",
                button {
                    class: "d0-btn is-primary",
                    "data-a11y-id": "filter-apply",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("filter.apply"),
                    tabindex: "0",
                    disabled: !can_apply,
                    "aria-disabled": if can_apply { "false" } else { "true" },
                    onclick: move |_| {
                        // `apply_transformation` returns `None` for an invalid
                        // state, so a click that raced the disabled attribute
                        // cannot emit a half-built filter.
                        if let Some(t) = state.read().apply_transformation() {
                            on_outcome.call(Outcome::Apply(t));
                        }
                    },
                    {dat0_i18n::t("filter.apply")}
                }
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "filter-cancel",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("common.cancel"),
                    tabindex: "0",
                    onclick: move |_| {
                        state.read().cancel();
                        on_outcome.call(Outcome::Cancel);
                    },
                    {dat0_i18n::t("common.cancel")}
                }
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "filter-clear",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("filter.clear"),
                    tabindex: "0",
                    onclick: move |_| {
                        // `clear_filter` reports whether there was a stored
                        // filter to retract; the caller needs that to choose
                        // between removing an op and doing nothing.
                        let pre_populated = state.read().clear_filter();
                        on_outcome.call(Outcome::Clear { pre_populated });
                    },
                    {dat0_i18n::t("filter.clear")}
                }
            }
        }
    }
}

/// The IN-list panel: candidate chips, a truncation notice, manual entry.
///
/// A component rather than a helper function because it owns the entry field's
/// text, and a hook inside a conditionally-called helper would shift the parent's
/// hook order the moment the operator changed.
///
/// Chips cover the fetched candidates **and** any selected value that is not
/// among them. The GPUI panel rendered candidates only, so a value typed into the
/// entry field enabled Apply while appearing nowhere — the user could not see,
/// let alone remove, what they had added.
#[component]
fn FilterList(state: Signal<FilterPopoverState>, candidates: Vec<String>, total: u64) -> Element {
    let mut entry = use_signal(String::new);
    let mut state = state;

    let selected = state.read().list_values.clone();
    let extra: Vec<String> = selected
        .iter()
        .filter(|v| !candidates.contains(v))
        .cloned()
        .collect();
    let notice = truncation_notice(total);

    rsx! {
        div { class: "d0-filter-list", "data-a11y-id": "filter-list",
            for (i, value) in candidates.iter().chain(extra.iter()).enumerate() {
                {
                    let v = value.clone();
                    let on = selected.contains(value);
                    rsx! {
                        button {
                            key: "{i}",
                            class: if on { "d0-chip is-on" } else { "d0-chip" },
                            "data-a11y-id": "filter-list-{i}",
                            role: AccessRole::Button.aria(),
                            "aria-label": "{value}",
                            "aria-pressed": if on { "true" } else { "false" },
                            tabindex: "0",
                            onclick: move |_| {
                                let mut s = state.write();
                                match s.list_values.iter().position(|x| *x == v) {
                                    Some(at) => {
                                        s.list_values.remove(at);
                                    }
                                    None => s.list_values.push(v.clone()),
                                }
                            },
                            if on {
                                span { class: "d0-check", "✓" }
                            }
                            "{value}"
                        }
                    }
                }
            }

            if banner_needed(total) {
                span {
                    class: "d0-mono is-muted",
                    "data-a11y-id": "filter-list-truncated",
                    role: AccessRole::Label.aria(),
                    "aria-label": "{notice}",
                    "{notice}"
                }
            }

            input {
                class: "d0-field",
                "data-a11y-id": "filter-list-entry",
                "aria-label": dat0_i18n::t("filter.list.add"),
                r#type: "text",
                placeholder: dat0_i18n::t("filter.list.add"),
                value: "{entry}",
                oninput: move |e| entry.set(e.value()),
                // Enter appends and empties the field. Duplicates are silently
                // ignored, so holding Enter cannot fill the list with one value.
                onkeydown: move |e| {
                    if e.key() != Key::Enter {
                        return;
                    }
                    e.stop_propagation();
                    let trimmed = entry.read().trim().to_string();
                    if trimmed.is_empty() {
                        return;
                    }
                    if !state.read().list_values.contains(&trimmed) {
                        state.write().list_values.push(trimmed);
                    }
                    entry.set(String::new());
                },
            }
        }
    }
}

/// "Showing 50 of 1204 distinct values; type to add more".
fn truncation_notice(total: u64) -> String {
    dat0_i18n::t("filter.list.truncated")
        .replace("{shown}", &TOP_N.to_string())
        .replace("{total}", &total.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every operator the popover can offer must have a label; a missing key
    /// renders as the key itself in a dropdown a user has to read.
    #[test]
    fn every_operator_label_resolves() {
        for ct in [
            ColumnType::Numeric,
            ColumnType::String,
            ColumnType::Bool,
            ColumnType::Date,
            ColumnType::Timestamp,
        ] {
            for op in supported_ops_for(ct) {
                let label = op_label(op);
                assert!(
                    !label.starts_with("filter.op."),
                    "{op:?} has no en.json entry"
                );
                assert!(!label.is_empty(), "{op:?} resolves to an empty label");
            }
        }
    }

    /// The value widgets follow the operator, so a nullary operator cannot leave
    /// a stale text field on screen for the user to fill in to no effect.
    #[test]
    fn the_value_shape_follows_the_operator() {
        assert_eq!(value_shape(FilterOp::IsEmpty), ValueShape::None);
        assert_eq!(value_shape(FilterOp::IsTrue), ValueShape::None);
        assert_eq!(value_shape(FilterOp::Between), ValueShape::Range);
        assert_eq!(value_shape(FilterOp::In), ValueShape::List);
        assert_eq!(value_shape(FilterOp::Contains), ValueShape::Single);
        assert_eq!(value_shape(FilterOp::Regex), ValueShape::Single);
    }

    #[test]
    fn the_truncation_notice_names_both_counts() {
        let notice = truncation_notice(1204);
        assert!(notice.contains("50"), "{notice:?} must name the cap");
        assert!(notice.contains("1204"), "{notice:?} must name the total");
    }
}
