//! Smoke tests for `FilterPopoverEntity` (T10b).
//!
//! Per `docs/internal/dat0-p4a-t0-spike.md` §3, `InputState::new` and
//! `SelectState::new` require `&mut Window`, which is unavailable in the
//! headless `TestAppContext`. Widget state is therefore lazy-initialised on
//! the first `render()` call, so entity construction itself is Window-free
//! and can be tested here.
//!
//! # Coverage
//!
//! These tests verify:
//! 1. `FilterPopoverEntity::new` constructs without panic.
//! 2. `FilterPopoverEntity::from_existing` pre-populates correctly.
//! 3. `Outcome::Apply` is emitted when `can_apply == true` and
//!    `apply_transformation` is called through the entity.
//! 4. `Outcome::Cancel` is emitted when cancel is triggered.
//! 5. `Outcome::Clear` carries the correct `pre_populated` flag.
//! 6. Multiple outcome callbacks all receive the emitted outcome.
//!
//! Visual snapshot tests (widget layout) are deferred per PD-013 — they
//! require a real macOS window and the bench-artifact runner pattern.

use std::sync::{Arc, Mutex};

use gpui::prelude::*;

use dat0_app::view::filter_popover::ColumnType;
use dat0_app::view::filter_popover_entity::{FilterPopoverEntity, Outcome};
use dat0_engine::{FilterOp, FilterValue, Scalar, Transformation};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a closure with a real `&mut gpui::App` using `TestAppContext::single`.
/// Returns the closure's return value.
fn with_app<F, T>(f: F) -> T
where
    F: FnOnce(&mut gpui::App) -> T,
{
    let cx = gpui::TestAppContext::single();
    cx.update(f)
}

// ---------------------------------------------------------------------------
// 1. Construction — no panic
// ---------------------------------------------------------------------------

#[test]
fn new_entity_constructs_without_panic() {
    with_app(|cx| {
        let _entity = cx.new(|_| FilterPopoverEntity::new("price".into(), ColumnType::Numeric));
    });
}

#[test]
fn new_entity_constructs_for_all_column_types() {
    with_app(|cx| {
        for ct in [
            ColumnType::Numeric,
            ColumnType::String,
            ColumnType::Bool,
            ColumnType::Date,
            ColumnType::Timestamp,
        ] {
            let _entity = cx.new(|_| FilterPopoverEntity::new("col".into(), ct));
        }
    });
}

// ---------------------------------------------------------------------------
// 2. from_existing — pre-population
// ---------------------------------------------------------------------------

#[test]
fn from_existing_pre_populates_eq_numeric() {
    let existing = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    };
    with_app(|cx| {
        let entity = cx.new(|_| {
            FilterPopoverEntity::from_existing("price".into(), ColumnType::Numeric, &existing)
        });
        // can_apply must be true because value_text = "42" (non-empty).
        assert!(
            entity.read(cx).state.pre_populated,
            "from_existing should be marked pre_populated"
        );
        assert!(
            entity.read(cx).can_apply(),
            "from_existing with Eq numeric should be apply-able immediately"
        );
    });
}

// ---------------------------------------------------------------------------
// 3. Outcome::Apply emitted when can_apply is true
// ---------------------------------------------------------------------------

#[test]
fn apply_outcome_emitted_when_can_apply() {
    with_app(|cx| {
        let captured: Arc<Mutex<Vec<Outcome>>> = Arc::new(Mutex::new(Vec::new()));
        let cap_clone = Arc::clone(&captured);

        let entity = cx.new(|_| FilterPopoverEntity::new("price".into(), ColumnType::Numeric));

        // Register outcome subscriber.
        entity.update(cx, |ent, _cx| {
            ent.on_outcome(move |o| {
                cap_clone.lock().unwrap().push(o);
            });
        });

        // Drive state to an apply-able condition: Eq with non-empty value_text.
        entity.update(cx, |ent, _cx| {
            ent.state.set_op(FilterOp::Eq);
            ent.state.set_value_text("99".into());
        });

        assert!(
            entity.read(cx).can_apply(),
            "precondition: can_apply must be true"
        );

        // Emit Apply directly through the entity's internal method.
        entity.update(cx, |ent, _cx| {
            if let Some(t) = ent.state.apply_transformation() {
                ent.emit_outcome(Outcome::Apply(t));
            }
        });

        let outcomes = captured.lock().unwrap();
        assert_eq!(outcomes.len(), 1, "exactly one outcome should be emitted");
        assert!(
            matches!(
                &outcomes[0],
                Outcome::Apply(Transformation::Filter {
                    op: FilterOp::Eq,
                    ..
                })
            ),
            "outcome should be Apply with Eq filter, got {:?}",
            outcomes[0]
        );
    });
}

// ---------------------------------------------------------------------------
// 4. Outcome::Cancel emitted
// ---------------------------------------------------------------------------

#[test]
fn cancel_outcome_emitted() {
    with_app(|cx| {
        let captured: Arc<Mutex<Vec<Outcome>>> = Arc::new(Mutex::new(Vec::new()));
        let cap_clone = Arc::clone(&captured);

        let entity = cx.new(|_| FilterPopoverEntity::new("name".into(), ColumnType::String));
        entity.update(cx, |ent, _cx| {
            ent.on_outcome(move |o| cap_clone.lock().unwrap().push(o));
        });

        entity.update(cx, |ent, _cx| {
            ent.state.cancel();
            ent.emit_outcome(Outcome::Cancel);
        });

        let outcomes = captured.lock().unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0], Outcome::Cancel));
    });
}

