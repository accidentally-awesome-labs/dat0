//! dat0 query engine.
//!
//! Public API: the [`QueryEngine`] trait and the [`DuckDBEngine`] implementation.
//! See `docs/specs/2026-04-27-dat0-p2-engine-design.md` for the architectural
//! contract.

pub(crate) mod attach;
pub(crate) mod catalog;
pub mod duckdb_engine;
pub mod error;
pub(crate) mod execute;
pub(crate) mod export;
pub mod extension_bootstrap;
pub mod migrations;
pub(crate) mod register;
pub(crate) mod tracing_helpers;
pub mod trait_def;
pub mod transform;
pub mod types;

pub use duckdb_engine::DuckDBEngine;
pub use error::EngineError;
pub use trait_def::QueryEngine;
pub use transform::{FilterOp, FilterValue, Scalar, SortDirection, SortKey, Transformation};
pub use types::{
    ArrowRecordBatchStream, AttachOpts, ColumnInfo, DerivedOrigin, EngineStatus, ExportFormat,
    FileFormat, MemoryBudget, PagedQueryResult, QueryResult, RegisterOpts, TableInfo, TableOrigin,
};

/// Result type for engine operations.
pub type Result<T> = std::result::Result<T, EngineError>;
