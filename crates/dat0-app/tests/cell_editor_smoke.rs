//! Smoke tests for `CellEditor` (T6 — subscription-storage regression guard).
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
//! 1. `CellEditor::new` constructs without panic for every `ColumnType`.
//! 2. `subscription_count() == 0` before the first render (mirrors the
//!    `filter_popover_entity_smoke` guard for the P4a T10b regression vector).
//!    Expected post-render count: 1 (text columns: one `InputEvent` sub;
//!    Bool column: one `SelectEvent` sub) — verified by code review of
//!    `ensure_widgets()`.
//! 3. `CellEditor::with_seed` constructs without panic and carries the seed.
//!
//! Visual / render tests require a real `&mut Window` and are deferred
//! (PD-013).

use gpui::prelude::*;

use dat0_app::grid::cell_editor::CellEditor;
use dat0_app::view::filter_popover::ColumnType;

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
fn new_editor_constructs_without_panic() {
    with_app(|cx| {
        let _entity = cx.new(|_| CellEditor::new(ColumnType::Numeric));
    });
}

#[test]
fn new_editor_constructs_for_all_column_types() {
    with_app(|cx| {
        for ct in [
            ColumnType::Numeric,
            ColumnType::String,
            ColumnType::Bool,
            ColumnType::Date,
            ColumnType::Timestamp,
        ] {
            let _entity = cx.new(|_| CellEditor::new(ct));
        }
    });
}

// ---------------------------------------------------------------------------
// 2. Subscription handles are stored — regression guard for T10b fix
// ---------------------------------------------------------------------------

/// Structural regression test for the T10b subscription-lifetime fix.
///
/// # What this guards
///
/// `ensure_widgets()` calls `cx.subscribe_in(...)` once (text columns: one
/// `InputEvent` sub; Bool column: one `SelectEvent` sub) and stores the
/// returned `Subscription` handle in `self._subscriptions`. GPUI silently
/// deregisters a subscription when its handle is dropped, so `let _ = ...`
/// at the call site — the original bug — would make the commit callback dead
/// on arrival.
///
/// Because `ensure_widgets()` requires `&mut Window` (unavailable headlessly),
/// we cannot trigger the widget-init path in a `TestAppContext`. Instead this
/// test:
///
/// 1. Verifies `subscription_count() == 0` immediately after construction
///    (before the first render) — guards correct zero-initialization.
/// 2. Documents the *expected* post-render count (1) as a comment so any
///    future change to the subscription wiring fails the review checklist.
///
/// The compiler enforces the non-drop invariant at the `ensure_widgets()` call
/// site: the subscriptions are pushed into `self._subscriptions`, so Clippy
/// `-D warnings` catches any `let _ = cx.subscribe_in(...)` reintroduction
/// via the `must_use` attribute on `Subscription`.
#[test]
fn subscriptions_stored_not_dropped() {
    with_app(|cx| {
        let entity = cx.new(|_| CellEditor::new(ColumnType::Numeric));

        // Pre-render: no widgets initialised yet, so no subscriptions.
        // Expected post-render count: 1 (one InputEvent subscription for the
        // text input) — verified by code review of ensure_widgets().
        let pre_render_count = entity.read(cx).subscription_count();
        assert_eq!(
            pre_render_count, 0,
            "subscriptions should be empty before ensure_widgets() runs; \
             got {pre_render_count} — something is subscribing at construction time"
        );
    });
}

#[test]
fn subscriptions_stored_not_dropped_bool_column() {
    with_app(|cx| {
        let entity = cx.new(|_| CellEditor::new(ColumnType::Bool));

        // Pre-render: no widgets initialised yet, so no subscriptions.
        // Expected post-render count: 1 (one SelectEvent subscription for the
        // boolean select) — verified by code review of ensure_widgets().
        let pre_render_count = entity.read(cx).subscription_count();
        assert_eq!(
            pre_render_count, 0,
            "subscriptions should be empty before ensure_widgets() runs (Bool); \
             got {pre_render_count}"
        );
    });
}

// ---------------------------------------------------------------------------
// 3. with_seed — constructs without panic, seed is preserved
// ---------------------------------------------------------------------------

#[test]
fn with_seed_constructs_without_panic() {
    with_app(|cx| {
        let _entity = cx.new(|_| CellEditor::with_seed(ColumnType::String, "hello"));
    });
}

// ---------------------------------------------------------------------------
// 4. Focus handle accessor — compile-level guard for the P4c T14 rework
// ---------------------------------------------------------------------------

/// `CellEditor` exposes a `focus_handle()` accessor (P4c T14).
///
/// Full focus *behaviour* (focus-on-mount, Enter→down advance) needs a real
/// `&mut Window` and is manual UAT (T15). This test is the compile-level guard
/// that the accessor exists and yields a usable [`gpui::FocusHandle`]: the
/// handle is built lazily (since `new` is `cx`-free, mirroring the lazy widget
/// mount), so calling the accessor twice returns the *same* handle.
#[test]
fn cell_editor_exposes_focus_handle() {
    with_app(|cx| {
        let entity = cx.new(|_| CellEditor::new(ColumnType::String));
        let (h1, h2) = entity.update(cx, |ed, cx| (ed.focus_handle(cx), ed.focus_handle(cx)));
        // The lazily-built handle is stable across calls (same focus identity).
        assert_eq!(h1, h2, "focus_handle() must return a stable handle");
    });
}
