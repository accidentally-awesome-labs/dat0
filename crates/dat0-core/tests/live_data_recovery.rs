//! P7c T12 e2e: live re-import round-trip (headless).
//!
//! Drives a real `DuckDBEngine` end to end: import a CSV, build a transform
//! stack of a structural filter + a rowid-keyed cell edit, re-import the
//! externally-changed file (re-CTAS under the same derived name), and assert:
//!   1. `split_replayable` KEEPS the filter and DROPS the edit (D3 partition);
//!   2. the surviving filter compiles against the base AND recomputes over the
//!      REFRESHED rows — proving re-import + structural replay genuinely
//!      refreshes the data (the row count changes from 2 → 3).
//!
//! The GUI click paths (refresh banner, confirm dialog, recovery Sheet buttons)
//! are not headless-testable and remain manual-UAT items (P7 standing backlog).

use dat0_engine::transform::{
    CellEdit, FilterOp, FilterValue, RowKey, Scalar, Transformation, split_replayable,
};
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine as _, RegisterOpts};

#[tokio::test]
async fn reimport_preserves_structural_drops_rowid() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(
        dir.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    // Initial import: 2 rows.
    let csv = dir.path().join("sales.csv");
    std::fs::write(&csv, "id,qty\n1,5\n2,10\n").unwrap();
    let info = engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .expect("initial import");

    // A structural filter (column-keyed → survives re-import) + a rowid-keyed
    // cell edit (the `__dat0_rowid` surrogate regenerates on re-CTAS → dropped).
    let filter = Transformation::Filter {
        column: "qty".into(),
        op: FilterOp::Gt,
        value: FilterValue::Scalar {
            value: Scalar::Int(4),
        },
    };
    let edit = Transformation::Edit {
        cells: vec![CellEdit {
            row: RowKey::Surrogate { id: 1 },
            column: "qty".into(),
            value: Scalar::Int(999),
        }],
    };
    let split = split_replayable(&[filter.clone(), edit]);
    assert_eq!(split.replayable.len(), 1, "filter survives the re-import");
    assert!(matches!(split.replayable[0], Transformation::Filter { .. }));
    assert_eq!(split.dropped_edits, 1, "the cell edit is discarded");
    assert_eq!(split.dropped_deletes, 0);

    // External change: the file gains a row (id=3, qty=99). Re-import is
    // idempotent CREATE OR REPLACE TABLE under the SAME derived table name.
    std::fs::write(&csv, "id,qty\n1,5\n2,10\n3,99\n").unwrap();
    let reimported = engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .expect("re-import");
    assert_eq!(
        reimported.name, info.name,
        "re-import re-derives the same table name"
    );

    // The surviving structural filter compiles against the base and recomputes
    // over the REFRESHED rows: qty > 4 over {5, 10, 99} → 3 rows (was 2 before).
    let sql = dat0_engine::compile_view_sql(&info.name, &split.replayable).expect("compile filter");
    let page = engine
        .execute_paged(&sql, 0, 100)
        .await
        .expect("paged read of refreshed view");
    assert_eq!(
        page.total_rows,
        Some(3),
        "replayed filter recomputes over the re-imported data (2 rows before, 3 after)"
    );

    engine.close().await.unwrap();
}
