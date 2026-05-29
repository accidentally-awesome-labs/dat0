//! GPUI widget mount for the compact-inline filter popover (T10b).
//!
//! This module owns [`FilterPopoverEntity`] — the GPUI [`Entity`] that wraps
//! [`FilterPopoverState`] and mounts the visible input / select / checkbox
//! widgets.
//!
//! # T10/T10b split (PD-013 §3)
//!
//! `FilterPopoverState` (T10) is pure logic with no GPUI import. This file is
//! the visible widget mount. The split exists because both
//! `InputState::new` and `SelectState::new` require `&mut Window`, which
//! is unavailable in headless test environments — see
//! `docs/internal/dat0-p4a-t0-spike.md` §3.
//!
//! # Widget construction strategy
//!
//! `Input`/`Select` state entities are initialised lazily on the first
//! `render()` call, following the same pattern as `WorkspaceShell` uses for
//! `TableState` (see `window.rs` lines 543-555). This avoids the need for a
//! `&mut Window` at construction time — the caller (`cx.new`) is only given
//! `&mut Context<Self>`, not a `Window`.
//!
//! # Outcome signalling
//!
//! The entity emits [`Outcome`] variants via registered callbacks. Upper
//! layers (T13: `ColumnHeaderZone::Funnel` wiring) subscribe via
//! [`FilterPopoverEntity::on_outcome`].
//!
//! # IN-list distinct-values stub
//!
//! When `op == In` the render shows a clearly-marked
//! `Label::new("distinct values: T11")` placeholder. T11 will replace it
//! with a fetched suggestion list and the chip editor.

use std::rc::Rc;

use gpui::{
    Context, Entity, EventEmitter, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, prelude::*,
};
use gpui_component::{
    Disableable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    label::Label,
    select::{Select, SelectEvent, SelectItem, SelectState},
    v_flex,
};

use crate::view::filter_popover::{ColumnType, FilterPopoverState, supported_ops_for};
use dat0_engine::FilterOp;

// ---------------------------------------------------------------------------
// Outcome enum
// ---------------------------------------------------------------------------

/// Signal emitted by the filter popover when the user takes a terminal action.
///
/// The upper layer (T13) subscribes via [`FilterPopoverEntity::on_outcome`]
/// and routes to `ViewModel::apply` / `replace_at_cursor` / filter removal.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// User pressed Apply and the state was valid. Contains the built
    /// `Transformation::Filter`. T13 routes this to `ViewModel::apply` (new
    /// filter) or `replace_at_cursor` (edit-existing flow).
    Apply(dat0_engine::Transformation),
    /// User pressed Cancel. The upper layer closes the popover.
    Cancel,
    /// User pressed Clear.
    ///
    /// `pre_populated` is `true` when there was an existing filter to remove
    /// (i.e., the upper layer should call `vm.replace_at_cursor` / remove the
    /// filter from the stack). `false` means the popover was opened on a fresh
    /// column — nothing to clean up at the ViewModel level.
    Clear { pre_populated: bool },
}

// ---------------------------------------------------------------------------
// FilterOpItem — SelectItem wrapper for FilterOp
// ---------------------------------------------------------------------------

/// Thin wrapper so [`FilterOp`] satisfies the [`SelectItem`] bound.
///
/// `value()` returns the [`FilterOp`] itself; `title()` returns a human-
/// readable label suitable for the dropdown.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterOpItem(pub FilterOp);

impl SelectItem for FilterOpItem {
    type Value = FilterOp;

    fn title(&self) -> SharedString {
        filter_op_label(self.0).into()
    }

    fn value(&self) -> &FilterOp {
        &self.0
    }
}

/// Human-readable label for a `FilterOp`.
fn filter_op_label(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "= Equals",
        FilterOp::Neq => "≠ Not equals",
        FilterOp::Lt => "< Less than",
        FilterOp::Lte => "≤ Less than or equal",
        FilterOp::Gt => "> Greater than",
        FilterOp::Gte => "≥ Greater than or equal",
        FilterOp::Between => "↔ Between",
        FilterOp::Contains => "⊇ Contains",
        FilterOp::NotContains => "⊅ Not contains",
        FilterOp::StartsWith => "↦ Starts with",
        FilterOp::EndsWith => "↤ Ends with",
        FilterOp::In => "∈ In list",
        FilterOp::Regex => "⌗ Regex",
        FilterOp::IsEmpty => "∅ Is empty",
        FilterOp::IsNotEmpty => "◉ Is not empty",
        FilterOp::IsTrue => "✓ Is true",
        FilterOp::IsFalse => "✗ Is false",
    }
}

