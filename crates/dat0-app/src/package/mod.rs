//! P8 T4: app-side glue between a live [`Session`] and the portable
//! [`dat0_format::PackageContents`] model.
//!
//! Two directions:
//! - [`session_to_contents`] (EXPORT): walks the engine catalog + session
//!   tabs/saved-queries into a [`PackageContents`] the writer can serialize.
//! - [`contents_to_workspace`] (UNPACK): materializes a parsed package into a
//!   fresh `.dat0/` workspace on disk (parquet → concrete tables + manifest +
//!   session.json) so [`Session::recover_workspace`] can open it.
//!
//! Internal surrogate columns (`__dat0_*`, e.g. `__dat0_rowid`) are never part
//! of the portable schema fingerprint, and are stripped on re-materialize so a
//! fresh, clean surrogate is injected by [`QueryEngine::ensure_rowid`] post-unpack.

use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use dat0_engine::{
    ColumnInfo, DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, TableInfo, TableOrigin,
    quote_ident,
};
use dat0_format::{
    ColumnFingerprint, Derivation, PackageContents, PackageQuery, PackageSource, PackageView,
    ParsedPackage, Queries, Recipe, RecipeTable, Sources, TableKind, Views,
};

use crate::session::Session;
use crate::session::queries::SavedQuery;
use crate::session::{SESSION_SCHEMA_VERSION, SessionState, Tab};

/// `true` if `name` is an internal dat0 surrogate (e.g. `__dat0_rowid`,
/// `__dat0_meta*`) that must not surface in the portable package. Scoped to the
/// `__dat0` reserved prefix so a legitimate user column/table named `__foo` is
/// preserved (matches the engine's `ROWID_COL` = `__dat0_rowid` convention).
fn is_internal(name: &str) -> bool {
    name.starts_with("__dat0")
}

/// Portable column fingerprints for a table, with internal surrogate columns
/// stripped (the package schema is the user-facing shape only).
fn fingerprint_schema(columns: &[ColumnInfo]) -> Vec<ColumnFingerprint> {
    columns
        .iter()
        .filter(|c| !is_internal(&c.name))
        .map(|c| ColumnFingerprint {
            name: c.name.clone(),
            r#type: c.data_type.clone(),
        })
        .collect()
}

/// Exact `count(*)` of a (freshly materialized) table. Read via the engine's
/// scalar path and downcast the single Int64 cell — the same pattern the grid /
/// inspector / `workspace_promote.rs` use. Falls back to `0` only if the result
/// shape is unexpectedly empty (a materialized table always yields one row).
async fn count_rows(engine: &dyn QueryEngine, name: &str) -> Result<u64> {
    use duckdb::arrow::array::{Array, Int64Array};
    let sql = format!("SELECT count(*) FROM {}", quote_ident(name));
    let result = engine
        .execute(&sql)
        .await
        .with_context(|| format!("count_rows: count(*) on {name}"))?;
    let n = result
        .batches
        .first()
        .and_then(|b| b.column(0).as_any().downcast_ref::<Int64Array>())
        .map(|a| a.value(0))
        .unwrap_or(0);
    Ok(n.max(0) as u64)
}

/// Build a [`PackageSource`] for a `File`-origin base table. `content_hash` is
/// the sha256 of the source file bytes when readable, else an empty marker
/// (informational only — the package carries the materialized parquet, not the
/// original file).
fn make_source(
    name: &str,
    path: &Path,
    schema: &[ColumnFingerprint],
    row_count: u64,
) -> PackageSource {
    let logical_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name)
        .to_string();
    let content_hash = match std::fs::read(path) {
        Ok(bytes) => format!("sha256:{:x}", Sha256::digest(&bytes)),
        Err(_) => String::new(),
    };
    PackageSource {
        id: format!("src_{name}"),
        logical_name,
        original_uri: path.to_string_lossy().into_owned(),
        schema_fingerprint: schema.to_vec(),
        content_hash,
        row_count,
    }
}

