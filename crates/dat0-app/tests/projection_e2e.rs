//! End-to-end: projection transforms (Reorder / Rename / DeleteColumn) + the
//! display-only fast path + surrogate-stripped export, all driven against a REAL
//! `DuckDBEngine` and a materialized rowid-bearing table.
//!
//! Modeled on `view_restore_e2e.rs::full_loop_persist_then_restore` (same engine
//! setup + `apply_change` apply/replay helper). The flow exercises the P4c
//! projection slice (Option B): projection ops are display-only — they fold into
//! the grid `ColumnView` (`fold_columns`) and the export projection
//! (`render_export_select(build_export(...))`) but never touch `compile_view_sql`
//! (the data view SQL is byte-identical with or without them). A `Filter` is the
//! one stage that changes the data SQL + drops rows.
//!
//! ## Adaptation note (real engine vs. plan outline)
//!
//! The plan's numbered outline (`projection_filter_export_remove_undo_restore`)
//! lists Reorder→Rename→Delete→Filter and asks that each projection `apply` be
//! `is_display_only()`. Against the real `ViewModel::regenerate_view`, the
//! display-only fast path only fires once an active view already exists (it
//! compares the recompiled data SQL to the SQL backing the current view — see
//! `view/model.rs`). So the FIRST op on a fresh tab always materializes a view,
//! even a projection op. To assert real behavior faithfully, this test
//! establishes the data view with the row-dropping `Filter` FIRST, then layers
//! the projection ops on top — at which point every projection `apply` IS
//! `is_display_only()` (matching the `history_zipper::projection_apply_is_display_only_no_sql`
//! unit contract). All of the plan's stage assertions (fold reflects
//! reorder/rename/delete, row count drops, export strips the surrogate + applies
//! the projection, remove/undo restore) are kept and made against real behavior.

use std::sync::Arc;

use tempfile::TempDir;

use dat0_app::session::{SESSION_SCHEMA_VERSION, SessionState, Tab, migrate};
use dat0_app::view::column_view::fold_columns;
use dat0_app::view::export_dialog::{ExportScope, build_export};
use dat0_app::view::{ViewChange, ViewModel};
use dat0_engine::types::ExportFormat;
use dat0_engine::{
    DuckDBEngine, FilterOp, FilterValue, MemoryBudget, ProjectionColumn, QueryEngine, ROWID_COL,
    RegisterOpts, Scalar, Transformation,
};

// ---------------------------------------------------------------------------
// Fixture helpers (mirror view_restore_e2e.rs)
// ---------------------------------------------------------------------------

/// Spin up a DuckDB engine backed by a small CSV materialized to a rowid-bearing
/// BASE TABLE (via `register_file_as_table`, the P4b import-materialization path,
/// PD-017). Columns: `id` INTEGER, `city` TEXT, `amt` INTEGER. The table carries
/// the `__dat0_rowid` surrogate so the export-strip assertion is meaningful.
/// Returns the engine (Arc-wrapped) and the registered table name.
async fn engine_with_table(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
    let csv = tmp.path().join("t.csv");
    let mut s = String::from("id,city,amt\n");
    for i in 0..10_i64 {
        // amt == 100 for exactly one row (id == 3); every other row differs.
        let amt = if i == 3 { 100 } else { 200 + i };
        s.push_str(&format!("{},c{},{}\n", i, i, amt));
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
    // register_file_as_table materializes a base table + eagerly injects the
    // __dat0_rowid surrogate (idempotent, COMMENT-marked).
    let info = engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let name = info.name.clone();
    (Arc::new(engine), name)
}

/// Build a `Filter { amt == v }` transformation (struct-variant per PD-015).
fn filter_amt_eq(v: i64) -> Transformation {
    Transformation::Filter {
        column: "amt".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(v),
        },
    }
}