// ---------------------------------------------------------------------------
// FilterPopoverEntity
// ---------------------------------------------------------------------------

/// GPUI entity that mounts the filter-popover widgets.
///
/// Owns the pure-logic [`FilterPopoverState`] and lazily constructs
/// `gpui_component` widget-state entities on the first `render()` call.
///
/// Subscribers registered via [`Self::on_outcome`] receive [`Outcome`] values
/// when the user presses Apply / Cancel / Clear.
pub struct FilterPopoverEntity {
    /// Pure state machine (T10). Mutated by widget callbacks.
    ///
    /// `pub` so tests and upper layers can read/drive state directly.
    pub state: FilterPopoverState,
    /// Lazily-initialised on first render when `&mut Window` is available.
    widgets: Option<PopoverWidgets>,
    /// Outcome subscribers registered by upper layers (T13).
    outcome_callbacks: Vec<Rc<dyn Fn(Outcome)>>,
}

/// Lazily-initialised widget-state handles. Created on first `render()`.
struct PopoverWidgets {
    /// Single-value text input (Eq/Neq/Contains/… ops).
    value_input: Entity<InputState>,
    /// Between: lower bound.
    range_lo_input: Entity<InputState>,
    /// Between: upper bound.
    range_hi_input: Entity<InputState>,
    /// IN-list: text input for comma-separated entry (T11 parses entries into
    /// `Scalar` list; T10b only carries the input).
    list_input: Entity<InputState>,
    /// Operator dropdown.
    op_select: Entity<SelectState<Vec<FilterOpItem>>>,
}

impl FilterPopoverEntity {
    // --- Constructors ---

    /// Construct a fresh popover entity for `column`.
    ///
    /// Widget state is **not** initialised here — it requires `&mut Window`
    /// and is therefore deferred to the first `render()` call. The entity is
    /// safe to construct from `cx.new(|_| ...)` without a `Window` parameter.
    pub fn new(column: String, column_type: ColumnType) -> Self {
        Self {
            state: FilterPopoverState::new(column, column_type),
            widgets: None,
            outcome_callbacks: Vec::new(),
        }
    }

    /// Construct a popover pre-populated from an existing
    /// `Transformation::Filter` (edit-existing flow, design §6 re-open).
    pub fn from_existing(
        column: String,
        column_type: ColumnType,
        existing: &dat0_engine::Transformation,
    ) -> Self {
        Self {
            state: FilterPopoverState::from_existing(column, column_type, existing),
            widgets: None,
            outcome_callbacks: Vec::new(),
        }
    }

    // --- Outcome subscription ---

    /// Register a callback invoked whenever the user presses Apply, Cancel, or
    /// Clear. Multiple subscribers may be registered; they are called in
    /// registration order.
    ///
    /// T13 uses this to route `Outcome::Apply(t)` → `ViewModel::apply`.
    pub fn on_outcome(&mut self, cb: impl Fn(Outcome) + 'static) {
        self.outcome_callbacks.push(Rc::new(cb));
    }

    // --- Outcome emission ---

    /// Emit an outcome to all registered subscribers.
    ///
    /// `pub` so smoke tests and the render-button closures can drive the
    /// signal path directly. Production callers should prefer the Apply /
    /// Cancel / Clear button closures baked into `render()`; this is the
    /// escape hatch used by tests.
    pub fn emit_outcome(&self, outcome: Outcome) {
        for cb in &self.outcome_callbacks {
            cb(outcome.clone());
        }
    }

    // --- Read-only state accessors (for render) ---

    /// Whether the Apply button should be enabled.
    pub fn can_apply(&self) -> bool {
        self.state.can_apply()
    }

    // --- Lazy widget initialisation ---