/// EXPORT: map a live [`Session`] to portable [`PackageContents`].
///
/// - Each non-internal engine table becomes a [`RecipeTable`]; its `origin`
///   selects `Base` vs `Derived` and (for `File` origins) emits a
///   [`PackageSource`].
/// - Session tabs become [`PackageView`]s; saved queries become
///   [`PackageQuery`]s.
///
/// The writer pulls the actual data bytes per `RecipeTable.name` (a plain
/// `SELECT *` parquet export), so this function only produces the metadata
/// recipe + the portable session state.
pub async fn session_to_contents(sess: &Session) -> Result<PackageContents> {
    let engine = sess.engine.as_ref();
    let tables = engine
        .get_tables()
        .await
        .context("session_to_contents: get_tables")?;

    let mut recipe_tables = Vec::new();
    let mut sources = Vec::new();

    for t in &tables {
        // Skip internal surrogate tables (defensive — get_tables already filters
        // __dat0_meta%, but a future surrogate table would be excluded here too).
        if is_internal(&t.name) {
            continue;
        }

        let schema = fingerprint_schema(&t.columns);
        let row_count = count_rows(engine, &t.name).await?;
        let data = format!("data/{}.parquet", t.name);

        let (kind, source_ref, derivation) = classify(engine, t, &schema, row_count, &mut sources)
            .await
            .with_context(|| format!("session_to_contents: classify {}", t.name))?;

        recipe_tables.push(RecipeTable {
            id: format!("t_{}", t.name),
            name: t.name.clone(),
            kind,
            schema,
            row_count,
            data,
            source_ref,
            derivation,
        });
    }

    let views = sess
        .tabs()
        .iter()
        .map(|tab| PackageView {
            table_name: tab.table_name.clone(),
            transform_stack: tab.transform_stack.clone(),
            undo_cursor: tab.undo_cursor,
        })
        .collect();

    let queries = sess
        .saved_queries()
        .iter()
        .map(|q| PackageQuery {
            id: q.id,
            name: q.name.clone(),
            sql: q.sql.clone(),
            saved_at: q.saved_at,
        })
        .collect();

    Ok(PackageContents {
        workspace_id: sess.window_id,
        created_at: crate::window::now_epoch_secs(),
        recipe: Recipe {
            tables: recipe_tables,
        },
        sources: Sources { sources },
        views: Views { views },
        queries: Queries { queries },
    })
}

/// Classify a table's [`TableOrigin`] into the recipe `(kind, source_ref,
/// derivation)` triple, pushing a [`PackageSource`] for `File` origins.
async fn classify(
    engine: &dyn QueryEngine,
    t: &TableInfo,
    schema: &[ColumnFingerprint],
    row_count: u64,
    sources: &mut Vec<PackageSource>,
) -> Result<(TableKind, Option<String>, Option<Derivation>)> {
    match &t.origin {
        TableOrigin::File(path) => {
            let src = make_source(&t.name, path, schema, row_count);
            let id = src.id.clone();
            sources.push(src);
            Ok((TableKind::Base, Some(id), None))
        }
        TableOrigin::Attached { .. } => {
            // An attached (external) table — base, but no portable source file
            // and no derivation. Its data is still materialized into parquet.
            Ok((TableKind::Base, None, None))
        }
        TableOrigin::Derived(DerivedOrigin::Sql(sql)) if sql.trim().is_empty() => {
            // Engine sentinel: `get_tables()` falls back to `Derived(Sql(""))`
            // for any table NOT recorded in its `table_origins` map — e.g. a
            // table created by a RAW `execute("CREATE TABLE … AS …")` (which
            // never populates that map). An empty-SQL "derivation" carries no
            // replayable provenance, so from the package's perspective this is a
            // concrete Base table (its data is materialized into parquet either
            // way). Classifying it Derived would emit a useless `sql: ""`
            // derivation that replay could not re-run.
            Ok((TableKind::Base, None, None))
        }
        TableOrigin::Derived(DerivedOrigin::Sql(sql)) => {
            let parents = engine.referenced_tables(sql).await.unwrap_or_default();
            Ok((
                TableKind::Derived,
                None,
                Some(Derivation::Sql {
                    sql: sql.clone(),
                    parents,
                }),
            ))
        }
        TableOrigin::Derived(DerivedOrigin::Transform { parent, ops }) => Ok((
            TableKind::Derived,
            None,
            Some(Derivation::Transform {
                parent: parent.clone(),
                ops: ops.clone(),
            }),
        )),
    }
}

/// UNPACK: materialize a [`ParsedPackage`] into a fresh `.dat0/` workspace under
/// `dir`, so [`Session::recover_workspace`] can open it.
///
/// Steps:
/// 1. Create `<dir>/.dat0/`.
/// 2. Extract the package's `data/*.parquet` into `<dir>/data/`.
/// 3. Open a THROWAWAY engine on `<dir>/.dat0/workspace.duckdb` and materialize
///    every recipe table as a concrete `CREATE TABLE … AS SELECT … FROM
///    read_parquet(...)` (user-facing columns only — internal surrogates are
///    stripped by listing the recipe schema explicitly, so a clean surrogate is
///    re-injected by `ensure_rowid`).
/// 4. Inject a fresh `__dat0_rowid` surrogate on each table (`ensure_rowid`).
/// 5. Write `manifest.json` + `session.json` (tabs from views, saved queries).
/// 6. **CRITICAL (P7a T6):** `close()` AND fully DROP the throwaway engine
///    before returning, or the moved-WAL data is invisible to the caller's
///    reopen (silent-empty-db).
pub async fn contents_to_workspace(parsed: &ParsedPackage, dir: &Path, budget: u64) -> Result<()> {
    let dat0 = crate::workspace::Home::dat0_dir_for(dir);
    std::fs::create_dir_all(&dat0)
        .with_context(|| format!("contents_to_workspace: mkdir {}", dat0.display()))?;

    // Extract parquet payloads into <dir>/data/.
    parsed
        .extract_data_to(dir)
        .context("contents_to_workspace: extract data")?;

    // Materialize the tables in a scoped block so the engine is dropped (not
    // merely closed) before `recover_workspace` reopens the same DB file.
    materialize_tables(parsed, dir, &dat0, budget).await?;

    // Workspace identity manifest.
    let manifest = crate::workspace::manifest::Manifest::new(crate::window::now_epoch_secs());
    crate::workspace::manifest::write(&dat0.join("manifest.json"), &manifest)
        .context("contents_to_workspace: write manifest")?;

    // Reconstruct session.json (schema v8) from the package views + queries.
    write_session_json(parsed, &dat0).context("contents_to_workspace: write session.json")?;

    Ok(())
}

