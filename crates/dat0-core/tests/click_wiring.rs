//! P4b T0 (PD-016) — click-wiring integration test.
//!
//! GPUI has no headless click harness (per P4a), so this test exercises the
//! **ViewModel half** of the two new `WorkspaceShell` click handlers:
//! `on_sort_zone_click` and (the routing half of) `on_funnel_click`. It mirrors
//! the P4a T13 harness in `tests/view_lifecycle.rs`: a real `DuckDBEngine`, a
//! registered fixture table, and a `ViewModel`.
//!
//! What's covered here:
//!   - Sort-zone click logic: read current sort → cycle the clicked column →
//!     `set_sort` → assert the emitted `ViewChange` has `ORDER BY` + a view name.
//!   - Funnel popover `Outcome` routing via the **production** `route_outcome`:
//!     `Outcome::Apply(t)` → `vm.set_filter(t)` (column-aware upsert) and
//!     `Outcome::Clear { pre_populated: true }` → `vm.clear()` drive a ViewChange.
//!
//! The full GPUI-window click simulation (mouse down/up on the header zones,
//! popover mount/present) is covered by the manual UAT in T14.

use std::sync::Arc;
use tempfile::TempDir;

use dat0_core::view::filter_popover::Outcome;
use dat0_core::view::{ViewModel, route_outcome};
use dat0_engine::{
    DuckDBEngine, FilterOp, FilterValue, MemoryBudget, QueryEngine, RegisterOpts, Scalar,
    Transformation,
};

// ---------------------------------------------------------------------------
// Fixture: an `orders` table with an `amount` numeric column (mirrors the
// plan's `test_engine_with_orders()` helper name).
// ---------------------------------------------------------------------------

/// Spin up a DuckDB engine over a small `orders` CSV (`id` INTEGER,
/// `amount` INTEGER). Returns the engine (Arc) and the already-quoted base
/// table name suitable for `ViewModel::new`.
async fn test_engine_with_orders(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
    let csv = tmp.path().join("orders.csv");
    let mut s = String::from("id,amount\n");
    for i in 0..20_i64 {
        s.push_str(&format!("{},{}\n", i, (20 - i) * 5));
    }
    std::fs::write(&csv, s).unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let quoted = format!("\"{}\"", info.name.replace('"', "\"\""));
    (Arc::new(engine), quoted)
}

fn filter_amount_gte(n: i64) -> Transformation {
    Transformation::Filter {
        column: "amount".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Int(n),
        },
    }
}

// ---------------------------------------------------------------------------
// Sort-zone click → ViewChange with ORDER BY
// ---------------------------------------------------------------------------

/// Sort-zone click cycles the clicked column's sort and yields a `ViewChange`
/// whose SQL has `ORDER BY` and which names a new active view.
#[tokio::test]
async fn sort_zone_click_sets_sort_and_emits_view_change() {
    let tmp = TempDir::new().unwrap();
    let (_engine, base_table) = test_engine_with_orders(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base_table);

    // Simulate the logic `on_sort_zone_click` runs (no GPUI window needed for
    // the VM half): read current sort, plain-click the `amount` column, write
    // back via `set_sort`.
    let active = vm.current_sort_as_active().click("amount");
    let change = vm.set_sort(active.keys().to_vec());

    assert!(
        change.sql.as_deref().unwrap().contains("ORDER BY"),
        "sort-zone click must emit SQL with ORDER BY, got: {:?}",
        change.sql
    );
    assert!(
        change.new_active_view.is_some(),
        "sort-zone click must name a new active view"
    );
}

/// Plain-clicking the same column three times cycles none → asc → desc → none;
/// the third click clears the sort, so the resulting SQL has no `ORDER BY`.
#[tokio::test]
async fn sort_zone_click_cycles_back_to_unsorted() {
    let tmp = TempDir::new().unwrap();
    let (_engine, base_table) = test_engine_with_orders(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base_table);

    // none → asc
    let a = vm.current_sort_as_active().click("amount");
    let _ = vm.set_sort(a.keys().to_vec());
    // asc → desc
    let a = vm.current_sort_as_active().click("amount");
    let _ = vm.set_sort(a.keys().to_vec());
    // desc → none (cleared)
    let a = vm.current_sort_as_active().click("amount");
    let change = vm.set_sort(a.keys().to_vec());

    // Empty sort key list → compile_view_sql emits no ORDER BY clause.
    let sql = change.sql.as_deref().unwrap_or("");
    assert!(
        !sql.contains("ORDER BY"),
        "third plain click clears the sort; SQL must not contain ORDER BY, got: {sql}"
    );
}

// ---------------------------------------------------------------------------
// Funnel popover Outcome routing
// ---------------------------------------------------------------------------

