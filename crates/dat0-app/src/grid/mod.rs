//! DataGrid: gpui-component Table wrapper over duckdb::arrow batches.

pub mod data_source;
pub mod renderers;

pub use data_source::GridDataSource;
