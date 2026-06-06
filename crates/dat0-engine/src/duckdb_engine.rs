//! `DuckDBEngine` — sole `QueryEngine` impl in v1. Per spec §2.1.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tracing::{debug, error, instrument};

use crate::Result;
use crate::catalog::quote_ident;
use crate::error::EngineError;
use crate::types::{EngineStatus, MemoryBudget, TableOrigin};

pub struct DuckDBEngine {
    pub(crate) conn: Arc<Mutex<duckdb::Connection>>,
    pub(crate) interrupt: Arc<duckdb::InterruptHandle>,
    pub(crate) budget: MemoryBudget,
    pub(crate) scratch_path: PathBuf,
    pub(crate) status: Arc<RwLock<EngineStatus>>,
    pub(crate) table_origins: Arc<RwLock<HashMap<String, TableOrigin>>>,
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
            table_origins: Arc::new(RwLock::new(HashMap::new())),
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

        let info = crate::types::TableInfo {
            name: table_name,
            schema: "main".to_string(),
            columns,
            row_count_estimate: None,
            origin: crate::types::TableOrigin::File(path.clone()),
        };
        self.table_origins
            .write()
            .expect("table_origins poisoned")
            .insert(info.name.clone(), TableOrigin::File(path));
        // NOTE: `register_file` does NOT eagerly inject `__dat0_rowid`. Unlike
        // `create_table` (a real CTAS → base table), every register path builds a
        // `CREATE OR REPLACE VIEW ... AS SELECT * FROM read_csv/read_json/
        // read_parquet(...)` (see `crate::register`). A VIEW cannot be
        // `ALTER TABLE ... ADD COLUMN`-ed, so `ensure_rowid` is inapplicable to a
        // file-registered view as-is.
        //
        // The eager surrogate is injected on the CTAS path (`create_table`); the
        // file-registered view gains row identity when it is materialized into a
        // base table (or via the app-side lazy back-fill once the relation is a
        // base table).
        //
        // T5/T6: pre-P4b tables back-fill via ensure_rowid on first view bind
        // (WorkspaceShell) — app-side, out of T3 engine scope. See hand-off note
        // in the T3 report. Materializing file imports to base tables (so the
        // grid's edit overlay can reference `__dat0_rowid`) is the related
        // app/import-path decision tracked there.
        Ok(info)
    }

    #[instrument(skip(self), fields(path = %path.display()))]
    async fn register_file_as_table(
        &self,
        path: &std::path::Path,
        opts: crate::types::RegisterOpts,
    ) -> Result<crate::types::TableInfo> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let table_name = crate::register::derive_table_name(path);

        // PD-017 Path A1: build the SAME `CREATE OR REPLACE VIEW … AS SELECT *
        // FROM read_*(…)` SQL that `register_file` uses — reusing 100% of the
        // P3b CSV/JSON delimiter+type sniffing — but target a transient
        // intermediate view name. We then CTAS the view into a real base table
        // under the final `table_name`, drop the intermediate view, and inject
        // the `__dat0_rowid` surrogate. All of this runs under a single
        // connection lock so the table is never observable mid-materialization
        // (mirrors the `create_table` invariant).
        //
        // NOTE: `dispatch_register_sql` emits `CREATE OR REPLACE VIEW` for the
        // transient, so we rewrite the leading statement to target `tmp_view`.
        let tmp_view = format!("__dat0_import_tmp_{table_name}");
        let view_sql = crate::register::dispatch_register_sql(path, &opts, &tmp_view)?;
        let path = path.to_path_buf();

        let columns = tokio::task::spawn_blocking({
            let conn = conn.clone();
            let table_name = table_name.clone();
            let tmp_view = tmp_view.clone();
            move || -> Result<Vec<crate::types::ColumnInfo>> {
                let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
                let qt = quote_ident(&table_name);
                let qv = quote_ident(&tmp_view);
                // Materialize the import atomically:
                //   1) Build the sniffing view (transient intermediate). This is
                //      `CREATE OR REPLACE VIEW` (see `register::*::build_*_view_sql`),
                //      so a leftover transient from a crashed pre-transaction-era
                //      import is overwritten in place and cannot wedge a fresh
                //      import — no separate defensive DROP needed.
                //   2) Materialize it into a base table via `CREATE OR REPLACE
                //      TABLE … AS SELECT` — idempotent, so re-importing the same
                //      filename overwrites rather than erroring (restores the
                //      old view-based `register_file` re-import semantics).
                //   3) Drop the intermediate view.
                //
                // We wrap the sequence in `BEGIN … COMMIT` AND explicitly
                // `ROLLBACK` on any error. This explicit rollback is REQUIRED:
                // in duckdb-rs 1.4.4 a statement error mid-transaction does NOT
                // auto-abort the txn — the already-applied `CREATE OR REPLACE
                // VIEW` would otherwise survive a later `COMMIT`/autocommit and
                // leak `__dat0_import_tmp_<name>` permanently (verified against
                // 1.4.4: error→COMMIT leaks; error→ROLLBACK is clean). With the
                // ROLLBACK, ANY failure (CTAS name-clash, disk full, malformed
                // file) unwinds the transient-view create — no leak.
                let batch = format!(
                    "BEGIN TRANSACTION;\n\
                     {view_sql}\n\
                     CREATE OR REPLACE TABLE {qt} AS SELECT * FROM {qv};\n\
                     DROP VIEW {qv};\n\
                     COMMIT;",
                    view_sql = view_sql,
                    qt = qt,
                    qv = qv,
                );
                if let Err(e) = conn.execute_batch(&batch) {
                    // Best-effort rollback; ignore its own error (e.g. if the
                    // failure was on `BEGIN` itself and no txn is open).
                    let _ = conn.execute_batch("ROLLBACK;");
                    return Err(e.into());
                }
                // Inject the surrogate EAGERLY (design §5: "at import"). Runs
                // only on batch success (after COMMIT), under the same lock, so
                // the base table is never visible without its `__dat0_rowid`.
                // `ensure_rowid_blocking` is itself idempotent; running it
                // outside the materialization txn is fine since it only fires on
                // a committed base table.
                ensure_rowid_blocking(&conn, &table_name)?;
                crate::catalog::describe_table(&conn, &table_name, None)
            }
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;

        let info = crate::types::TableInfo {
            name: table_name,
            schema: "main".to_string(),
            columns,
            row_count_estimate: None,
            origin: crate::types::TableOrigin::File(path.clone()),
        };
        self.table_origins
            .write()
            .expect("table_origins poisoned")
            .insert(info.name.clone(), TableOrigin::File(path));
        Ok(info)
    }

    #[instrument(skip(self), fields(name = name, sql_len = sql.len()))]
    async fn create_table(
        &self,
        name: &str,
        sql: &str,
        origin: crate::types::DerivedOrigin,
    ) -> Result<crate::types::TableInfo> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        let sql = sql.to_owned();
        let info = tokio::task::spawn_blocking({
            let name = name.clone();
            let sql = sql.clone();
            move || -> Result<crate::types::TableInfo> {
                let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
                // CTAS first, then inject the `__dat0_rowid` surrogate EAGERLY so
                // fresh derived tables carry row identity at create time (design
                // §5: "at import / table create"). Done under the same lock so
                // the table is never observable without its surrogate. The
                // returned `TableInfo.columns` is re-derived after injection so
                // the catalog reflects the column (catalog::create_table
                // describes before we add it, so re-describe here).
                let mut info = crate::catalog::create_table(&conn, &name, &sql)?;
                ensure_rowid_blocking(&conn, &name)?;
                info.columns = crate::catalog::describe_table(&conn, &name, None)?;
                Ok(info)
            }
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;
        // Honor the PASSED origin (P5b T11): the `table_origins` map is the
        // source of truth that `table_origin(name)` reads. For the console
        // Save-as-Table path the caller passes `Sql(<raw statement>)`; for the
        // grid Save-as-Table path it passes `Transform { parent, ops }` — the
        // lineage-meaningful variant. `origin` is owned and unused by the
        // blocking catalog create (which only consumed `sql`), so it moves
        // cleanly into the insert here.
        self.table_origins
            .write()
            .expect("table_origins poisoned")
            .insert(name, TableOrigin::Derived(origin));
        Ok(info)
    }

    #[instrument(skip(self), fields(name = name))]
    async fn drop_table(&self, name: &str, schema: Option<&str>) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        let schema = schema.map(|s| s.to_owned());
        let name_for_closure = name.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::drop_table(&conn, &name_for_closure, schema.as_deref())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;
        // Remove origin entry only after the DB op succeeds.
        self.table_origins
            .write()
            .expect("table_origins poisoned")
            .remove(&name);
        Ok(())
    }

    #[instrument(skip(self), fields(old = old, new = new))]
    async fn rename_table(&self, old: &str, new: &str, schema: Option<&str>) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let old = old.to_owned();
        let new = new.to_owned();
        let schema = schema.map(|s| s.to_owned());
        let old_for_closure = old.clone();
        let new_for_closure = new.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::rename_table(
                &conn,
                &old_for_closure,
                &new_for_closure,
                schema.as_deref(),
            )
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;
        // Atomically rekey origin entry only after the DB op succeeds.
        // If the old name has no entry (e.g. table created outside register_file /
        // create_table), do nothing — don't fabricate an entry for the new name.
        let mut origins = self.table_origins.write().expect("table_origins poisoned");
        if let Some(origin) = origins.remove(&old) {
            origins.insert(new, origin);
        }
        Ok(())
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
        let origins = self
            .table_origins
            .read()
            .expect("table_origins poisoned")
            .clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<crate::types::TableInfo>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::catalog::get_tables(&conn, &origins)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(name = name))]
    async fn profile_table(
        &self,
        name: &str,
        schema: Option<&str>,
    ) -> Result<crate::profile::TableProfile> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let target = match schema {
            Some(s) => format!("{}.{}", quote_ident(s), quote_ident(name)),
            None => quote_ident(name),
        };
        tokio::task::spawn_blocking(move || -> Result<crate::profile::TableProfile> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::profile::profile_blocking(&conn, &target)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(sql_len = sql.len()))]
    async fn profile_query(&self, sql: &str) -> Result<crate::profile::TableProfile> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let target = format!("({sql})");
        tokio::task::spawn_blocking(move || -> Result<crate::profile::TableProfile> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::profile::profile_blocking(&conn, &target)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(table = table, col = col, n))]
    async fn column_topn(&self, table: &str, col: &str, n: u64) -> Result<Vec<(String, u64)>> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let table = table.to_string();
        let col = col.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::profile::topn_blocking(&conn, &table, &col, n)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(table = table, col = col))]
    async fn column_length_stats(
        &self,
        table: &str,
        col: &str,
    ) -> Result<crate::profile::LengthStats> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let table = table.to_string();
        let col = col.to_string();
        tokio::task::spawn_blocking(move || -> Result<crate::profile::LengthStats> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::profile::length_blocking(&conn, &table, &col)
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

    #[instrument(skip(self, select_sql), fields(format = ?format, sql_len = select_sql.len()))]
    async fn export_query_to_path(
        &self,
        select_sql: &str,
        format: crate::types::ExportFormat,
        dest: &std::path::Path,
    ) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let select_sql = select_sql.to_owned();
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            crate::export::export_query_to_path(&conn, &select_sql, format, &dest)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    // `opts` is deliberately excluded from the span: AttachOpts carries the raw
    // MotherDuck token. Skipping it (rather than relying on the redacted Debug
    // impl) makes the no-log guarantee structural, not a property that could
    // silently regress if Debug changes.
    #[instrument(skip(self, opts), fields(dsn_scheme = ?dsn.split(':').next(), alias))]
    async fn attach(&self, dsn: &str, alias: &str, opts: crate::types::AttachOpts) -> Result<()> {
        self.assert_open()?;
        let (scheme, rest) = crate::attach::parse_scheme(dsn)?;
        match scheme {
            crate::attach::AttachScheme::MotherDuck => {
                let Some(_) = opts.token.as_ref() else {
                    return Err(EngineError::MotherDuckAuth);
                };
                // Lazy, memoized global extension install (design D8). Use a
                // throwaway temp scratch DB — NOT the live engine db file
                // (`self.scratch_path`), which already has an open connection;
                // opening it twice would contend on the DuckDB file lock.
                let bootstrap_scratch = std::env::temp_dir()
                    .join(format!("dat0-md-bootstrap-{}.duckdb", std::process::id()));
                crate::extension_bootstrap::install_motherduck_at_app_boot(bootstrap_scratch)?;
                // `install_*` only LOADs on its throwaway connection; the live
                // connection must LOAD it too (mirrors init()'s `LOAD
                // sqlite_scanner`). The SET+ATTACH `sql` below contains the raw
                // token — never log it or fold it into an error message. The
                // `alias` param is intentionally unused for `md:`: MotherDuck
                // workspace mode attaches the account's databases under their
                // real names and rejects `AS <alias>` for owned databases.
                let _ = alias;
                let sql = crate::attach::build_attach_md_sql(&opts);
                let conn = self.conn.clone();
                // D-012: enumerate the account's databases (those with
                // `lower(type)='motherduck'` in `duckdb_databases()`) and record
                // an `Attached` origin per table/view, keyed by the REAL db name
                // (workspace mode has no alias). Returns the `(real_db, table)`
                // pairs from the blocking task; the origin-map write happens after
                // the await. This runs on BOTH the fresh ATTACH and the idempotent
                // `already_attached` short-circuit so reconnect re-populates.
                // Best-effort: an enumeration hiccup must NOT fail an attach that
                // already succeeded, so enumeration errors collapse to an empty
                // list rather than propagating.
                let pairs = tokio::task::spawn_blocking(move || -> Result<Vec<(String, String)>> {
                    let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
                    conn.execute_batch("LOAD motherduck;")
                        .map_err(|_| EngineError::ExtensionLoad { name: "motherduck" })?;
                    // Idempotent: if MotherDuck is already attached in this
                    // session (e.g. reconnect after a soft disconnect, or a
                    // second connect), skip the ATTACH — re-running `ATTACH 'md:'`
                    // errors, and we must NOT DETACH first (workspace-mode DETACH
                    // persists to the account's saved workspace).
                    let already_attached: bool = conn
                        .query_row(
                            "SELECT count(*) FROM duckdb_databases() WHERE lower(type) = 'motherduck'",
                            [],
                            |r| r.get::<_, i64>(0),
                        )
                        .map(|n| n > 0)
                        .unwrap_or(false);
                    if !already_attached {
                        // `ATTACH 'md:'` (workspace mode) can switch the current
                        // database to a MotherDuck db; capture + restore the local
                        // one so the engine's scratch tables stay reachable unqualified.
                        let current_db: Option<String> = conn
                            .query_row("SELECT current_database()", [], |r| r.get(0))
                            .ok();
                        conn.execute_batch(&sql).map_err(|_| {
                            // A failed ATTACH after a good extension load is almost
                            // always a bad/expired token; surface as auth. The error
                            // is dropped (not logged) because it can echo token-
                            // adjacent text.
                            EngineError::MotherDuckAuth
                        })?;
                        if let Some(db) = current_db {
                            let _ = conn.execute_batch(&format!("USE {};", quote_ident(&db)));
                        }
                    }
                    // Enumerate the attached MotherDuck catalogs by real db name.
                    let md_dbs: Vec<String> = (|| -> Result<Vec<String>> {
                        let mut stmt = conn.prepare(
                            "SELECT database_name FROM duckdb_databases() \
                             WHERE lower(type) = 'motherduck'",
                        )?;
                        let v = stmt
                            .query_map([], |r| r.get::<_, String>(0))?
                            .filter_map(std::result::Result::ok)
                            .collect();
                        Ok(v)
                    })()
                    .unwrap_or_default();
                    let mut pairs = Vec::new();
                    for db in md_dbs {
                        if let Ok(rows) = crate::catalog::list_attached_tables(&conn, &db) {
                            for (_schema, table) in rows {
                                pairs.push((db.clone(), table));
                            }
                        }
                    }
                    Ok(pairs)
                })
                .await
                .map_err(|e| EngineError::TaskJoin(e.to_string()))??;
                let mut origins = self.table_origins.write().expect("table_origins poisoned");
                for (db, table) in pairs {
                    origins.insert(
                        table,
                        TableOrigin::Attached {
                            alias: db,
                            source: dsn.to_owned(),
                        },
                    );
                }
                return Ok(());
            }
            crate::attach::AttachScheme::Sqlite => {}
        }
        let sql = crate::attach::build_attach_sqlite_sql(rest, alias, &opts);
        let conn = self.conn.clone();
        // D-012: after a successful ATTACH, enumerate the attached catalog's
        // tables/views (`database_name = <alias>`) and record an
        // `Attached { alias, source }` origin per object. We do the enumeration
        // inside the SAME spawn_blocking that ran the ATTACH (the connection is
        // already held there) and return the names; the map write happens after
        // the await, off the blocking lock.
        let alias_owned = alias.to_owned();
        let source = dsn.to_owned();
        let names = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            conn.execute_batch(&sql)?;
            let names = crate::catalog::list_attached_tables(&conn, &alias_owned)?
                .into_iter()
                .map(|(_schema, table)| table)
                .collect();
            Ok(names)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;
        // Bare-name keying matches the existing `table_origins` convention
        // (last-writer-wins): a local and attached table sharing a name collide.
        // Qualified-key resolution is out of P6a scope.
        let mut origins = self.table_origins.write().expect("table_origins poisoned");
        for name in names {
            origins.insert(
                name,
                TableOrigin::Attached {
                    alias: alias.to_owned(),
                    source: source.clone(),
                },
            );
        }
        Ok(())
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
        .map_err(|e| EngineError::TaskJoin(e.to_string()))??;
        // D-012: prune the origins this alias contributed. Keyed by bare table
        // name (matching the existing convention), so we match on the recorded
        // Attached.alias rather than the table name.
        self.table_origins
            .write()
            .expect("table_origins poisoned")
            .retain(|_, o| {
                !matches!(o, crate::types::TableOrigin::Attached { alias: a, .. } if a == alias)
            });
        Ok(())
    }

    #[instrument(skip(self), fields(table = table))]
    async fn ensure_rowid(&self, table: &str) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        // Raw (unquoted) name for catalog lookups; quoted form for DDL.
        let raw = table.to_owned();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            ensure_rowid_blocking(&conn, &raw)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self, sql), fields(name = name, sql_len = sql.len()))]
    async fn create_or_replace_view(&self, name: &str, sql: &str) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        let sql = sql.to_owned();
        let stmt = format!(
            "CREATE OR REPLACE TEMP VIEW {} AS {}",
            quote_ident(&name),
            sql
        );
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            conn.execute_batch(&stmt)?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    #[instrument(skip(self), fields(name = name))]
    async fn drop_view(&self, name: &str) -> Result<()> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let name = name.to_owned();
        let stmt = format!("DROP VIEW IF EXISTS {}", quote_ident(&name));
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            conn.execute_batch(&stmt)?;
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

    /// Return the recorded `TableOrigin` for `name`, or `None` if not tracked.
    ///
    /// T7 passes the origins map directly to `catalog::get_tables` rather than
    /// going through this accessor. Available for T8/T9 call sites and tests.
    pub fn table_origin(&self, name: &str) -> Option<TableOrigin> {
        self.table_origins
            .read()
            .expect("table_origins poisoned")
            .get(name)
            .cloned()
    }
}

