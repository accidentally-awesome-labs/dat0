//! End-to-end (T13 — P4b): ViewModel + session.json **v3** + engine view
//! lifecycle for the in-place EDIT/DELETE overlay.
//!
//! Extends P4a's `view_restore_e2e.rs` (filter/sort) with the P4b mutation
//! transforms: apply Edit + RowDelete + Filter → undo → redo → serialize the
//! session (v3) → simulate crash → reload via `migrate::load` → rehydrate a new
//! ViewModel by replaying the active stack → assert the active stack (incl.
//! Edit/RowDelete), the cursor, and the rebound view SQL all round-trip, and
//! that the reloaded view's SQL carries the `SELECT * REPLACE` overlay (T2).
//!
//! PD-018 sidestep: the grid's *paged cache* is unpopulated in headless tests,
//! so we never assert through the grid. Row-count fidelity is checked by
//! running the rebound view's SQL **directly** via `execute_paged` (a plain
//! query against a live engine view) — the SQL compilation is pure and the
//! direct query does not depend on the grid cache.

use std::sync::Arc;

use tempfile::TempDir;

use dat0_app::session::{SESSION_SCHEMA_VERSION, SessionState, Tab, migrate};
use dat0_app::view::ViewModel;
use dat0_engine::{
    CellEdit, DuckDBEngine, FilterOp, FilterValue, MemoryBudget, QueryEngine, RegisterOpts, RowKey,
    Scalar, Transformation,
};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Spin up a DuckDB engine backed by a 10-row CSV (columns: `a` INTEGER,
/// `b` TEXT), MATERIALIZED into a base table that eagerly carries the
/// `__dat0_rowid` surrogate (PD-017 Path A). The surrogate is required for the
/// Edit/RowDelete overlay SQL (`SELECT * REPLACE (...)` + `WHERE __dat0_rowid
/// NOT IN (...)`) to bind against a real relation.
async fn engine_with_base_table(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
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
    // Materialize to a BASE TABLE with the surrogate (not a lazy VIEW).
    let info = engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let name = info.name.clone();
    (Arc::new(engine), name)
}

/// A single-cell edit: set column `col` on surrogate row `id` to `value`.
fn edit_cell(id: i64, col: &str, value: Scalar) -> CellEdit {
    CellEdit {
        row: RowKey::Surrogate { id },
        column: col.into(),
        value,
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
// Test: apply Edit + RowDelete + Filter → undo → redo → persist v3 → crash →
//       reload → rehydrate → assert stack + cursor + `* REPLACE` SQL.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn edit_delete_filter_round_trip_through_v3_session() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_base_table(&tmp).await;
    let base = format!("\"{}\"", table.replace('"', "\"\""));

    // --- Phase 1: build the mutation stack in-memory ---
    let mut vm = ViewModel::new("tab1".into(), base.clone());

    // (1) Edit: set b = 'EDITED' on row id=2.
    let c_edit = vm.edit_cells(vec![edit_cell(2, "b", Scalar::Str("EDITED".into()))]);
    apply_change(&engine, &c_edit).await;

    // (2) RowDelete: drop row id=0.
    let c_del = vm.delete_rows(vec![RowKey::Surrogate { id: 0 }]);
    apply_change(&engine, &c_del).await;

    // (3) Filter: a >= 0 (keeps every remaining row; exercises the outer WHERE).
    let c_filter = vm.apply(Transformation::Filter {
        column: "a".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Int(0),
        },
    });
    apply_change(&engine, &c_filter).await;

    assert_eq!(vm.stack().len(), 3);
    assert_eq!(vm.cursor(), 3);
    assert!(vm.is_dirty(), "Edit/RowDelete in active stack ⇒ dirty");

    // --- Phase 2: undo (drop the Filter) then redo (restore it) ---
    let c_undo = vm.undo().expect("undo after 3 applies");
    apply_change(&engine, &c_undo).await;
    assert_eq!(vm.cursor(), 2);

    let c_redo = vm.redo().expect("redo after undo");
    apply_change(&engine, &c_redo).await;
    assert_eq!(vm.cursor(), 3);

    // The rebound (post-redo) view SQL must carry the edit overlay.
    let live_sql = c_redo.sql.as_ref().expect("active change carries SQL");
    assert!(
        live_sql.contains("SELECT * REPLACE"),
        "rebound view SQL must contain the edit overlay, got: {live_sql}"
    );

    // Confirm against the live engine view directly (NOT via the grid cache —
    // PD-018). Started with 10 rows; deleted id=0 ⇒ 9 rows; filter keeps all 9.
    let live_view = c_redo.new_active_view.as_ref().unwrap().clone();
    let paged_pre = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", live_view.replace('"', "\"\"")),
            0,
            20,
        )
        .await
        .unwrap();
    assert_eq!(paged_pre.total_rows, 9, "10 rows − 1 deleted (id=0) = 9");

    // --- Phase 3: persist the v3 session ---
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
    };
    std::fs::write(&session_json, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    // The on-disk file must declare v3.
    let raw = std::fs::read_to_string(&session_json).unwrap();
    assert!(
        raw.contains("\"schema_version\": 3") || raw.contains("\"schema_version\":3"),
        "session.json must declare schema_version 3"
    );
    assert_eq!(SESSION_SCHEMA_VERSION, 3, "current schema must be v3");

    // --- Phase 4: simulate crash + reload via migrate::load ---
    drop(vm);
    let restored = migrate::load(&session_json).unwrap();
    assert_eq!(restored.schema_version, 3, "reloaded schema must be v3");

    let tab = &restored.tabs[0];
    assert_eq!(tab.transform_stack.len(), 3, "full stack must survive");
    assert_eq!(tab.undo_cursor, 3, "cursor must survive");
    assert_eq!(restored.active_tab, Some(0));

    // The Edit + RowDelete variants must deserialize back intact.
    assert!(
        matches!(&tab.transform_stack[0], Transformation::Edit { cells }
            if cells == &vec![edit_cell(2, "b", Scalar::Str("EDITED".into()))]),
        "stack[0] must be the Edit transform, got {:?}",
        tab.transform_stack[0]
    );
    assert!(
        matches!(&tab.transform_stack[1], Transformation::RowDelete { rows }
            if rows == &vec![RowKey::Surrogate { id: 0 }]),
        "stack[1] must be the RowDelete transform, got {:?}",
        tab.transform_stack[1]
    );
    assert!(
        matches!(&tab.transform_stack[2], Transformation::Filter { .. }),
        "stack[2] must be the Filter transform, got {:?}",
        tab.transform_stack[2]
    );

    // --- Phase 5: rehydrate a fresh ViewModel by replaying the active stack ---
    let mut vm2 = ViewModel::new("tab1".into(), base);
    let mut last_change = None;
    for t in &tab.transform_stack[..tab.undo_cursor] {
        last_change = Some(vm2.apply(t.clone()));
    }
    assert_eq!(vm2.cursor(), tab.undo_cursor);
    assert!(vm2.is_dirty(), "rehydrated VM must still be dirty");

    let final_change = last_change.expect("active transforms present");

    // The rebound SQL after restore must STILL carry the `SELECT * REPLACE`
    // overlay — i.e. the Edit transform survived the v3 round-trip.
    let restored_sql = final_change.sql.as_ref().expect("rebound SQL");
    assert!(
        restored_sql.contains("SELECT * REPLACE"),
        "restored view SQL must contain the `* REPLACE` overlay, got: {restored_sql}"
    );
    assert!(
        restored_sql.contains("NOT IN"),
        "restored view SQL must contain the row-delete predicate, got: {restored_sql}"
    );

    // --- Phase 6: drive the engine with the restored view; assert row count ---
    apply_change(&engine, &final_change).await;
    let restored_view = final_change.new_active_view.as_ref().expect("view name");
    let paged_post = engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", restored_view.replace('"', "\"\"")),
            0,
            20,
        )
        .await
        .unwrap();
    assert_eq!(
        paged_post.total_rows, 9,
        "restored view must return the same 9 rows (10 − deleted id=0)"
    );

    // And the edited cell must read back as the NEW value through the overlay:
    // filter on b = 'EDITED' so the assertion verifies the overlay actually
    // produced the edited value end-to-end (not merely that the row survives).
    // 1 row matches iff the restored overlay rewrote id=2's `b` to 'EDITED'.
    let edited = engine
        .execute_paged(
            &format!(
                "SELECT b FROM \"{}\" WHERE {} = 2 AND b = 'EDITED'",
                restored_view.replace('"', "\"\""),
                dat0_engine::ROWID_COL
            ),
            0,
            20,
        )
        .await
        .unwrap();
    assert_eq!(
        edited.total_rows, 1,
        "id=2's `b` must read back as 'EDITED' through the restored overlay"
    );
}
