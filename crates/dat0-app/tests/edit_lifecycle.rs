//! P4b edit lifecycle — grid hidden-key plumbing (T5) and beyond (T6/T8 expand
//! this file).
//!
//! T5 covers the `GridDataSource` row-key surface:
//!   - `__dat0_rowid` is present in the bound table's Arrow schema (the surrogate
//!     reaches the grid because views/tables `SELECT *` it),
//!   - it is hidden from `visible_column_names` (and therefore from the rendered
//!     grid columns), and
//!   - `row_key(screen_row)` resolves a screen row to its surrogate `i64`.
//!
//! The harness builds a *base table* via the real `create_table` CTAS path,
//! which T3 wired to auto-inject `__dat0_rowid` (eager surrogate at create
//! time). This deliberately exercises the present-surrogate path; PD-017 (file
//! imports are VIEWs without the surrogate) is handled separately by the
//! controller and is out of T5 scope.

use std::sync::Arc;
use tempfile::TempDir;

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

/// Build a DuckDB engine with an `orders` *base table* (columns `id`, `amount`)
/// that carries `__dat0_rowid`. `create_table` (CTAS) auto-injects the surrogate
/// eagerly at create time (T3), so the bound table genuinely has the key.
///
/// Returns the engine (Arc), the base table name (suitable for
/// `GridDataSource::new`), and the `TempDir` — the caller MUST keep the
/// `TempDir` bound for the test's duration, since the engine holds an open
/// handle to the on-disk db inside it (mirrors the `view_lifecycle.rs`
/// convention; lets T6/T8 add files to the scratch path after calling).
async fn test_engine_with_orders_rowid() -> (Arc<DuckDBEngine>, String, TempDir) {
    let tmp = TempDir::new().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    // CTAS path → base table → eager `__dat0_rowid` injection (T3).
    engine
        .create_table(
            "orders",
            "SELECT * FROM (VALUES (1, 100), (2, 200), (3, 300)) v(id, amount)",
            DerivedOrigin::Sql(
                "SELECT * FROM (VALUES (1, 100), (2, 200), (3, 300)) v(id, amount)".into(),
            ),
        )
        .await
        .unwrap();

    // Hand `tmp` back so the caller keeps the engine's scratch dir alive for the
    // test's duration (dropping it would remove the dir out from under the open
    // db handle).
    (Arc::new(engine), "orders".to_string(), tmp)
}

/// T8 — fill_down: one `Edit` undo step regardless of how many cells are filled.
///
/// Directly calls `ViewModel::edit_cells` with a multi-cell `Vec<CellEdit>`
/// (the shape `fill_down` on WorkspaceShell will produce) and asserts that the
/// undo stack has exactly ONE step — confirming the "ONE transform per op"
/// invariant required by T8.
#[tokio::test]
async fn fill_down_sets_column_from_top_cell_one_undo_step() {
    let (engine, base, _tmp) = test_engine_with_orders_rowid().await;
    let mut vm = dat0_app::view::ViewModel::new("t".into(), base);

    // Simulate fill-down over rows 1..=2 of "amount" from row 0's value 100.
    // (The helper creates: (1,100), (2,200), (3,300) — id and amount columns.)
    let cells: Vec<dat0_engine::CellEdit> = (1i64..=2)
        .map(|r| dat0_engine::CellEdit {
            row: dat0_engine::RowKey::Surrogate { id: r },
            column: "amount".into(),
            value: dat0_engine::Scalar::Int(100),
        })
        .collect();
    let _ = vm.edit_cells(cells);

    // ONE undo step regardless of cell count — the core T8 guarantee.
    assert_eq!(
        vm.active().len(),
        1,
        "fill_down must produce exactly one undo step"
    );

    // Suppress unused-engine warning; the engine is needed to keep _tmp alive.
    let _ = engine;
}

/// T8 — set_null_selection: one `Edit` undo step for multiple cells.
#[tokio::test]
async fn set_null_selection_one_undo_step() {
    let (_engine, base, _tmp) = test_engine_with_orders_rowid().await;
    let mut vm = dat0_app::view::ViewModel::new("t".into(), base);

    // Two selected cells → two CellEdits with Scalar::Null, but ONE Edit transform.
    let cells: Vec<dat0_engine::CellEdit> = vec![
        dat0_engine::CellEdit {
            row: dat0_engine::RowKey::Surrogate { id: 0 },
            column: "amount".into(),
            value: dat0_engine::Scalar::Null,
        },
        dat0_engine::CellEdit {
            row: dat0_engine::RowKey::Surrogate { id: 1 },
            column: "amount".into(),
            value: dat0_engine::Scalar::Null,
        },
    ];
    let _ = vm.edit_cells(cells);
    assert_eq!(
        vm.active().len(),
        1,
        "set_null must produce exactly one undo step"
    );
}