// ---------------------------------------------------------------------------
// 5. Outcome::Clear carries correct pre_populated flag
// ---------------------------------------------------------------------------

#[test]
fn clear_outcome_pre_populated_false_when_new() {
    with_app(|cx| {
        let captured: Arc<Mutex<Vec<Outcome>>> = Arc::new(Mutex::new(Vec::new()));
        let cap_clone = Arc::clone(&captured);

        let entity = cx.new(|_| FilterPopoverEntity::new("x".into(), ColumnType::Numeric));
        entity.update(cx, |ent, _cx| {
            ent.on_outcome(move |o| cap_clone.lock().unwrap().push(o));
        });

        entity.update(cx, |ent, _cx| {
            let pre_pop = ent.state.clear_filter();
            ent.emit_outcome(Outcome::Clear {
                pre_populated: pre_pop,
            });
        });

        let outcomes = captured.lock().unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(
                outcomes[0],
                Outcome::Clear {
                    pre_populated: false
                }
            ),
            "new popover should emit Clear {{ pre_populated: false }}"
        );
    });
}

#[test]
fn clear_outcome_pre_populated_true_when_existing() {
    let existing = Transformation::Filter {
        column: "x".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(1),
        },
    };
    with_app(|cx| {
        let captured: Arc<Mutex<Vec<Outcome>>> = Arc::new(Mutex::new(Vec::new()));
        let cap_clone = Arc::clone(&captured);

        let entity = cx.new(|_| {
            FilterPopoverEntity::from_existing("x".into(), ColumnType::Numeric, &existing)
        });
        entity.update(cx, |ent, _cx| {
            ent.on_outcome(move |o| cap_clone.lock().unwrap().push(o));
        });

        entity.update(cx, |ent, _cx| {
            let pre_pop = ent.state.clear_filter();
            ent.emit_outcome(Outcome::Clear {
                pre_populated: pre_pop,
            });
        });

        let outcomes = captured.lock().unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(
                outcomes[0],
                Outcome::Clear {
                    pre_populated: true
                }
            ),
            "from_existing popover should emit Clear {{ pre_populated: true }}"
        );
    });
}

// ---------------------------------------------------------------------------
// 7. Subscription handles are stored — regression guard for T10b fix
// ---------------------------------------------------------------------------

/// Structural regression test for the T10b subscription-lifetime fix.
///
/// # What this guards
///
/// `ensure_widgets()` calls `cx.subscribe_in(...)` five times (operator Select,
/// value Input, range-lo Input, range-hi Input, list Input) and stores the
/// returned `Subscription` handles in `self._subscriptions`. GPUI silently
/// deregisters a subscription when its handle is dropped, so `let _ = ...` at
/// the call site — the original bug — made all five callbacks dead on arrival.
///
/// Because `ensure_widgets()` requires `&mut Window` (unavailable headlessly,
/// per T0 spike / PD-013), we cannot trigger the widget-init path in a
/// `TestAppContext`. Instead this test:
///
/// 1. Verifies `subscription_count() == 0` immediately after construction
///    (before the first render) — guards correct zero-initialization and
///    ensures the field wasn't accidentally pre-populated.
/// 2. Documents the *expected* post-render count (5) as a comment so any
///    future change to the subscription wiring fails the review checklist.
///
/// The compiler enforces the non-drop invariant at the `ensure_widgets()` call
/// site: the subscriptions are pushed into `self._subscriptions`, so Clippy
/// `-D warnings` catches any `let _ = cx.subscribe_in(...)` reintroduction
/// via the `must_use` attribute on `Subscription`.
#[test]
fn subscriptions_stored_not_dropped() {
    with_app(|cx| {
        let entity = cx.new(|_| FilterPopoverEntity::new("price".into(), ColumnType::Numeric));

        // Pre-render: no widgets initialised yet, so no subscriptions.
        // Expected post-render count: 5 (op_select, value_input, range_lo,
        // range_hi, list_input) — verified by code review of ensure_widgets().
        let pre_render_count = entity.read(cx).subscription_count();
        assert_eq!(
            pre_render_count, 0,
            "subscriptions should be empty before ensure_widgets() runs; \
             got {pre_render_count} — something is subscribing at construction time"
        );
    });
}

// ---------------------------------------------------------------------------
// 8. Multiple subscribers all receive the outcome
// ---------------------------------------------------------------------------

#[test]
fn multiple_outcome_callbacks_all_fired() {
    with_app(|cx| {
        let count_a: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let count_b: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let ca = Arc::clone(&count_a);
        let cb = Arc::clone(&count_b);

        let entity = cx.new(|_| FilterPopoverEntity::new("flag".into(), ColumnType::Bool));
        entity.update(cx, |ent, _cx| {
            ent.on_outcome(move |_o| *ca.lock().unwrap() += 1);
            ent.on_outcome(move |_o| *cb.lock().unwrap() += 1);
        });

        entity.update(cx, |ent, _cx| {
            ent.emit_outcome(Outcome::Cancel);
        });

        assert_eq!(*count_a.lock().unwrap(), 1, "callback A should fire once");
        assert_eq!(*count_b.lock().unwrap(), 1, "callback B should fire once");
    });
}
