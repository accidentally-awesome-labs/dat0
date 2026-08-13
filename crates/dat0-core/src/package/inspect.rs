//! P8 T9: read-only "Inspect" open path for a `.dat0` package.
//!
//! [`open_readonly`] is the headless, GPUI-free engine core behind both the GUI
//! "Open Package" action and the CLI `inspect` UX: it extracts the package's
//! cached parquet payloads into a scratch directory and registers each recipe
//! table as a `read_parquet(...)` **view** (non-mutable). The returned engine
//! therefore answers `SELECT`s against every table by reading the parquet on
//! disk, while any `INSERT`/`UPDATE`/`DELETE`/DDL against those names errors
//! (DuckDB does not allow DML on a view) — a hard, engine-level read-only
//! guarantee independent of the app's `read_only` shell gate.
//!
//! No `__dat0_rowid` surrogate is injected (read-only: there are no edits to
//! address), and no concrete tables are materialized — the data lives only in
//! the extracted parquet, which the caller MUST keep alive for the engine's
//! lifetime (the views read it lazily on every query).

use std::path::Path;

use anyhow::{Context, Result};

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

/// Open a package read-only into a fresh engine.
///
/// Extracts the package's `data/*.parquet` into `<scratch_dir>/data/`, opens a
/// fresh [`DuckDBEngine`] at `<scratch_dir>/inspect.duckdb`, and registers every
/// [`dat0_format::RecipeTable`] as a `read_parquet(...)` view named after the
/// table. Returns the live engine plus the list of registered view names (recipe
/// order).
///
/// The views are non-mutable: a `SELECT` reads the parquet, but any DML/DDL
/// against the name errors. `<scratch_dir>` must outlive the returned engine —
/// the views read the extracted parquet on every query.
///
/// On any failure the engine is closed best-effort before the error propagates
/// (so a half-built inspect engine never leaks an open DB handle).
pub async fn open_readonly(
    parsed: &dat0_format::ParsedPackage,
    scratch_dir: &Path,
    budget: u64,
) -> Result<(DuckDBEngine, Vec<String>)> {
    // 1. Materialize the parquet payloads under <scratch_dir>/data/.
    parsed
        .extract_data_to(scratch_dir)
        .context("open_readonly: extract package data")?;

    // 2. Fresh engine bound to a throwaway DB beside the extracted data.
    let engine = DuckDBEngine::new(
        scratch_dir.join("inspect.duckdb"),
        MemoryBudget { bytes: budget },
    )
    .context("open_readonly: DuckDBEngine::new")?;
    engine
        .init()
        .await
        .context("open_readonly: engine.init()")?;

    // 3. Register each recipe table as a read_parquet view (non-mutable). On any
    //    failure, close the engine before returning so no DB handle leaks.
    let mut view_names = Vec::with_capacity(parsed.recipe.tables.len());
    for t in &parsed.recipe.tables {
        let parquet = scratch_dir.join("data").join(format!("{}.parquet", t.name));
        // read_parquet takes a single-quoted string literal — escape embedded
        // quotes in the (temp) path. The engine quotes the VIEW name itself, so
        // we pass `t.name` plain.
        let parquet_str = parquet.to_string_lossy();
        let sql = format!(
            "SELECT * FROM read_parquet('{}')",
            parquet_str.replace('\'', "''")
        );
        if let Err(e) = engine.create_or_replace_view(&t.name, &sql).await {
            engine.close().await.ok();
            return Err(e).with_context(|| {
                format!("open_readonly: register read_parquet view for {}", t.name)
            });
        }
        view_names.push(t.name.clone());
    }

    Ok((engine, view_names))
}
