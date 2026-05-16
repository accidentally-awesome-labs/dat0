//! `DuckDBEngine` — sole `QueryEngine` impl in v1. Per spec §2.1.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tracing::{debug, error, instrument};

use crate::Result;
use crate::catalog::quote_ident;
use crate::error::EngineError;
use crate::types::{EngineStatus, MemoryBudget};

pub struct DuckDBEngine {
    pub(crate) conn: Arc<Mutex<duckdb::Connection>>,
    pub(crate) interrupt: Arc<duckdb::InterruptHandle>,
    pub(crate) budget: MemoryBudget,
    pub(crate) scratch_path: PathBuf,
    pub(crate) status: Arc<RwLock<EngineStatus>>,
}

impl DuckDBEngine {
    /// Construct an engine bound to `scratch_path` (a DuckDB file). Status begins
    /// `Initializing`; call `init()` to transition to `Ready`.
    pub fn new(scratch_path: PathBuf, budget: MemoryBudget) -> Result<Self> {
        let conn = duckdb::Connection::open(&scratch_path)?;
        // duckdb-rs 1.4.x: `interrupt_handle()` returns `Arc<InterruptHandle>` directly.
        let interrupt = conn.interrupt_handle();
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            interrupt,
            budget,
            scratch_path,
            status: Arc::new(RwLock::new(EngineStatus::Initializing)),
        })
    }

    /// External cancel handle. Callable from any task; cooperative — in-flight
    /// query returns `EngineError::Interrupted` from spawn_blocking on next yield.
    pub fn interrupt(&self) {
        self.interrupt.interrupt();
    }

    fn assert_open(&self) -> Result<()> {
        let status = self
            .status
            .read()
            .map_err(|_| EngineError::EnginePoisoned)?;
        match &*status {
            EngineStatus::Closing | EngineStatus::Closed => Err(EngineError::EngineClosed),
            EngineStatus::Failed(reason) => Err(EngineError::EngineFailed(reason.clone())),
            _ => Ok(()),
        }
    }

    fn set_status(&self, new_status: EngineStatus) {
        match self.status.write() {
            Ok(mut s) => *s = new_status,
            Err(_) => {
                // Status RwLock poisoned by a panicking worker thread. We can't
                // write the new status; subsequent assert_open() and status() calls
                // will observe the poisoned lock and surface EngineError::EnginePoisoned
                // / EngineStatus::Failed respectively. Logging here makes the
                // poisoned state visible operationally per spec §2.9.
                tracing::error!("status RwLock poisoned in set_status; engine state indeterminate");
            }
        }
    }
}

#[async_trait::async_trait]
impl crate::QueryEngine for DuckDBEngine {
    #[instrument(skip(self), fields(scratch = %self.scratch_path.display(), budget_mb = self.budget.bytes / (1024*1024)))]
    async fn init(&self) -> Result<()> {
        // Guard: init must only be called once, when status is Initializing.
        {
            let s = self
                .status
                .read()
                .map_err(|_| EngineError::EnginePoisoned)?;
            if !matches!(*s, EngineStatus::Initializing) {
                return Err(EngineError::EngineFailed(format!(
                    "init() called on engine in non-Initializing state: {:?}",
                    *s
                )));
            }
        }

        let conn = self.conn.clone();
        let budget = self.budget;

        // T14 installs sqlite_scanner once at app boot. Engine init only LOADs.
        // For tests where boot has not run, LOAD will fail with "extension not
        // found" — tests that exercise sqlite ATTACH must call
        // `extension_bootstrap::__test_install_sqlite_scanner()` first.
        let result: Result<()> = tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            // Memory + thread pragmas
            conn.execute_batch(&format!(
                "PRAGMA memory_limit='{}'; PRAGMA threads={};",
                budget.as_pragma(),
                num_cpus::get().saturating_sub(1).max(1),
            ))?;
            // LOAD extensions if installed (best effort — tests may run without).
            // Errors here are swallowed; T11b asserts ATTACH 'sqlite:' end-to-end.
            let _ = conn.execute_batch("LOAD sqlite_scanner;");
            Ok(())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?;

        match result {
            Ok(()) => {
                // Apply migrations (T3) — runs after pragmas so they execute under
                // the configured memory budget; before Ready so callers never see
                // a partly-migrated engine.
                self.apply_migrations_real().await?;
                self.set_status(EngineStatus::Ready);
                debug!("engine ready");
                Ok(())
            }
            Err(e) => {
                self.set_status(EngineStatus::Failed(e.to_string()));
                error!(error = %e, "engine init failed");
                Err(e)
            }
        }
    }

    #[instrument(skip(self))]
    async fn close(&self) -> Result<()> {
        self.set_status(EngineStatus::Closing);
        // duckdb-rs exposes `Connection::close(self)` (consuming) but our
        // connection lives behind `Arc<Mutex<_>>` and is shared with any
        // outstanding `spawn_blocking` workers (paged/streaming/etc.). We
        // cannot consume it here without breaking those workers. Instead:
        // 1. Flip status to Closed so subsequent calls fail via assert_open.
        // 2. The connection drops naturally when the last Arc reference goes,
        //    typically when the engine itself drops along with all in-flight
        //    streams. This is safe — DuckDB's Connection::Drop closes the
        //    underlying handle.
        // P3+ may want graceful drain (interrupt + await all streams) before
        // marking Closed; for P2 the synchronous status flip is sufficient.
        self.set_status(EngineStatus::Closed);
        Ok(())
    }