/// T8 — delete_rows: one `RowDelete` undo step for multiple rows.
#[tokio::test]
async fn delete_rows_selection_one_undo_step() {
    let (_engine, base, _tmp) = test_engine_with_orders_rowid().await;
    let mut vm = dat0_app::view::ViewModel::new("t".into(), base);

    // Two selected rows → two RowKeys, but ONE RowDelete transform.
    let keys: Vec<dat0_engine::RowKey> = vec![
        dat0_engine::RowKey::Surrogate { id: 0 },
        dat0_engine::RowKey::Surrogate { id: 1 },
    ];
    let _ = vm.delete_rows(keys);
    assert_eq!(
        vm.active().len(),
        1,
        "delete_rows must produce exactly one undo step"
    );
}

#[tokio::test]
async fn data_source_exposes_row_key_and_hides_column() {
    // After ensure_rowid + a view bind, the source yields __dat0_rowid per screen
    // row but does NOT list it among visible columns.
    let (engine, base, _tmp) = test_engine_with_orders_rowid().await;
    let ds = dat0_app::grid::GridDataSource::new(engine, base)
        .await
        .unwrap();

    // The surrogate is in the Arrow schema (SELECT * carries it) ...
    assert!(
        ds.schema
            .fields()
            .iter()
            .any(|f| f.name() == dat0_engine::ROWID_COL),
        "schema must carry the surrogate column"
    );
    // ... but it is hidden from the visible columns the grid paints.
    assert!(
        ds.visible_column_names()
            .iter()
            .all(|c| c != dat0_engine::ROWID_COL),
        "visible columns must not include the surrogate"
    );
    // The user's data columns are still visible.
    assert_eq!(
        ds.visible_column_names(),
        vec!["id".to_string(), "amount".to_string()],
    );

    // `row_key` is a synchronous cache lookup (it mirrors the synchronous
    // `render_td` contract and never triggers a fetch), so it returns `None`
    // until the row's page is loaded — exactly as it would before the paging
    // layer has prefetched.
    assert!(
        ds.row_key(0).is_none(),
        "row_key before any page load must be None (no panic, graceful)"
    );

    // After loading the row's page, the surrogate resolves per screen row.
    ds.page_for(0).await.unwrap();
    assert_eq!(
        ds.row_key(0),
        Some(0),
        "first screen row's surrogate is 0 (gap-free 0..n-1 scan order)"
    );
    assert!(ds.row_key(1).is_some());
    assert!(ds.row_key(2).is_some());
    // Beyond row count → None (no panic).
    assert!(ds.row_key(999).is_none());
}

/// PD-018 — prefetching a page populates the LRU so the synchronous render-path
/// readers (`cell_display` / `cell_render` / `row_key`) resolve REAL values
/// instead of `None`. This is the cache contract the grid's `render_td` and the
/// copy/paste/edit handlers depend on: before any page load they read nothing;
/// after `page_for` the visible cells light up. Mirrors the T5 `row_key` test
/// above (page_for(0) then assert), extended to the display readers PD-018 wired
/// into `render_td`.
#[tokio::test]
async fn prefetch_populates_cache_so_cell_display_resolves_real_values() {
    let (engine, base, _tmp) = test_engine_with_orders_rowid().await;
    let ds = dat0_app::grid::GridDataSource::new(engine, base)
        .await
        .unwrap();

    // Before any page load the synchronous readers find nothing (the running
    // grid would paint em-dash placeholders — the PD-018 symptom).
    assert!(
        ds.cell_display(0, 0).is_none(),
        "cell_display before prefetch must be None (empty cache → placeholder)"
    );
    assert!(
        ds.cell_render(0, 0).is_none(),
        "cell_render before prefetch must be None (empty cache → placeholder)"
    );

    // Prefetch page 0 — exactly what `WorkspaceShell::prefetch_visible_rows`
    // does off-thread on bind.
    ds.page_for(0).await.unwrap();

    // Now the visible cells resolve their REAL values. Visible columns are
    // `id`, `amount` (the surrogate is hidden); the rows are (1,100),(2,200),(3,300).
    assert_eq!(
        ds.cell_display(0, 0).as_deref(),
        Some("1"),
        "row 0 / col 0 (id) must be the real value after prefetch"
    );
    assert_eq!(
        ds.cell_display(0, 1).as_deref(),
        Some("100"),
        "row 0 / col 1 (amount) must be the real value after prefetch"
    );
    assert_eq!(ds.cell_display(2, 1).as_deref(), Some("300"));

    // `cell_render` returns the structured display the delegate paints (with
    // alignment for the focus-ring / right-align styling).
    let rendered = ds.cell_render(1, 0).expect("cell_render after prefetch");
    assert_eq!(rendered.text, "2");
    assert!(!rendered.is_null);
}