// `route_outcome` (the production decision) is imported from `dat0_core::view`
// and exercised directly below — no local duplicate, so the test catches any
// divergence in the routing logic.

#[tokio::test]
async fn funnel_apply_outcome_drives_filter_view_change() {
    let tmp = TempDir::new().unwrap();
    let (_engine, base_table) = test_engine_with_orders(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base_table);

    let change = route_outcome(&mut vm, Outcome::Apply(filter_amount_gte(50)))
        .expect("Apply outcome must produce a ViewChange");
    assert!(
        change.new_active_view.is_some(),
        "applying a filter must name a new active view"
    );
    let sql = change.sql.as_deref().unwrap();
    assert!(
        sql.contains("WHERE") && sql.contains("amount"),
        "filter SQL must constrain `amount`, got: {sql}"
    );
}

#[tokio::test]
async fn funnel_clear_prepopulated_rebinds_to_base() {
    let tmp = TempDir::new().unwrap();
    let (_engine, base_table) = test_engine_with_orders(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base_table);

    // Apply a filter so there is something to clear.
    let _ = vm.apply(filter_amount_gte(50));
    let cleared = route_outcome(
        &mut vm,
        Outcome::Clear {
            pre_populated: true,
        },
    )
    .expect("Clear{pre_populated:true} must produce a ViewChange");
    assert!(
        cleared.new_active_view.is_none(),
        "clear rebinds to the base table (no active view)"
    );
}

#[tokio::test]
async fn funnel_cancel_and_unpopulated_clear_are_noops() {
    let tmp = TempDir::new().unwrap();
    let (_engine, base_table) = test_engine_with_orders(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base_table);

    assert!(route_outcome(&mut vm, Outcome::Cancel).is_none());
    assert!(
        route_outcome(
            &mut vm,
            Outcome::Clear {
                pre_populated: false
            }
        )
        .is_none()
    );
}

// ---------------------------------------------------------------------------
// Edit-apply replaces (does not stack) a filter on a column that already has
// one — the bug PD-016/T0 review caught. Routing goes through the production
// `route_outcome` → `vm.set_filter`, so the resulting SQL must constrain the
// edited column with a SINGLE predicate, not an `AND` of the old + new value.
// ---------------------------------------------------------------------------

/// Re-editing the funnel on a column with an existing filter REPLACES the
/// predicate. After applying `amount >= 50` then `amount >= 80`, the view SQL
/// must contain `80` but not `50` (no stacked `AND` of both bounds), and the
/// stack must hold a single op (one undo step).
#[tokio::test]
async fn funnel_edit_apply_replaces_existing_filter_on_same_column() {
    let tmp = TempDir::new().unwrap();
    let (_engine, base_table) = test_engine_with_orders(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base_table);

    // First filter on `amount`.
    let _ = route_outcome(&mut vm, Outcome::Apply(filter_amount_gte(50)))
        .expect("first Apply must produce a ViewChange");
    assert_eq!(vm.stack().len(), 1, "first filter appends one op");

    // Re-edit the same column: should REPLACE, not stack.
    let change = route_outcome(&mut vm, Outcome::Apply(filter_amount_gte(80)))
        .expect("edit Apply must produce a ViewChange");

    assert_eq!(
        vm.stack().len(),
        1,
        "re-editing the same column must replace in place (single undo step), not stack a second predicate"
    );

    let sql = change.sql.as_deref().unwrap();
    assert!(
        sql.contains("80"),
        "replaced predicate must use the new bound (80), got: {sql}"
    );
    assert!(
        !sql.contains("50"),
        "old predicate (50) must be gone — must NOT be `AND`-stacked, got: {sql}"
    );
}

/// Applying a filter on a *different* column still APPENDS (the upsert keys on
/// column). Two distinct columns → two ops → both predicates present.
#[tokio::test]
async fn funnel_apply_new_column_still_appends() {
    let tmp = TempDir::new().unwrap();
    let (_engine, base_table) = test_engine_with_orders(&tmp).await;
    let mut vm = ViewModel::new("tab1".into(), base_table);

    let _ = route_outcome(&mut vm, Outcome::Apply(filter_amount_gte(50)))
        .expect("first Apply must produce a ViewChange");

    // Filter on a different column (`id`).
    let id_filter = Transformation::Filter {
        column: "id".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Int(3),
        },
    };
    let change = route_outcome(&mut vm, Outcome::Apply(id_filter))
        .expect("Apply on a new column must produce a ViewChange");

    assert_eq!(
        vm.stack().len(),
        2,
        "a filter on a new column must append, not replace"
    );
    let sql = change.sql.as_deref().unwrap();
    assert!(
        sql.contains("amount") && sql.contains("id"),
        "both column predicates must be present, got: {sql}"
    );
}
