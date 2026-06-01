//! End-to-end ViewModel ↔ engine integration.
//!
//! Each test drives ViewModel (T5) through apply / undo / redo / replace_at_cursor,
//! translates the emitted ViewChange into real DuckDB operations (T4), then verifies
//! row counts by paging the active view.

use std::sync::Arc;
use tempfile::TempDir;

use dat0_app::view::ViewModel;
use dat0_engine::{
    DuckDBEngine, FilterOp, FilterValue, MemoryBudget, QueryEngine, RegisterOpts, Scalar,
    SortDirection, SortKey, Transformation,
};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Spin up a DuckDB engine with a small 100-row CSV (columns: `a` INTEGER, `b` TEXT).
/// Returns the engine (Arc) and the registered table name.
async fn engine_with_100_rows(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
    let csv = tmp.path().join("t.csv");
    let mut s = String::from("a,b\n");
    for i in 0..100_i64 {
        s.push_str(&format!("{},x{}\n", i, i));
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
    let name = info.name.clone();
    (Arc::new(engine), name)
}

/// Convenience: build `a >= n` filter transformation (struct-variant form per PD-015).
fn filter_a_gte(n: i64) -> Transformation {
    Transformation::Filter {
        column: "a".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Int(n),
        },
    }
}

/// Apply a ViewChange to the engine: create the new view if needed, drop the
/// previous view if one is named. This is the caller-side protocol T6 validates.
async fn apply_change(engine: &DuckDBEngine, change: &dat0_app::view::ViewChange) {
    if let (Some(name), Some(sql)) = (&change.new_active_view, &change.sql) {
        engine.create_or_replace_view(name, sql).await.unwrap();
    }
    if let Some(prev) = &change.previous_active_view {
        engine.drop_view(prev).await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// T6-1: apply → view created → paged rows match filter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn apply_creates_view_and_pages_match_filter() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_100_rows(&tmp).await;

    // base_table passed to ViewModel must already be quoted (design §4).
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = ViewModel::new("tab1".into(), base);

    let change = vm.apply(filter_a_gte(50));

    let view_name = change
        .new_active_view
        .as_ref()
        .expect("apply must emit a view name");
    let sql = change.sql.as_ref().expect("apply must emit SQL");
    engine.create_or_replace_view(view_name, sql).await.unwrap();

    let paged = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", view_name.replace('"', "\"\"")),
            0,
            200,
        )
        .await
        .unwrap();

    // a in [50..99] = 50 rows
    assert_eq!(paged.total_rows, 50, "a >= 50 should match 50 of 100 rows");
}

// ---------------------------------------------------------------------------
// T6-2: undo → previous view dropped → querying the dropped name errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn undo_returns_change_to_drop_previous_view() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_100_rows(&tmp).await;
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = ViewModel::new("tab1".into(), base);

    // Apply a filter and materialise the view.
    let c1 = vm.apply(filter_a_gte(50));
    let view1 = c1.new_active_view.clone().unwrap();
    engine
        .create_or_replace_view(&view1, c1.sql.as_ref().unwrap())
        .await
        .unwrap();

    // Undo to empty stack.
    let c_undo = vm.undo().expect("undo must return Some after one apply");
    assert!(
        c_undo.new_active_view.is_none(),
        "undo to empty rebinds to base table"
    );
    assert!(c_undo.sql.is_none(), "base-table rebind needs no SQL");
    assert_eq!(
        c_undo.previous_active_view.as_deref(),
        Some(view1.as_str()),
        "previous_active_view must name the view that was active before undo"
    );

    // Drop the named view from the engine.
    engine
        .drop_view(c_undo.previous_active_view.as_ref().unwrap())
        .await
        .unwrap();

    // Confirm the view is gone: querying it must fail.
    let res = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", view1.replace('"', "\"\"")),
            0,
            1,
        )
        .await;
    assert!(res.is_err(), "dropped view must not be queryable");
}

