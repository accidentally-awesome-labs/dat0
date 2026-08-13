//! A table with columns and no rows is a table the grid can open.
//!
//! `GridDataSource::new` probes the schema with a `LIMIT 1` window. DuckDB's
//! `Arrow` iterator yields no batches at all for a zero-row result, so the probe
//! used to fail with "schema probe yielded no batch" — despite the comment above
//! it promising a fallback, and despite [`GridDataSource::is_empty`] existing
//! for exactly this case with the doc "the user opened a freshly-created empty
//! table".
//!
//! Two user-visible consequences, both fixed by `run_page` capturing
//! `Arrow::get_schema` before draining and returning one zero-row batch:
//!
//! 1. Importing a header-only CSV surfaced an error banner instead of an empty
//!    grid — the shell's `source` resource maps the `Err` straight through.
//! 2. `is_empty()` was unreachable: construction failed before any caller could
//!    observe `row_count == 0`.
//!
//! Both directions are asserted here, because the fix is only worth having if
//! the *columns* survive: an empty grid that also forgot its headers would be
//! the same failure wearing a different face.

use std::sync::Arc;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use tempfile::TempDir;

/// A real CSV with a header row and no data rows, registered through the same
/// engine call a file drop uses.
async fn header_only_csv() -> (Arc<DuckDBEngine>, String, TempDir) {
    let tmp = TempDir::new().unwrap();
    let csv = tmp.path().join("empty.csv");
    std::fs::write(&csv, "id,region,revenue,active\n").unwrap();

    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    let engine = Arc::new(engine);

    let info = engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .expect("a header-only CSV is a legitimate import");
    (engine, info.name, tmp)
}

#[tokio::test]
async fn a_header_only_csv_binds_to_the_grid() {
    let (engine, table, _tmp) = header_only_csv().await;

    let ds = dat0_core::grid::GridDataSource::new(engine, table)
        .await
        .expect("a table with no rows is still a table the grid can show");

    assert_eq!(ds.row_count, 0, "the CSV has a header and nothing else");
    assert!(
        ds.is_empty(),
        "is_empty exists for this exact case and was unreachable while \
         construction failed first"
    );
    assert_eq!(
        ds.visible_column_names(),
        vec!["id", "region", "revenue", "active"],
        "an empty grid must still know its columns — otherwise the header row \
         is gone too and the user cannot see what they imported"
    );
}

/// The synchronous render path must degrade to "nothing here", not to a panic
/// or a placeholder, because `Grid` calls it for every visible cell.
#[tokio::test]
async fn an_empty_source_reads_no_cells() {
    let (engine, table, _tmp) = header_only_csv().await;
    let ds = dat0_core::grid::GridDataSource::new(engine, table)
        .await
        .unwrap();

    assert_eq!(ds.cell_display(0, 0), None, "there is no row 0");
    assert_eq!(
        ds.column_name(0).as_deref(),
        Some("id"),
        "the columns are addressable even with no rows behind them"
    );
}