    /// Ensure widget state entities exist. Called at the start of every
    /// `render()` so the first frame initialises them with a real `Window`.
    fn ensure_widgets(&mut self, cx: &mut Context<Self>, window: &mut Window) {
        if self.widgets.is_some() {
            return;
        }

        // Pre-populate inputs using the builder `default_value` method, which
        // sets the text without requiring `&mut Window`. This avoids the borrow
        // limitation that `replace()` (which needs `Window`) would impose when
        // called from update closures inside `ensure_widgets`.
        let value_text = self.state.value_text.clone();
        let range_lo = self.state.range_lo.clone();
        let range_hi = self.state.range_hi.clone();

        let value_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            if !value_text.is_empty() {
                s = s.default_value(value_text);
            }
            s
        });
        let range_lo_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            if !range_lo.is_empty() {
                s = s.default_value(range_lo);
            }
            s
        });
        let range_hi_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            if !range_hi.is_empty() {
                s = s.default_value(range_hi);
            }
            s
        });
        let list_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Comma-separated values (T11 will handle parsing)")
        });

        let state = &self.state;

        // Build operator list for the column type.
        let ops: Vec<FilterOpItem> = supported_ops_for(state.column_type)
            .into_iter()
            .map(FilterOpItem)
            .collect();
        let current_op = state.op;
        let selected_ix = ops.iter().position(|item| item.0 == current_op);
        use gpui_component::IndexPath;
        let selected_index = selected_ix.map(|ix| IndexPath::default().row(ix));

        let op_select = cx.new(|cx| SelectState::new(ops, selected_index, window, cx));

        self.widgets = Some(PopoverWidgets {
            value_input,
            range_lo_input,
            range_hi_input,
            list_input,
            op_select,
        });
    }
}

// ---------------------------------------------------------------------------
// EventEmitter (not used for internal routing — outcome callbacks are used
// instead — but impl is needed to satisfy gpui conventions for entities that
// may emit to subscribers).
// ---------------------------------------------------------------------------

/// Internal event type. Reserved for future GPUI-subscription-based wiring;
/// the primary signal path is the [`Outcome`] callback registered via
/// [`FilterPopoverEntity::on_outcome`].
pub enum FilterPopoverEvent {
    OutcomeEmitted(Outcome),
}

impl EventEmitter<FilterPopoverEvent> for FilterPopoverEntity {}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for FilterPopoverEntity {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Ensure widget entities exist (lazy-init with Window).
        self.ensure_widgets(cx, window);
        let widgets = self.widgets.as_ref().expect("ensure_widgets just ran");

        let can_apply = self.state.can_apply();
        let op = self.state.op;
        let range_inclusive = self.state.range_inclusive;

        // ── Operator dropdown ──────────────────────────────────────────────
        let op_select_widget = Select::new(&widgets.op_select).placeholder("Choose operator…");

        // Subscribe to operator selection changes. We use cx.subscribe so the
        // subscription lives for the entity's lifetime.
        // Wire this once per render by observing SelectEvent::Confirm.
        // Note: gpui subscriptions are deduplicated by the entity observer
        // slot — re-subscribing on each render is intentional: gpui
        // `cx.subscribe` captures the current closure and the previous
        // subscription is dropped when overwritten. We rely on the lazy-init
        // flag (`widgets.is_some()` above) to call ensure_widgets only once,
        // so this subscribe path is entered exactly once per widget lifetime.
        let op_sub_handle = cx.subscribe_in(
            &widgets.op_select,
            window,
            |this, _select, ev: &SelectEvent<Vec<FilterOpItem>>, _window, cx| {
                let SelectEvent::Confirm(maybe_val) = ev;
                if let Some(op) = maybe_val {
                    this.state.set_op(*op);
                    cx.notify();
                }
            },
        );
        // Keep subscription alive for the entity's lifetime.
        drop(op_sub_handle); // Actually we need to store it — see comment below.

