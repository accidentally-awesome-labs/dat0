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

    /// Like [`register_file`](Self::register_file), but MATERIALIZES the import
    /// into a DuckDB BASE TABLE that eagerly carries the `__dat0_rowid`
    /// surrogate, instead of registering a lazy VIEW.
    ///
    /// This is the app's import path (PD-017, Path A): a VIEW cannot be
    /// `ALTER TABLE … ADD COLUMN`-ed, so the surrogate the P4b edit/delete
    /// overlay references (`WHERE __dat0_rowid = …`) only reaches imports when
    /// they are base tables. Implementations REUSE the same `read_csv` /
    /// `read_json` / `read_parquet(…)` SQL `register_file` produces (all P3b
    /// delimiter/type sniffing preserved), materialize it via CTAS, then run the
    /// `ensure_rowid` injection — under one lock so the table is never
    /// observable without its surrogate.
    ///
    /// Trade-off vs `register_file`: the file's data is loaded into the scratch
    /// DuckDB base table rather than read lazily on each scan. For dat0 Scratch
    /// mode this is the accepted behavior (the base table is on-disk in
    /// `scratch.duckdb`, not a RAM-resident copy).
    ///
    /// # Errors
    /// - `EngineError::EngineClosed` — if the engine has been closed.
    /// - `EngineError::UnsupportedFormat` — if the format cannot be resolved.
    /// - `EngineError::DuckDb` — if the read SQL or materialization fails.
    async fn register_file_as_table(&self, path: &Path, opts: RegisterOpts) -> Result<TableInfo>;

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

    /// Profile every column of `name` in one `SUMMARIZE` scan (per design D2):
    /// null%, approx-distinct, per-type min/max, and numeric stats (avg/std/
    /// quartiles) where applicable. Distinct% is approximate (HLL).
    ///
    /// `name` is a plain (unquoted) identifier; quoting is applied internally.
    /// `schema` qualifies the table when present.
    ///
    /// # Errors
    /// - `EngineError::EngineClosed` — if the engine has been closed.
    /// - `EngineError::DuckDb` — if the table does not exist or SUMMARIZE fails.
    /// - `EngineError::EnginePoisoned` — if the connection mutex was poisoned.
    async fn profile_table(
        &self,
        name: &str,
        schema: Option<&str>,
    ) -> Result<crate::profile::TableProfile>;

    async fn export_table(&self, name: &str, format: ExportFormat) -> Result<Vec<u8>>;

    /// Stream a SELECT to `dest` via DuckDB `COPY … TO`. Writes straight to
    /// disk (no in-RAM buffering), so it scales to multi-GB exports. `select_sql`
    /// is a complete SELECT — the caller (export dialog) builds it via
    /// [`crate::render::render_export_select`] to strip the surrogate and apply
    /// any column projection.
    ///
    /// # Errors
    /// - `EngineError::EngineClosed` — if the engine has been closed.
    /// - `EngineError::InvalidPath` — if `dest` is not valid UTF-8.
    /// - `EngineError::DuckDb` — if the COPY fails (bad SQL, unwritable path).
    async fn export_query_to_path(
        &self,
        select_sql: &str,
        format: ExportFormat,
        dest: &std::path::Path,
    ) -> Result<()>;

    async fn attach(&self, dsn: &str, alias: &str, opts: AttachOpts) -> Result<()>;
    async fn detach(&self, alias: &str) -> Result<()>;

    /// Ensure `table` carries the `__dat0_rowid` surrogate (idempotent). Injected
    /// at import; back-filled lazily for pre-P4b tables. See design §5.
    ///
    /// The surrogate is a gap-free `0..n-1` BIGINT key in physical scan order
    /// (stable across reads), tagged with a `dat0:surrogate` column COMMENT that
    /// makes this call idempotent and disambiguates our key from a user column
    /// of the same name. A pre-existing UNMARKED `__dat0_rowid` source column is
    /// renamed to `__dat0_rowid__src` (preserving the user's data) before the
    /// surrogate is injected.
    ///
    /// `table` is a plain (unquoted) identifier; quoting is applied internally.
    ///
    /// # Errors
    /// - `EngineError::EngineClosed` — if the engine has been closed.
    /// - `EngineError::DuckDb` — if `table` is a VIEW or does not exist
    ///   (`ALTER TABLE` only applies to base tables).
    /// - `EngineError::EnginePoisoned` — if the connection mutex was poisoned.
    async fn ensure_rowid(&self, table: &str) -> Result<()>;
}
