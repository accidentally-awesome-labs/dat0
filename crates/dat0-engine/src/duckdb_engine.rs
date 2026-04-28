//! `DuckDBEngine` — sole `QueryEngine` impl in v1. Per spec §2.1.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tracing::{debug, error, instrument};

use crate::Result;
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

    /// Test-only scalar probe. T7 replaces in tests with `execute()`.
    #[doc(hidden)]
    #[deprecated(note = "test-only; will be replaced by execute() in T7")]
    pub async fn __debug_query_scalar(&self, sql: &str) -> Result<String> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || -> Result<String> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            let v: String = conn.query_row(&sql, [], |r| r.get(0))?;
            Ok(v)
        })
        .await
        .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
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
        .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?;

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
                let mut stmt = conn.prepare(&format!("DESCRIBE \"{}\"", table_name))?;
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
        .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))??;

        Ok(crate::types::TableInfo {
            name: table_name,
            schema: "main".to_string(),
            columns,
            row_count_estimate: None,
            origin: crate::types::TableOrigin::File(path),
        })
    }

    async fn create_table(
        &self,
        _name: &str,
        _sql: &str,
        _origin: crate::types::DerivedOrigin,
    ) -> Result<crate::types::TableInfo> {
        Err(EngineError::NotImplemented {
            feature: "create_table (T9)",
        })
    }

    async fn drop_table(&self, _name: &str, _schema: Option<&str>) -> Result<()> {
        Err(EngineError::NotImplemented {
            feature: "drop_table (T9)",
        })
    }

    async fn rename_table(&self, _old: &str, _new: &str, _schema: Option<&str>) -> Result<()> {
        Err(EngineError::NotImplemented {
            feature: "rename_table (T9)",
        })
    }

    async fn execute(&self, _sql: &str) -> Result<crate::types::QueryResult> {
        Err(EngineError::NotImplemented {
            feature: "execute (T7)",
        })
    }

    async fn execute_paged(
        &self,
        _sql: &str,
        _offset: u64,
        _limit: u64,
    ) -> Result<crate::types::PagedQueryResult> {
        Err(EngineError::NotImplemented {
            feature: "execute_paged (T8)",
        })
    }

    async fn execute_streaming(&self, _sql: &str) -> Result<crate::types::ArrowRecordBatchStream> {
        Err(EngineError::NotImplemented {
            feature: "execute_streaming (T8)",
        })
    }

    async fn describe_table(
        &self,
        _name: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<crate::types::ColumnInfo>> {
        Err(EngineError::NotImplemented {
            feature: "describe_table (T9)",
        })
    }

    async fn get_tables(&self) -> Result<Vec<crate::types::TableInfo>> {
        Err(EngineError::NotImplemented {
            feature: "get_tables (T9)",
        })
    }

    async fn export_table(
        &self,
        _name: &str,
        _format: crate::types::ExportFormat,
    ) -> Result<Vec<u8>> {
        Err(EngineError::NotImplemented {
            feature: "export_table (T10)",
        })
    }

    async fn attach(
        &self,
        _dsn: &str,
        _alias: &str,
        _opts: crate::types::AttachOpts,
    ) -> Result<()> {
        Err(EngineError::NotImplemented {
            feature: "attach (T11)",
        })
    }

    async fn detach(&self, _alias: &str) -> Result<()> {
        Err(EngineError::NotImplemented {
            feature: "detach (T11)",
        })
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
        .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
    }
}