/// Open a throwaway engine, materialize every recipe table from its parquet,
/// inject fresh surrogates, then CLOSE + DROP the engine before returning so its
/// WAL is flushed and visible to a subsequent reopen (P7a T6 finding).
async fn materialize_tables(
    parsed: &ParsedPackage,
    dir: &Path,
    dat0: &Path,
    budget: u64,
) -> Result<()> {
    let db_path = dat0.join("workspace.duckdb");
    let engine = DuckDBEngine::new(db_path, MemoryBudget { bytes: budget })
        .context("contents_to_workspace: open throwaway engine")?;
    engine
        .init()
        .await
        .context("contents_to_workspace: init throwaway engine")?;

    for t in &parsed.recipe.tables {
        // Absolute path to the extracted parquet for this table.
        let parquet = dir.join("data").join(format!("{}.parquet", t.name));
        let parquet_str = parquet.to_string_lossy();

        // Explicit user-facing column list (surrogates already excluded from the
        // recipe schema). An empty schema is degenerate (a table with no
        // user columns) — fall back to `*` so we never emit `SELECT  FROM`.
        let projection = if t.schema.is_empty() {
            "*".to_string()
        } else {
            t.schema
                .iter()
                .map(|c| quote_ident(&c.name))
                .collect::<Vec<_>>()
                .join(", ")
        };

        let create_sql = format!(
            "CREATE TABLE {tbl} AS SELECT {proj} FROM read_parquet('{path}')",
            tbl = quote_ident(&t.name),
            proj = projection,
            // read_parquet takes a single-quoted string literal; escape any
            // embedded single quote (rare in temp paths, but be safe).
            path = parquet_str.replace('\'', "''"),
        );
        engine
            .execute(&create_sql)
            .await
            .with_context(|| format!("materialize {}: CREATE TABLE AS read_parquet", t.name))?;

        // Inject a fresh, clean surrogate so the post-unpack edit-overlay path
        // works (mirrors register_file_as_table). Idempotent.
        engine
            .ensure_rowid(&t.name)
            .await
            .with_context(|| format!("materialize {}: ensure_rowid", t.name))?;
    }

    // CRITICAL: close THEN drop so the moved WAL is flushed and a reopen sees
    // the data (P7a T6: a still-alive connection makes a fresh connection read
    // an EMPTY db). `close()` only flags; the drop at end-of-fn releases.
    engine
        .close()
        .await
        .context("contents_to_workspace: close throwaway engine")?;
    drop(engine);
    Ok(())
}

/// Build + write `<dat0>/session.json` (schema v8) from the package's portable
/// views (→ tabs) and queries (→ saved queries), matching the on-disk shape
/// [`Session::persist`] writes.
fn write_session_json(parsed: &ParsedPackage, dat0: &Path) -> Result<()> {
    let tabs: Vec<Tab> = parsed
        .views
        .views
        .iter()
        .map(|v| Tab {
            table_name: v.table_name.clone(),
            source_path: None,
            transform_stack: v.transform_stack.clone(),
            undo_cursor: v.undo_cursor,
            extra: Default::default(),
        })
        .collect();

    let active_tab = if tabs.is_empty() { None } else { Some(0) };

    let saved_queries: Vec<SavedQuery> = parsed
        .queries
        .queries
        .iter()
        .map(|q| SavedQuery {
            id: q.id,
            name: q.name.clone(),
            sql: q.sql.clone(),
            saved_at: q.saved_at,
        })
        .collect();

    let state = SessionState {
        schema_version: SESSION_SCHEMA_VERSION,
        tabs,
        active_tab,
        saved_queries,
        ..Default::default()
    };

    let bytes = serde_json::to_vec_pretty(&state).context("serialize session.json")?;
    std::fs::write(dat0.join("session.json"), &bytes)
        .with_context(|| format!("write {}", dat0.join("session.json").display()))?;
    Ok(())
}