/// Apply a ViewChange to the engine: create the new view if SQL is present,
/// then drop the old view if one was previously active. Identical to the
/// `apply_change` helper in `view_restore_e2e.rs`.
async fn apply_change(engine: &DuckDBEngine, change: &ViewChange) {
    if let (Some(name), Some(sql)) = (&change.new_active_view, &change.sql) {
        engine.create_or_replace_view(name, sql).await.unwrap();
    }
    if let Some(prev) = &change.previous_active_view {
        let _ = engine.drop_view(prev).await; // best-effort
    }
}

/// Source-column list for the base table EXCLUDING the surrogate — what
/// `fold_columns`/`build_export` consume as the visible base columns.
async fn visible_base_columns(engine: &DuckDBEngine, table: &str) -> Vec<String> {
    engine
        .describe_table(table, None)
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .filter(|n| n != ROWID_COL)
        .collect()
}

/// Total rows returned by a view (via execute_paged total_rows).
async fn view_row_count(engine: &DuckDBEngine, view: &str) -> u64 {
    engine
        .execute_paged(
            &format!("SELECT * FROM \"{}\"", view.replace('"', "\"\"")),
            0,
            50,
        )
        .await
        .unwrap()
        .total_rows
}

// ---------------------------------------------------------------------------
// The projection E2E
// ---------------------------------------------------------------------------