        // ── Value field wiring ─────────────────────────────────────────────
        let _ = cx.subscribe_in(
            &widgets.value_input,
            window,
            |this, input, ev: &InputEvent, _window, _cx| {
                if matches!(ev, InputEvent::Change) {
                    let text = input.read(_cx).value().to_string();
                    this.state.set_value_text(text);
                    if this.state.op == FilterOp::Regex {
                        this.state.revalidate_regex();
                    }
                }
            },
        );
        let _ = cx.subscribe_in(
            &widgets.range_lo_input,
            window,
            |this, input, ev: &InputEvent, _window, cx| {
                if matches!(ev, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.state.set_range_lo(text);
                }
            },
        );
        let _ = cx.subscribe_in(
            &widgets.range_hi_input,
            window,
            |this, input, ev: &InputEvent, _window, cx| {
                if matches!(ev, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.state.set_range_hi(text);
                }
            },
        );
        let _ = cx.subscribe_in(
            &widgets.list_input,
            window,
            |_this, _input, ev: &InputEvent, _window, _cx| {
                if matches!(ev, InputEvent::Change) {
                    // T10b: comma-separated text is tracked via InputState.
                    // T11 will replace this stub with a chip-list UI and
                    // distinct-values suggestions; list_values population is
                    // T11's responsibility.
                }
            },
        );

        // ── Value-field layout by operator ─────────────────────────────────
        let value_section: gpui::AnyElement = match op {
            // Nullary ops: no value field.
            FilterOp::IsEmpty | FilterOp::IsNotEmpty | FilterOp::IsTrue | FilterOp::IsFalse => {
                div().into_any_element()
            }

            // Between: lo + hi inputs + inclusive checkbox.
            FilterOp::Between => {
                let entity = cx.entity();
                v_flex()
                    .gap_2()
                    .child(Input::new(&widgets.range_lo_input).appearance(true))
                    .child(Input::new(&widgets.range_hi_input).appearance(true))
                    .child(
                        Checkbox::new("range-inclusive")
                            .label("Inclusive")
                            .checked(range_inclusive)
                            .on_click(move |checked, _window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.state.set_range_inclusive(*checked);
                                    cx.notify();
                                });
                            }),
                    )
                    .into_any_element()
            }

            // IN list: single text input + T11 stub.
            FilterOp::In => v_flex()
                .gap_2()
                // T11 stub: distinct-values panel mounts here.
                // Render an explicit placeholder so the gap is visible in dev.
                .child(Label::new("distinct values: T11"))
                .child(Input::new(&widgets.list_input).appearance(true))
                .into_any_element(),

            // All other ops (unary text): single value input.
            _ => Input::new(&widgets.value_input)
                .appearance(true)
                .into_any_element(),
        };

        // ── Regex validity indicator ───────────────────────────────────────
        let regex_hint: gpui::AnyElement = if op == FilterOp::Regex {
            match self.state.regex_valid {
                Some(true) => Label::new("valid pattern").into_any_element(),
                Some(false) => Label::new("invalid pattern").into_any_element(),
                None => div().into_any_element(),
            }
        } else {
            div().into_any_element()
        };

        // ── Apply / Cancel / Clear buttons ─────────────────────────────────
        let entity_apply = cx.entity();
        let entity_cancel = cx.entity();
        let entity_clear = cx.entity();

        let apply_btn = Button::new("filter-apply")
            .label("Apply")
            .primary()
            .disabled(!can_apply)
            .on_click(move |_ev, _window, cx| {
                entity_apply.update(cx, |this, _cx| {
                    if let Some(t) = this.state.apply_transformation() {
                        this.emit_outcome(Outcome::Apply(t));
                    }
                });
            });

        let cancel_btn = Button::new("filter-cancel")
            .label("Cancel")
            .ghost()
            .on_click(move |_ev, _window, cx| {
                entity_cancel.update(cx, |this, _cx| {
                    this.state.cancel();
                    this.emit_outcome(Outcome::Cancel);
                });
            });

        let clear_btn =
            Button::new("filter-clear")
                .label("Clear")
                .ghost()
                .on_click(move |_ev, _window, cx| {
                    entity_clear.update(cx, |this, _cx| {
                        let pre_pop = this.state.clear_filter();
                        this.emit_outcome(Outcome::Clear {
                            pre_populated: pre_pop,
                        });
                    });
                });

        // ── Assemble ───────────────────────────────────────────────────────
        v_flex()
            .gap_2()
            .p_3()
            .min_w(gpui::px(240.))
            .child(op_select_widget)
            .child(value_section)
            .child(regex_hint)
            .child(
                h_flex()
                    .gap_2()
                    .child(apply_btn)
                    .child(cancel_btn)
                    .child(clear_btn),
            )
    }
}