/// P4c T5 fix — the body-cell PAINT path must address by `ColumnView` SOURCE,
/// not by display/schema ordinal, so a (future T6) `Reorder`/`DeleteColumn`
/// paints each header's OWN column underneath it.
///
/// This is the non-identity lock. The `orders` table's VISIBLE schema order is
/// `[id, amount]` with values `(1,100),(2,200),(3,300)`. We model a display-only
/// reorder to `[amount, id]` exactly the way `GridTableDelegate::new`'s
/// `ColumnView` branch does — each rendered `Column`'s `key` is set to the
/// `ProjectionColumn::source`. `render_td` then reads `columns[col_ix].key` and
/// calls `cell_render_for_source(row, key)`. We drive that mapping directly here
/// (no UI gesture, no Reorder dispatch — T6 owns those) and assert that:
///
///   display col 0 (source "amount") paints amount's value (100/200/300), and
///   display col 1 (source "id")     paints id's value (1/2/3),
///
/// i.e. the painted column tracks the source, NOT the ordinal.
///
/// FAILS BEFORE the fix: the unfixed `render_td` read `cell_render(row, col_ix)`,
/// which resolves `col_ix` as a VISIBLE index — so display col 0 would have
/// painted visible col 0 ("id" → 1/2/3) under the "amount" header, and display
/// col 1 would have painted "amount" (100/200/300) under the "id" header — the
/// exact wrong-column defect. `cell_render_for_source` did not even exist, so the
/// new addressing path is what makes the source-keyed reads resolve correctly.
#[tokio::test]
async fn reordered_column_view_paints_by_source_not_ordinal() {
    let (engine, base, _tmp) = test_engine_with_orders_rowid().await;
    let ds = dat0_app::grid::GridDataSource::new(engine, base)
        .await
        .unwrap();

    // Prefetch page 0 so the synchronous render-path readers resolve real values
    // (mirrors `WorkspaceShell::prefetch_visible_rows` on bind).
    ds.page_for(0).await.unwrap();

    // Sanity: in SCHEMA/VISIBLE order, col 0 is `id` and col 1 is `amount`.
    assert_eq!(ds.cell_render(0, 0).expect("id cell").text, "1");
    assert_eq!(ds.cell_render(0, 1).expect("amount cell").text, "100");

    // Model the display-only reorder `[amount, id]` exactly as the delegate's
    // `ColumnView` branch does: the rendered column's `key` is its source name.
    // `render_td` reads `columns[col_ix].key` and calls `cell_render_for_source`.
    // This is the precise display→source→schema mapping the paint path relies on.
    let display_order_sources = ["amount", "id"]; // reversed vs. schema [id, amount]

    // display col 0 → source "amount" → must paint amount's column (100/200/300),
    // NOT the schema-ordinal-0 column ("id"). A schema-index bug returns "1".
    for (row, expected) in [(0usize, "100"), (1, "200"), (2, "300")] {
        let cell = ds
            .cell_render_for_source(row, display_order_sources[0])
            .expect("amount cell after prefetch");
        assert_eq!(
            cell.text, expected,
            "display col 0 (source amount) row {row} must paint amount's value, not id's"
        );
    }

    // display col 1 → source "id" → must paint id's column (1/2/3), NOT the
    // schema-ordinal-1 column ("amount"). A schema-index bug returns "100".
    for (row, expected) in [(0usize, "1"), (1, "2"), (2, "3")] {
        let cell = ds
            .cell_render_for_source(row, display_order_sources[1])
            .expect("id cell after prefetch");
        assert_eq!(
            cell.text, expected,
            "display col 1 (source id) row {row} must paint id's value, not amount's"
        );
    }

    // Cross-check the exact `render_td` fallback contract: the source-keyed read
    // for "amount" must DIFFER from the index-based read at the same display
    // ordinal (col 0), proving the reorder is actually exercised (not a no-op
    // where source order == schema order).
    let by_ordinal_0 = ds.cell_render(0, 0).expect("ordinal-0 cell").text;
    let by_source_amount = ds
        .cell_render_for_source(0, "amount")
        .expect("source amount cell")
        .text;
    assert_ne!(
        by_ordinal_0, by_source_amount,
        "reorder must be non-identity: ordinal-0 (id=1) != source amount (100)"
    );

    // Unknown source → None (no panic), matching the empty/defensive contract.
    assert!(ds.cell_render_for_source(0, "no_such_col").is_none());
    // The hidden surrogate is never a ColumnView source.
    assert!(
        ds.cell_render_for_source(0, dat0_engine::ROWID_COL)
            .is_none(),
        "the __dat0_rowid surrogate is never addressable as a ColumnView source"
    );
}