// ---------------------------------------------------------------------------
// T6-3: redo → recreates view with a fresh nonce → correct row count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn redo_recreates_view_with_fresh_nonce() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_100_rows(&tmp).await;
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = ViewModel::new("tab1".into(), base);

    // Apply then undo.
    let c_apply = vm.apply(filter_a_gte(50));
    let original_name = c_apply.new_active_view.clone().unwrap();
    engine
        .create_or_replace_view(&original_name, c_apply.sql.as_ref().unwrap())
        .await
        .unwrap();

    let c_undo = vm.undo().unwrap();
    if let Some(prev) = &c_undo.previous_active_view {
        engine.drop_view(prev).await.unwrap();
    }

    // Redo — per design §5 the nonce bumps on every regenerate_view call,
    // so the redo name must differ from the original apply name.
    let c_redo = vm.redo().expect("redo must return Some after undo");
    let redo_name = c_redo.new_active_view.as_ref().unwrap();
    assert_ne!(
        redo_name, &original_name,
        "nonce must increment on every regenerate_view so redo yields a fresh name"
    );

    engine
        .create_or_replace_view(redo_name, c_redo.sql.as_ref().unwrap())
        .await
        .unwrap();

    let paged = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", redo_name.replace('"', "\"\"")),
            0,
            200,
        )
        .await
        .unwrap();
    assert_eq!(
        paged.total_rows, 50,
        "redo must restore the same filter semantics"
    );
}

// ---------------------------------------------------------------------------
// T6-4: replace_at_cursor → same history depth, new view with updated filter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replace_at_cursor_does_not_change_history_size() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_100_rows(&tmp).await;
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = ViewModel::new("tab1".into(), base);

    // Push one filter.
    let c1 = vm.apply(filter_a_gte(50));
    engine
        .create_or_replace_view(
            c1.new_active_view.as_ref().unwrap(),
            c1.sql.as_ref().unwrap(),
        )
        .await
        .unwrap();
    let stack_len = vm.stack().len();

    // Replace the cursor op — must not grow the stack.
    let c2 = vm.replace_at_cursor(filter_a_gte(90));
    assert_eq!(
        vm.stack().len(),
        stack_len,
        "replace_at_cursor must not grow history"
    );

    apply_change(&engine, &c2).await;

    let view2 = c2.new_active_view.as_ref().unwrap();
    let paged = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", view2.replace('"', "\"\"")),
            0,
            200,
        )
        .await
        .unwrap();
    // a in [90..99] = 10 rows
    assert_eq!(paged.total_rows, 10, "a >= 90 should match 10 of 100 rows");
}

// ---------------------------------------------------------------------------
// T6-5: clear → rebind to base → undo restores full stack in one step
//
// P4c zipper: clear() is an undoable structural edit (checkpoints the pre-clear
// present onto `past`, clears the redo future). Restoring the cleared stack is
// therefore done via undo(), not redo().
// ---------------------------------------------------------------------------

#[tokio::test]
async fn clear_and_redo_restores_full_stack() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_100_rows(&tmp).await;
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = ViewModel::new("tab1".into(), base.clone());

    // Build a two-op stack: filter then sort.
    let c1 = vm.apply(filter_a_gte(50));
    apply_change(&engine, &c1).await;

    let c2 = vm.set_sort(vec![SortKey {
        column: "a".into(),
        direction: SortDirection::Desc,
    }]);
    apply_change(&engine, &c2).await;
    assert_eq!(vm.stack().len(), 2);

    // Clear: cursor drops to 0, both views should be dropped.
    let c_clear = vm.clear();
    assert_eq!(vm.cursor(), 0);
    assert!(
        c_clear.new_active_view.is_none(),
        "clear rebinds to base table"
    );
    if let Some(prev) = &c_clear.previous_active_view {
        engine.drop_view(prev).await.unwrap();
    }

    // Querying the base table directly must still return 100 rows.
    let paged_base = engine
        .execute_paged(&format!("SELECT * FROM {}", base), 0, 200)
        .await
        .unwrap();
    assert_eq!(
        paged_base.total_rows, 100,
        "base table untouched after clear"
    );

    // One undo restores the full pre-clear stack in a single step (design §5).
    let c_undo = vm.undo().expect("undo must be available after clear");
    assert_eq!(
        vm.cursor(),
        2,
        "undo of clear should restore the full pre-clear stack"
    );
    apply_change(&engine, &c_undo).await;

    let undo_view = c_undo.new_active_view.as_ref().unwrap();
    let paged_undo = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", undo_view.replace('"', "\"\"")),
            0,
            200,
        )
        .await
        .unwrap();
    assert_eq!(
        paged_undo.total_rows, 50,
        "restored view must honour the filter"
    );
}

