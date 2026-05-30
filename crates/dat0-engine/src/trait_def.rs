//! `QueryEngine` trait per design-spec §6.1 verbatim.

use std::path::Path;

use crate::Result;
use crate::types::{
    ArrowRecordBatchStream, AttachOpts, ColumnInfo, DerivedOrigin, EngineStatus, ExportFormat,
    PagedQueryResult, QueryResult, RegisterOpts, TableInfo,
};

#[async_trait::async_trait]
pub trait QueryEngine: Send + Sync {
    async fn init(&self) -> Result<()>;
    async fn close(&self) -> Result<()>;
    fn status(&self) -> EngineStatus;

    async fn register_file(&self, path: &Path, opts: RegisterOpts) -> Result<TableInfo>;
    async fn create_table(&self, name: &str, sql: &str, origin: DerivedOrigin)
    -> Result<TableInfo>;
    async fn drop_table(&self, name: &str, schema: Option<&str>) -> Result<()>;
    async fn rename_table(&self, old: &str, new: &str, schema: Option<&str>) -> Result<()>;

    /// Create or replace a DuckDB TEMP VIEW. View is scoped to the connection
    /// and is dropped automatically when the connection drops.
    ///
    /// Caller owns the view name: pass a plain identifier string (e.g.
    /// `"v_tab3_a1b2"`). This method quotes it via `catalog::quote_ident`
    /// before interpolating into SQL — callers MUST NOT pre-quote the name.
    ///
    /// Idempotent: calling with the same `name` and a different `sql` replaces
    /// the existing view without error.
    ///
    /// # Errors
    /// - `EngineError::EngineClosed` — if the engine has been closed.
    /// - `EngineError::DuckDb` — if `sql` is malformed or references a nonexistent relation.
    /// - `EngineError::EnginePoisoned` — if the connection mutex was poisoned by an earlier panic.
    async fn create_or_replace_view(&self, name: &str, sql: &str) -> Result<()>;

    /// Drop a TEMP VIEW. Idempotent — succeeds whether or not the view exists.
    ///
    /// Caller owns the view name in the same sense as `create_or_replace_view`:
    /// pass the plain identifier; quoting is applied internally.
    ///
    /// # Errors
    /// - `EngineError::EngineClosed` — if the engine has been closed.
    /// - `EngineError::EnginePoisoned` — if the connection mutex was poisoned by an earlier panic.
    async fn drop_view(&self, name: &str) -> Result<()>;

    async fn execute(&self, sql: &str) -> Result<QueryResult>;
    async fn execute_paged(&self, sql: &str, offset: u64, limit: u64) -> Result<PagedQueryResult>;
    async fn execute_streaming(&self, sql: &str) -> Result<ArrowRecordBatchStream>;

    async fn describe_table(&self, name: &str, schema: Option<&str>) -> Result<Vec<ColumnInfo>>;
    async fn get_tables(&self) -> Result<Vec<TableInfo>>;

    async fn export_table(&self, name: &str, format: ExportFormat) -> Result<Vec<u8>>;

    async fn attach(&self, dsn: &str, alias: &str, opts: AttachOpts) -> Result<()>;
    async fn detach(&self, alias: &str) -> Result<()>;
}