#[tokio::test]
async fn projection_filter_export_remove_undo_restore() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_table(&tmp).await;

    let base_quoted = format!("\"{}\"", table.replace('"', "\"\""));
    let base_cols = visible_base_columns(&engine, &table).await;
    assert_eq!(
        base_cols,
        vec!["id".to_string(), "city".to_string(), "amt".to_string()],
        "fixture base columns (surrogate excluded)"
    );

    let mut vm = ViewModel::new("tab1".into(), base_quoted.clone());

    // ---- Stage 4 (engine-ordering first): Filter on a still-source column ----
    // The Filter is on `amt`, a base column that is NOT renamed/deleted in the
    // fold below, so it compiles cleanly. This is a NON-display-only ViewChange:
    // it changes the data SQL and materializes a real engine view; the row count
    // drops to the single amt==100 row.
    let c_filter = vm.apply(filter_amt_eq(100));
    assert!(
        !c_filter.is_display_only(),
        "a Filter changes the data SQL → NOT display-only, got {c_filter:?}"
    );
    assert!(c_filter.sql.is_some(), "Filter must emit data SQL");
    apply_change(&engine, &c_filter).await;
    let view_after_filter = c_filter.new_active_view.clone().unwrap();
    assert_eq!(
        view_row_count(&engine, &view_after_filter).await,
        1,
        "amt==100 matches exactly one row (id==3); row count drops from 10 → 1"
    );

    // ---- Stage 1: Reorder columns → ColumnView order changes; SQL unchanged ----
    // New visible source order: amt, id, city (move amt to front).
    let reorder = Transformation::Reorder {
        columns: vec!["amt".into(), "id".into(), "city".into()],
    };
    let c_reorder = vm.apply(reorder);
    assert!(
        c_reorder.is_display_only(),
        "reorder recompiles to identical data SQL → display-only, got {c_reorder:?}"
    );
    assert!(
        c_reorder.sql.is_none(),
        "display-only change carries no SQL"
    );
    assert_eq!(
        c_reorder.previous_active_view, None,
        "display-only change must not drop the live view"
    );
    // Engine view is untouched: re-applying the (no-SQL) change is a no-op and
    // the row count is still 1.
    apply_change(&engine, &c_reorder).await;
    assert_eq!(view_row_count(&engine, &view_after_filter).await, 1);
    // The fold reflects the new order.
    let folded = fold_columns(&base_cols, vm.active());
    assert_eq!(
        folded.iter().map(|c| c.source.as_str()).collect::<Vec<_>>(),
        vec!["amt", "id", "city"],
        "ColumnView order tracks the Reorder op"
    );

    // ---- Stage 2: Rename a column → display-only; fold shows the new label ----
    let rename = Transformation::Rename {
        column: "city".into(),
        to: "City".into(),
    };
    let c_rename = vm.apply(rename);
    assert!(
        c_rename.is_display_only(),
        "rename recompiles to identical data SQL → display-only, got {c_rename:?}"
    );
    apply_change(&engine, &c_rename).await;
    let folded = fold_columns(&base_cols, vm.active());
    let city_col = folded.iter().find(|c| c.source == "city").unwrap();
    assert_eq!(
        city_col.display, "City",
        "fold shows the renamed display label; source identity is unchanged"
    );

    // ---- Stage 3: DeleteColumn → fold excludes it ----
    let delete = Transformation::DeleteColumn {
        columns: vec!["id".into()],
    };
    let c_delete = vm.apply(delete);
    assert!(
        c_delete.is_display_only(),
        "delete-column recompiles to identical data SQL → display-only, got {c_delete:?}"
    );
    apply_change(&engine, &c_delete).await;
    let folded = fold_columns(&base_cols, vm.active());
    assert!(
        !folded.iter().any(|c| c.source == "id"),
        "fold excludes the deleted column"
    );
    // After delete the visible projection is: amt, City(city).
    assert_eq!(
        folded,
        vec![
            ProjectionColumn {
                source: "amt".into(),
                display: "amt".into()
            },
            ProjectionColumn {
                source: "city".into(),
                display: "City".into()
            },
        ],
        "visible projection after reorder+rename+delete"
    );

    // ---- Stage 5: Export current view → read back ----
    // build_export(CurrentView, …) reads the active engine view (filter applied)
    // and projects through the folded ColumnView; render_export_select strips the
    // surrogate by omission and aliases the rename.
    let active_view = vm.active_view().expect("a view is active after the filter");
    let active_view_quoted = format!("\"{}\"", active_view.replace('"', "\"\""));
    let (inner, cols) = build_export(
        ExportScope::CurrentView,
        &base_quoted,
        Some(&active_view_quoted),
        &folded,
        &base_cols,
    );
    let select = dat0_engine::render_export_select(&inner, &cols);
    let dest = tmp.path().join("current_view.csv");
    engine
        .export_query_to_path(&select, ExportFormat::Csv, &dest)
        .await
        .unwrap();
    let exported = std::fs::read_to_string(&dest).unwrap();
    let mut lines = exported.lines();
    let header = lines.next().unwrap();
    // Header reflects rename + reorder; deleted column absent; surrogate absent.
    assert_eq!(
        header, "amt,City",
        "export header = reordered+renamed visible cols, deleted col absent: {exported:?}"
    );
    assert!(
        !exported.contains(ROWID_COL),
        "surrogate must be stripped from the export: {exported:?}"
    );
    assert!(
        !exported.contains("\nid") && !header.contains("id"),
        "deleted column 'id' must be absent from the export: {exported:?}"
    );
    // Exactly one data row (the amt==100 filter), value 100 + city c3.
    let data_rows: Vec<&str> = lines.filter(|l| !l.is_empty()).collect();
    assert_eq!(
        data_rows.len(),
        1,
        "export rows reflect the amt==100 filter (1 row): {exported:?}"
    );
    assert_eq!(
        data_rows[0], "100,c3",
        "the surviving row is amt=100, city=c3 (id==3 fixture row)"
    );

    // ---- Stage 6: remove_at the rename → fold reverts the label; filter intact ----
    // Active stack order is [Filter, Reorder, Rename, DeleteColumn]; the Rename
    // is at index 2.
    assert!(
        matches!(vm.active()[2], Transformation::Rename { .. }),
        "rename sits at active index 2"
    );
    let c_remove = vm.remove_at(2);
    apply_change(&engine, &c_remove).await;
    let folded_no_rename = fold_columns(&base_cols, vm.active());
    let city_col = folded_no_rename
        .iter()
        .find(|c| c.source == "city")
        .unwrap();
    assert_eq!(
        city_col.display, "city",
        "removing the Rename reverts the display label to the source name"
    );
    // The Filter survives the removal and still drives a 1-row view.
    assert!(
        vm.active()
            .iter()
            .any(|t| matches!(t, Transformation::Filter { .. })),
        "filter intact after removing the rename"
    );
    assert_eq!(
        view_row_count(&engine, vm.active_view().unwrap()).await,
        1,
        "filter still yields 1 row after the rename removal"
    );

    // ---- Stage 7: undo → rename restored ----
    let c_undo = vm.undo().expect("undo must yield a ViewChange");
    apply_change(&engine, &c_undo).await;
    let folded_redone = fold_columns(&base_cols, vm.active());
    let city_col = folded_redone.iter().find(|c| c.source == "city").unwrap();
    assert_eq!(
        city_col.display, "City",
        "undo of the remove restores the Rename → display label is 'City' again"
    );
    assert!(
        matches!(vm.active()[2], Transformation::Rename { .. }),
        "rename restored at active index 2 after undo"
    );

    // ---- Stage 8: persist (active stack) → drop → migrate::load → rehydrate ----
    let active_stack = vm.stack().to_vec(); // [Filter, Reorder, Rename, DeleteColumn]
    let active_cursor = vm.cursor();
    assert_eq!(active_stack.len(), 4);
    assert_eq!(active_cursor, 4);

    let session_json = tmp.path().join("session.json");
    let state = SessionState {
        schema_version: SESSION_SCHEMA_VERSION,
        tabs: vec![Tab {
            table_name: table.clone(),
            source_path: None,
            transform_stack: active_stack.clone(),
            undo_cursor: active_cursor,
            extra: serde_json::Map::new(),
        }],
        active_tab: Some(0),
        sql_tabs: Vec::new(),
        active_sql_tab: None,
        query_history: Vec::new(),
        saved_queries: Vec::new(),
        charts: Vec::new(),
        attachments: Vec::new(),
        ui: Default::default(),
    };
    std::fs::write(&session_json, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    // Simulate crash.
    drop(vm);

    // Reload.
    let restored = migrate::load(&session_json).unwrap();
    assert_eq!(
        restored.schema_version, SESSION_SCHEMA_VERSION,
        "reloaded schema_version must be the current schema (8)"
    );
    assert_eq!(restored.tabs.len(), 1);
    assert_eq!(
        restored.tabs[0].transform_stack, active_stack,
        "transform_stack must equal the persisted active slice"
    );
    assert_eq!(restored.tabs[0].undo_cursor, 4);

    // Rehydrate by replaying the active transforms, mirroring the restore path.
    let mut vm2 = ViewModel::new("tab1".into(), base_quoted);
    let mut last_change = None;
    for t in &restored.tabs[0].transform_stack[..restored.tabs[0].undo_cursor] {
        last_change = Some(vm2.apply(t.clone()));
    }
    let final_change = last_change.expect("at least one active transform");
    apply_change(&engine, &final_change).await;

    // The rehydrated fold matches the pre-crash fold (reorder + rename + delete).
    let folded_restored = fold_columns(&base_cols, vm2.active());
    assert_eq!(
        folded_restored,
        vec![
            ProjectionColumn {
                source: "amt".into(),
                display: "amt".into()
            },
            ProjectionColumn {
                source: "city".into(),
                display: "City".into()
            },
        ],
        "restored ColumnView matches the pre-crash projection"
    );

    // The rehydrated DATA view matches: still the amt==100 filter → 1 row.
    let restored_view = vm2.active_view().expect("restored view is active");
    assert_eq!(
        view_row_count(&engine, restored_view).await,
        1,
        "restored data view still yields the single amt==100 row"
    );

    // On-disk session.json declares the current schema (v8).
    let raw = std::fs::read_to_string(&session_json).unwrap();
    assert!(
        raw.contains("\"schema_version\": 8") || raw.contains("\"schema_version\":8"),
        "session.json must declare schema_version 8"
    );

    engine.close().await.unwrap();
}