/// Synchronous core of `ensure_rowid`, run inside `spawn_blocking`.
///
/// Idempotent. Disambiguates our injected key from a colliding user source
/// column via a `dat0:surrogate` column COMMENT:
///   - column present + marked  → no-op (already injected by us)
///   - column present + unmarked → rename source col to `<ROWID_COL>__src`,
///     then inject our surrogate
///   - column absent             → inject our surrogate
///
/// `table` is the raw (unquoted) identifier.
fn ensure_rowid_blocking(conn: &duckdb::Connection, table: &str) -> Result<()> {
    let qt = quote_ident(table);

    if column_exists(conn, table, crate::ROWID_COL)? {
        if is_marked_rowid(conn, table)? {
            return Ok(()); // already our surrogate — idempotent no-op
        }
        // A user's source column collides with our sentinel name. Move it out of
        // the way (preserving their data) before injecting our key.
        conn.execute_batch(&format!(
            "ALTER TABLE {qt} RENAME COLUMN {rowid} TO {rowid}__src;",
            qt = qt,
            rowid = crate::ROWID_COL,
        ))?;
    }

    // Inject the deterministic surrogate in physical scan order. This is the
    // T0-probe-verified SQL (docs/internal/dat0-p4b-t0-probe.md §3): gap-free
    // 0..n-1, keyed off `rowid` (scan order), stable across reads. The
    // `dat0:surrogate` COMMENT marks the column so future calls are no-ops.
    conn.execute_batch(&format!(
        "ALTER TABLE {qt} ADD COLUMN {rowid} BIGINT;\n\
         UPDATE {qt} SET {rowid} = seq.rn FROM (\n\
           SELECT rowid AS rid, (row_number() OVER (ORDER BY rowid)) - 1 AS rn FROM {qt}\n\
         ) seq WHERE {qt}.rowid = seq.rid;\n\
         COMMENT ON COLUMN {qt}.{rowid} IS 'dat0:surrogate';",
        qt = qt,
        rowid = crate::ROWID_COL,
    ))?;
    Ok(())
}

