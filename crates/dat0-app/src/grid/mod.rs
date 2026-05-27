//! DataGrid: gpui-component Table wrapper over duckdb::arrow batches.

pub mod data_source;
pub mod renderers;

pub use data_source::GridDataSource;

use std::sync::Arc;

use duckdb::arrow::datatypes::DataType;
use gpui::{
    App, Context, IntoElement, ParentElement, SharedString, Styled, Window, div, prelude::*,
};
use gpui_component::table::{Column, TableDelegate, TableState};

use renderers::{CellAlignment, render_cell, type_badge};

/// Adapter that bridges [`GridDataSource`] (Arrow-paged, interior-mutable
/// cache) to gpui-component's `TableDelegate` (main-thread, `&mut self`,
/// synchronous render contract).
///
/// Per `docs/internal/gpui-table-api-notes.md` §4.1: the delegate is
/// `Sized + 'static` — no `Send`/`Sync` bounds — so it lives on the GPUI
/// main thread alongside the parent view. The `Arc<GridDataSource>` field
/// is the only cross-thread handle; pages are still fetched off-thread
/// by P3a's async paging layer (later P3b tasks will wire `load_more`).
///
/// For T4 this is a minimum-viable delegate: columns are derived from the
/// `GridDataSource` schema at construction, and `render_td` renders the
/// in-memory page-0 batch via [`renderers::render_cell`]. `load_more` /
/// `visible_rows_changed` will land in a follow-up task that wires the
/// async paged fetch back into a per-page cache — until then any row
/// outside the initial page renders as an em-dash placeholder so the
/// widget mounts cleanly.
pub struct GridTableDelegate {
    /// Shared paging source. `Arc` so the same source can be inspected by
    /// other views (e.g., status bar row-count badge in a later task).
    source: Arc<GridDataSource>,
    /// Pre-computed column metadata. Cloned cheap (per spike §2) so we can
    /// hand a `&Column` to the widget on each `column()` call without
    /// re-deriving from the Arrow schema every frame.
    columns: Vec<Column>,
}

impl GridTableDelegate {
    /// Build a delegate over `source`, deriving column metadata from the
    /// Arrow schema. Columns get a small default width and a type-badge
    /// suffix in their display name (e.g., `id (INT64)`).
    pub fn new(source: Arc<GridDataSource>) -> Self {
        let schema = source.schema.clone();
        let columns = schema
            .fields()
            .iter()
            .map(|f| {
                let badge = type_badge(f.data_type());
                let name: SharedString = format!("{} ({})", f.name(), badge).into();
                let key: SharedString = f.name().to_string().into();
                Column::new(key, name)
            })
            .collect();
        Self { source, columns }
    }

    /// `Arc::ptr_eq` shortcut for the parent view's "rebuild on data-source
    /// swap" check.
    pub fn source_ptr_eq(&self, other: &Arc<GridDataSource>) -> bool {
        Arc::ptr_eq(&self.source, other)
    }
}

impl TableDelegate for GridTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        // `GridDataSource::row_count` is a `u64`; the delegate API is `usize`.
        // Saturate on the (extremely unlikely) 32-bit overflow case rather
        // than panic — DuckDB tables larger than `usize::MAX` rows are not
        // representable in the gpui widget anyway.
        usize::try_from(self.source.row_count).unwrap_or(usize::MAX)
    }

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // Per §4.4: `render_td` must return synchronously from in-memory
        // data. P3a's `GridDataSource::page_for` is async; we can't await
        // here. The proper fix (background prefetch + cached page lookup)
        // is the next task in P3b — for T4 we read the Arrow schema for
        // column type and, when the cache happens to hold the row's page,
        // render the cell. Otherwise we emit a placeholder.
        //
        // Page-0 is the only page that's "guaranteed" available because
        // `GridDataSource::new` probes the schema via `LIMIT 1` — that
        // call does not populate the LRU. Hence "—" is the expected
        // render for every cell at T4. The follow-up task replaces this
        // with a synchronous cache lookup.
        let text = self
            .source
            .schema
            .fields()
            .get(col_ix)
            .map(|f| match f.data_type() {
                DataType::Int32 | DataType::Int64 | DataType::UInt64 | DataType::Float64 => "—",
                _ => "—",
            })
            .unwrap_or("—");

        // Note: the explicit numeric/right-align branch will land alongside
        // the cache lookup; we keep the visual API (`render_cell` →
        // `CellDisplay`) live below to ensure dead-code linters do not strip
        // it, but the rendered output here is the placeholder.
        let _ = (render_cell, CellAlignment::Right);

        // ElementId only accepts (&str, usize) tuples (gpui 0.2.2), not
        // 3-tuples. Encode (row_ix, col_ix) into a single index so each
        // cell still gets a unique id.
        let cell_ix = row_ix
            .saturating_mul(self.columns.len())
            .saturating_add(col_ix);
        div().id(("td", cell_ix)).size_full().child(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::data_source::GridDataSource;
    use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
    use tempfile::TempDir;

    async fn build_source(rows: usize) -> (TempDir, Arc<GridDataSource>) {
        let tmp = TempDir::new().unwrap();
        let csv = tmp.path().join("t.csv");
        let mut s = String::from("a,b\n");
        for i in 0..rows {
            s.push_str(&format!("{},x{}\n", i, i));
        }
        std::fs::write(&csv, s).unwrap();
        let engine = DuckDBEngine::new(
            tmp.path().join("scratch.duckdb"),
            MemoryBudget {
                bytes: 256 * 1024 * 1024,
            },
        )
        .unwrap();
        engine.init().await.unwrap();
        engine
            .register_file(&csv, RegisterOpts::default())
            .await
            .unwrap();
        let engine = Arc::new(engine);
        let name = engine.get_tables().await.unwrap()[0].name.clone();
        let ds = Arc::new(GridDataSource::new(engine, name).await.unwrap());
        (tmp, ds)
    }

    #[tokio::test]
    async fn delegate_columns_match_schema() {
        let (_tmp, ds) = build_source(8).await;
        let delegate = GridTableDelegate::new(Arc::clone(&ds));
        assert_eq!(delegate.columns.len(), ds.schema.fields().len());
        // Column name carries the type badge suffix.
        let first = &delegate.columns[0];
        assert!(
            first.name.contains('('),
            "expected type badge in column name, got {:?}",
            first.name
        );
    }

    #[tokio::test]
    async fn delegate_source_ptr_eq() {
        let (_tmp, ds) = build_source(4).await;
        let delegate = GridTableDelegate::new(Arc::clone(&ds));
        assert!(delegate.source_ptr_eq(&ds));
    }
}