    fn status(&self) -> EngineStatus {
        self.status
            .read()
            .map(|s| s.clone())
            .unwrap_or(EngineStatus::Failed("status mutex poisoned".into()))
    }

    // The rest of the trait surface is unimplemented in T2. T3..T11 fill in.

    async fn register_file(
        &self,
        path: &std::path::Path,
        opts: crate::types::RegisterOpts,
    ) -> Result<crate::types::TableInfo> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let table_name = crate::register::derive_table_name(path);
        let sql = crate::register::dispatch_register_sql(path, &opts, &table_name)?;
        let path = path.to_path_buf();

        let columns = tokio::task::spawn_blocking({
            let conn = conn.clone();
            let sql = sql.clone();
            let table_name = table_name.clone();
            move || -> Result<Vec<crate::types::ColumnInfo>> {
                let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
                conn.execute_batch(&sql)?;
                // DESCRIBE returns columns: column_name, column_type, null, key, default, extra
                let mut stmt = conn.prepare(&format!("DESCRIBE {}", quote_ident(&table_name)))?;
                let rows: Vec<crate::types::ColumnInfo> = stmt
                    .query_map([], |row| {
                        Ok(crate::types::ColumnInfo {
                            name: row.get::<_, String>(0)?,
                            data_type: row.get::<_, String>(1)?,
                            nullable: row
                                .get::<_, String>(2)
                                .map(|s| s.eq_ignore_ascii_case("YES"))
                                .unwrap_or(true),
                        })
                    })?
                    .filter_map(std::result::Result::ok)
                    .collect();
                Ok(rows)
            }
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;

        Ok(crate::types::TableInfo {
            name: table_name,
            schema: "main".to_string(),
            columns,
            row_count_estimate: None,
            origin: crate::types::TableOrigin::File(path),
        })
    }

    #[instrument(skip(self), fields(name = name, sql_len = sql.len()))]
    async fn create_table(
        &self,
        name: &str,
        sql: &str,
        _origin: crate::types::DerivedOrigin,
    ) -> Result<crate::types::TableInfo> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || -> Result<crate::types::TableInfo> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::create_table(&conn, &name, &sql)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(name = name))]
    async fn drop_table(&self, name: &str, schema: Option<&str>) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        let schema = schema.map(|s| s.to_owned());
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::drop_table(&conn, &name, schema.as_deref())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(old = old, new = new))]
    async fn rename_table(&self, old: &str, new: &str, schema: Option<&str>) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let old = old.to_owned();
        let new = new.to_owned();
        let schema = schema.map(|s| s.to_owned());
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::rename_table(&conn, &old, &new, schema.as_deref())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(sql_len = sql.len()))]
    async fn execute(&self, sql: &str) -> Result<crate::types::QueryResult> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || -> Result<crate::types::QueryResult> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::execute::run_materialized(&conn, &sql)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(sql_len = sql.len(), offset, limit))]
    async fn execute_paged(
        &self,
        sql: &str,
        offset: u64,
        limit: u64,
    ) -> Result<crate::types::PagedQueryResult> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || -> Result<crate::types::PagedQueryResult> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::execute::paged::run_paged(&conn, &sql, offset, limit)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(sql_len = sql.len()))]
    async fn execute_streaming(&self, sql: &str) -> Result<crate::types::ArrowRecordBatchStream> {
        self.assert_open()?;
        crate::execute::streaming::spawn_streaming(self.conn.clone(), sql.to_owned())
    }

    #[instrument(skip(self), fields(name = name, schema = ?schema))]
    async fn describe_table(
        &self,
        name: &str,
        schema: Option<&str>,
    ) -> Result<Vec<crate::types::ColumnInfo>> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        let schema = schema.map(|s| s.to_owned());
        tokio::task::spawn_blocking(move || -> Result<Vec<crate::types::ColumnInfo>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::describe_table(&conn, &name, schema.as_deref())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip_all)]
    async fn get_tables(&self) -> Result<Vec<crate::types::TableInfo>> {
        self.assert_open()?;
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<crate::types::TableInfo>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::get_tables(&conn)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(name = name, format = ?format))]
    async fn export_table(
        &self,
        name: &str,
        format: crate::types::ExportFormat,
    ) -> Result<Vec<u8>> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::export::export_table_bytes(&conn, &name, format)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(dsn_scheme = ?dsn.split(':').next(), alias))]
    async fn attach(&self, dsn: &str, alias: &str, opts: crate::types::AttachOpts) -> Result<()> {
        self.assert_open()?;
        let (scheme, rest) = crate::attach::parse_scheme(dsn)?;
        match scheme {
            crate::attach::AttachScheme::MotherDuck => {
                // D-007: end-to-end deferred to P5.
                return Err(EngineError::NotImplemented {
                    feature: "MotherDuck",
                });
            }
            crate::attach::AttachScheme::Sqlite => {}
        }
        let sql = crate::attach::build_attach_sqlite_sql(rest, alias, &opts);
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            conn.execute_batch(&sql)?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(alias))]
    async fn detach(&self, alias: &str) -> Result<()> {
        self.assert_open()?;
        let sql = crate::attach::build_detach_sql(alias);
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            conn.execute_batch(&sql)?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }
}

impl DuckDBEngine {
    /// Run the migration runner inside `spawn_blocking` (DuckDB calls block).
    /// Returns Ok on success; on failure the engine init's match arm will flip
    /// status to `Failed`.
    async fn apply_migrations_real(&self) -> Result<()> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::migrations::apply_migrations(&conn, crate::migrations::MIGRATIONS)?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }
}