/// True if `table` has a column named `column`. Queries `duckdb_columns()`
/// (the DuckDB-native catalog table function) filtered to non-internal objects.
fn column_exists(conn: &duckdb::Connection, table: &str, column: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM duckdb_columns() \
         WHERE NOT internal AND schema_name = 'main' AND table_name = ?1 AND column_name = ?2",
        duckdb::params![table, column],
        |r| r.get::<_, i64>(0),
    )?;
    Ok(count > 0)
}

/// True if `table`'s `__dat0_rowid` column carries our `dat0:surrogate` COMMENT
/// marker (i.e. it was injected by us, not a colliding user column).
fn is_marked_rowid(conn: &duckdb::Connection, table: &str) -> Result<bool> {
    let comment: Option<String> = conn.query_row(
        "SELECT comment FROM duckdb_columns() \
         WHERE NOT internal AND schema_name = 'main' AND table_name = ?1 AND column_name = ?2",
        duckdb::params![table, crate::ROWID_COL],
        |r| r.get::<_, Option<String>>(0),
    )?;
    Ok(comment.as_deref() == Some("dat0:surrogate"))
}

/// Integration-test hooks. `#[doc(hidden)] pub` (not `#[cfg(test)]`) so the
/// crate's `tests/*.rs` integration suites — which compile against the crate as
/// an external dependency, where `#[cfg(test)]` items are absent — can read raw
/// SQL off the pooled connection. Mirrors the `__test_only_*` convention in
/// `crate::migrations`. Not part of the stable API.
#[doc(hidden)]
impl DuckDBEngine {
    /// Test hook: run a raw SQL batch on the pooled connection.
    pub async fn __test_execute_batch(&self, sql: &str) -> Result<()> {
        let conn = self.conn.clone();
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            conn.execute_batch(&sql)?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    /// Test hook: read a single BIGINT column into a `Vec<i64>`.
    pub async fn __test_query_i64_col(&self, sql: &str) -> Result<Vec<i64>> {
        let conn = self.conn.clone();
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || -> Result<Vec<i64>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            let mut stmt = conn.prepare(&sql)?;
            let vals: Vec<i64> = stmt
                .query_map([], |r| r.get::<_, i64>(0))?
                .filter_map(std::result::Result::ok)
                .collect();
            Ok(vals)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }

    /// Test hook: list a table's column names via `duckdb_columns()`.
    pub async fn __test_column_names(&self, table: &str) -> Result<Vec<String>> {
        let conn = self.conn.clone();
        let table = table.to_owned();
        tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            let mut stmt = conn.prepare(
                "SELECT column_name FROM duckdb_columns() \
                 WHERE NOT internal AND schema_name = 'main' AND table_name = ?1 ORDER BY column_index",
            )?;
            let names: Vec<String> = stmt
                .query_map(duckdb::params![table], |r| r.get::<_, String>(0))?
                .filter_map(std::result::Result::ok)
                .collect();
            Ok(names)
        })
        .await
        .map_err(|e| EngineError::TaskJoin(e.to_string()))?
    }
}
