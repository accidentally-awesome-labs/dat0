//! End-to-end: ViewModel + session.json v2 + engine view lifecycle.
//!
//! Apply → undo → redo → clear → simulate crash → reload from disk → restore.
//!
//! Path B (PD-016): drives the full P4a lifecycle via direct ViewModel API +
//! engine round-trips; no UI click-handler wiring required.

use std::sync::Arc;

use tempfile::TempDir;

use dat0_app::session::{SESSION_SCHEMA_VERSION, SessionState, Tab, migrate};
use dat0_app::view::ViewModel;
use dat0_engine::{
    DuckDBEngine, FilterOp, FilterValue, MemoryBudget, QueryEngine, RegisterOpts, Scalar,
    Transformation,
};

// ---------------------------------------------------------------------------
// Fixture helpers (shared by both tests below)
// ---------------------------------------------------------------------------

/// Spin up a DuckDB engine backed by a 10-row CSV (columns: `a` INTEGER, `b` TEXT).
/// Returns the engine (Arc-wrapped) and the registered table name.
async fn engine_with_table(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
    let csv = tmp.path().join("t.csv");
    let mut s = String::from("a,b\n");
    for i in 0..10_i64 {
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

/// Build a `Filter { a == v }` transformation (struct-variant per PD-015).
fn filter_eq(col: &str, v: i64) -> Transformation {
    Transformation::Filter {
        column: col.into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(v),
        },
    }
}

/// Apply a ViewChange to the engine: create the new view if SQL is present,
/// then drop the old view if one was previously active.
async fn apply_change(engine: &DuckDBEngine, change: &dat0_app::view::ViewChange) {
    if let (Some(name), Some(sql)) = (&change.new_active_view, &change.sql) {
        engine.create_or_replace_view(name, sql).await.unwrap();
    }
    if let Some(prev) = &change.previous_active_view {
        let _ = engine.drop_view(prev).await; // best-effort
    }
}

// ---------------------------------------------------------------------------
// Test 1: session.json v2 round-trip preserves transform_stack + undo_cursor
// ---------------------------------------------------------------------------

/// Verify that writing a ViewModel's active stack + cursor to session.json v2
/// and reloading it via `migrate::load` preserves the exact active-stack
/// contents and cursor position.
///
/// P4c: session persists the ACTIVE stack only; cross-restart redo intentionally
/// narrows (design §4.1). Under the zipper, `stack()` == the active `present`,
/// so after apply×3 + undo the persisted stack is the 2 active ops (the undone
/// op lives in the in-memory `future`, which is not persisted).
#[tokio::test]
async fn session_round_trip_preserves_stack() {
    let tmp = TempDir::new().unwrap();
    let (_, table) = engine_with_table(&tmp).await;

    // Build a 3-op stack then undo once → present is the active 2 ops.
    let base = format!("\"{}\"", table.replace('"', "\"\""));
    let mut vm = ViewModel::new("tab1".into(), base);
    vm.apply(filter_eq("a", 1));
    vm.apply(filter_eq("a", 2));
    vm.apply(filter_eq("a", 3));
    vm.undo(); // present drops from 3 → 2 active ops; the undone op moves to `future`

    assert_eq!(vm.stack().len(), 2);
    assert_eq!(vm.cursor(), 2);

    // Persist to session.json.
    let session_json = tmp.path().join("session.json");
    let state = SessionState {
        schema_version: SESSION_SCHEMA_VERSION,
        tabs: vec![Tab {
            table_name: table.clone(),
            source_path: None,
            transform_stack: vm.stack().to_vec(),
            undo_cursor: vm.cursor(),
            extra: serde_json::Map::new(),
        }],
        active_tab: Some(0),
        sql_tabs: Vec::new(),
        active_sql_tab: None,
        query_history: Vec::new(),
        saved_queries: Vec::new(),
    };
    std::fs::write(&session_json, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    // Simulate crash: drop vm.
    drop(vm);

    // Reload via migrate::load.
    let restored = migrate::load(&session_json).unwrap();

    assert_eq!(
        restored.schema_version, SESSION_SCHEMA_VERSION,
        "reloaded schema_version must be the current schema"
    );
    assert_eq!(restored.tabs.len(), 1);
    // P4c: session persists the ACTIVE stack only; cross-restart redo
    // intentionally narrows (design §4.1). The undone op (a=3) lived in the
    // in-memory `future` and is not part of the persisted active stack.
    assert_eq!(
        restored.tabs[0].transform_stack.len(),
        2,
        "only the active stack (present) is persisted; redo future is not"
    );
    assert_eq!(
        restored.tabs[0].undo_cursor, 2,
        "cursor at time of persist must be preserved"
    );

    // Verify the two entries match the active transforms.
    assert_eq!(restored.tabs[0].transform_stack[0], filter_eq("a", 1));
    assert_eq!(restored.tabs[0].transform_stack[1], filter_eq("a", 2));
}

// ---------------------------------------------------------------------------
// Test 2: full loop — apply → undo → redo → clear → crash → reload → restore
// ---------------------------------------------------------------------------

/// Full lifecycle:
/// 1. Apply a filter + undo + redo + clear in-memory.
/// 2. Re-apply a filter to reach a known "current" state, persist to session.json.
/// 3. Simulate crash by dropping the ViewModel.
/// 4. Reload from disk, rehydrate a new ViewModel by replaying the active stack
///    (stack[0..undo_cursor]), drive the engine, and assert the view returns the
///    expected row count.
#[tokio::test]
async fn full_loop_persist_then_restore() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_table(&tmp).await;

    let base = format!("\"{}\"", table.replace('"', "\"\""));

    // --- Phase 1: exercise apply / undo / redo / clear ---
    let mut vm = ViewModel::new("tab1".into(), base.clone());

    // apply a=5 filter
    let c_apply = vm.apply(filter_eq("a", 5));
    apply_change(&engine, &c_apply).await;
    assert_eq!(vm.stack().len(), 1);

    // undo → back to base
    let c_undo = vm.undo().expect("undo must return Some after one apply");
    apply_change(&engine, &c_undo).await;
    assert_eq!(vm.cursor(), 0);

    // redo → a=5 view back
    let c_redo = vm.redo().expect("redo must return Some after undo");
    apply_change(&engine, &c_redo).await;
    assert_eq!(vm.cursor(), 1);

    // clear → rebind to base
    let c_clear = vm.clear();
    apply_change(&engine, &c_clear).await;
    assert_eq!(vm.cursor(), 0);

    // --- Phase 2: apply a known filter and persist ---
    // Apply a=3 filter (rows where a == 3; the fixture has exactly one such row).
    let c_final = vm.apply(filter_eq("a", 3));
    apply_change(&engine, &c_final).await;
    assert_eq!(vm.cursor(), 1);

    // Confirm the engine view is correct before crash.
    let pre_view = c_final.new_active_view.as_ref().unwrap().clone();
    let paged_pre = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", pre_view.replace('"', "\"\"")),
            0,
            20,
        )
        .await
        .unwrap();
    assert_eq!(
        paged_pre.total_rows, 1,
        "a=3 matches exactly one row pre-crash"
    );

    // Persist current state to session.json.
    let session_json = tmp.path().join("session.json");
    let state = SessionState {
        schema_version: SESSION_SCHEMA_VERSION,
        tabs: vec![Tab {
            table_name: table.clone(),
            source_path: None,
            transform_stack: vm.stack().to_vec(),
            undo_cursor: vm.cursor(),
            extra: serde_json::Map::new(),
        }],
        active_tab: Some(0),
        sql_tabs: Vec::new(),
        active_sql_tab: None,
        query_history: Vec::new(),
        saved_queries: Vec::new(),
    };
    std::fs::write(&session_json, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    // --- Phase 3: simulate crash by dropping vm ---
    drop(vm);

    // --- Phase 4: reload from disk ---
    let restored = migrate::load(&session_json).unwrap();
    assert_eq!(restored.schema_version, SESSION_SCHEMA_VERSION);
    let tab = &restored.tabs[0];
    assert_eq!(tab.transform_stack.len(), 1);
    assert_eq!(tab.undo_cursor, 1);

    // --- Phase 5: rehydrate ViewModel by replaying active transforms ---
    // Replay stack[0..undo_cursor] via apply(); cursor will land at undo_cursor.
    let mut vm2 = ViewModel::new("tab1".into(), base);
    let mut last_change = None;
    for t in &tab.transform_stack[..tab.undo_cursor] {
        last_change = Some(vm2.apply(t.clone()));
    }
    assert_eq!(vm2.cursor(), tab.undo_cursor);

    // Drive the engine with the final change.
    let final_change = last_change.expect("must have at least one active transform");
    apply_change(&engine, &final_change).await;

    // --- Phase 6: assert the restored view returns the same row count ---
    let restored_view = final_change
        .new_active_view
        .as_ref()
        .expect("active transform must produce a view");
    let paged_post = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", restored_view.replace('"', "\"\"")),
            0,
            20,
        )
        .await
        .unwrap();
    assert_eq!(
        paged_post.total_rows, 1,
        "restored view must return the same 1 row for a=3"
    );

    // --- Phase 7: verify the on-disk session.json declares the current schema
    // (v6) with correct content ---
    let raw = std::fs::read_to_string(&session_json).unwrap();
    assert!(
        raw.contains("\"schema_version\": 6") || raw.contains("\"schema_version\":6"),
        "session.json must declare the current schema_version (6)"
    );
    assert!(
        raw.contains("\"eq\"") || raw.contains("eq"),
        "session.json must contain the serialized filter op"
    );
}