// ---------------------------------------------------------------------------
// T6-6: set_sort upserts → same view semantics via engine
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_sort_upserts_and_pages_are_ordered() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_100_rows(&tmp).await;
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = ViewModel::new("tab1".into(), base);

    // Apply a filter first, then add a sort.
    let c1 = vm.apply(filter_a_gte(50));
    apply_change(&engine, &c1).await;
    assert_eq!(vm.stack().len(), 1);

    let c_sort = vm.set_sort(vec![SortKey {
        column: "a".into(),
        direction: SortDirection::Desc,
    }]);
    apply_change(&engine, &c_sort).await;
    assert_eq!(vm.stack().len(), 2, "set_sort appends when no Sort exists");

    // Upsert (replace existing Sort in place) — stack must not grow.
    let c_sort2 = vm.set_sort(vec![SortKey {
        column: "a".into(),
        direction: SortDirection::Asc,
    }]);
    apply_change(&engine, &c_sort2).await;
    assert_eq!(vm.stack().len(), 2, "set_sort upsert must not grow stack");

    let view_name = c_sort2.new_active_view.as_ref().unwrap();
    let paged = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", view_name.replace('"', "\"\"")),
            0,
            200,
        )
        .await
        .unwrap();
    // Filter a >= 50 still applies → 50 rows.
    assert_eq!(
        paged.total_rows, 50,
        "sort upsert must not alter the filter row count"
    );
}

// ---------------------------------------------------------------------------
// T13: supersede-cancel — engine.interrupt() doesn't leave engine broken
// ---------------------------------------------------------------------------

/// Verify that `engine.interrupt()` signals a cooperative cancel on an
/// in-flight view creation, and that the engine remains usable afterward
/// (either completing normally or returning an error, but not poisoning
/// the connection).
///
/// Timing note: this test is intentionally lenient — we do NOT assert that
/// the first task was actually interrupted (race between interrupt and
/// completion). What we DO assert is that:
/// 1. After the interrupt + wait, the engine is still functional.
/// 2. The second `ViewChange` (c2) creates a correct view.
/// 3. The final paged query against c2's view returns the expected rows.
///
/// This is the "supersede semantics" contract: the newer change wins, and
/// the engine is never left in a bad state regardless of whether the
/// interrupt arrived early or late.
///
/// Marked `#[ignore]` because it is timing-sensitive on heavily-loaded CI;
/// run via `cargo test -- --ignored supersede_cancels_in_flight_view_regen`
/// or the `run-heavy` workflow.
#[tokio::test]
#[ignore = "timing-sensitive; run with --ignored on dedicated CI"]
async fn supersede_cancels_in_flight_view_regen() {
    use std::time::Duration;

    let tmp = tempfile::TempDir::new().unwrap();
    let (engine, table) = engine_with_100_rows(&tmp).await;
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = dat0_app::view::ViewModel::new("tab1".into(), base);

    // First change: a >= 50.
    let c1 = vm.apply(filter_a_gte(50));
    let c1_name = c1.new_active_view.clone().unwrap();
    let c1_sql = c1.sql.clone().unwrap();

    // Spawn the first view creation on a separate task to simulate an
    // in-flight engine round-trip.
    let engine_c1 = Arc::clone(&engine);
    let task =
        tokio::spawn(async move { engine_c1.create_or_replace_view(&c1_name, &c1_sql).await });

    // Almost immediately, signal interrupt (supersede).
    tokio::time::sleep(Duration::from_millis(5)).await;
    engine.interrupt(); // cooperative cancel

    // Wait for the first task — it either completed normally or returned
    // an Interrupted error. Either is acceptable; the engine must be healthy.
    let _ = task.await;

    // Second change: a >= 90. This must succeed regardless of whether the
    // first task was actually interrupted.
    let c2 = vm.apply(filter_a_gte(90));
    let c2_name = c2.new_active_view.as_ref().unwrap();
    let c2_sql = c2.sql.as_ref().unwrap();

    engine
        .create_or_replace_view(c2_name, c2_sql)
        .await
        .expect("second view creation must succeed after interrupt");

    // Verify c2's view returns the expected row count.
    let paged = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", c2_name.replace('"', "\"\"")),
            0,
            100,
        )
        .await
        .expect("paged query must succeed");

    // a in [90..99] = 10 rows
    assert_eq!(
        paged.total_rows, 10,
        "second view must return rows for a >= 90 (10 of 100)"
    );
}
