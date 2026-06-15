//! ReplayEngine (T6) — re-run a package's recipe against fresh source files.
//!
//! A `.dat0` package carries a portable *recipe*: base tables that map to
//! [`PackageSource`]s plus derived tables that carry a [`Derivation`] (stored
//! SQL or a projection `Transform`). Replay rebinds each source to a NEW file,
//! structurally checks the new schema is compatible, then re-executes the
//! derived tables in topological order against the fresh data — yielding a new
//! [`PackageContents`] with the same recipe *shape* but refreshed row counts
//! and schemas. The caller (T7 `dat0 replay`) writes the result via [`Writer`].
//!
//! This addresses deferral D-023 (replay-on-new-source).
//!
//! Two responsibilities, kept narrow:
//! - [`compat_check`] — pure structural compatibility (no engine), so its unit
//!   tests don't need a database.
//! - [`ReplayEngine::replay`] — the engine-backed re-execution.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use dat0_engine::{ColumnInfo, DerivedOrigin, QueryEngine, RegisterOpts, compile_view_sql};

use crate::error::{FormatError, Result};
use crate::model::{
    Charts, ColumnFingerprint, Derivation, PackageContents, ParsedPackage, RecipeTable, TableKind,
};

/// `true` if `name` is an internal dat0 surrogate (e.g. `__dat0_rowid`) that
/// must not surface in a portable fingerprint. Mirrors the app's
/// `package::is_internal` and the engine's `ROWID_COL` = `__dat0_rowid`.
fn is_internal(name: &str) -> bool {
    name.starts_with("__dat0")
}

/// Portable column fingerprints for a freshly described table, with internal
/// surrogate columns stripped (the recipe schema is the user-facing shape).
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

/// Normalize a DuckDB type literal for family comparison: uppercased, with any
/// parenthesised precision/scale suffix dropped (`DECIMAL(18,4)` → `DECIMAL`).
fn type_base(t: &str) -> String {
    let upper = t.trim().to_ascii_uppercase();
    match upper.split_once('(') {
        Some((head, _)) => head.trim().to_string(),
        None => upper,
    }
}

/// Structural type compatibility between a *needed* type `a` (what the recipe
/// recorded) and a *provided* type `b` (what the new source actually has).
///
/// Conservative, name-family based (v1):
/// - exact literal match (case/whitespace-insensitive);
/// - the signed integer-width family (`TINYINT`/`SMALLINT`/`INTEGER`/`BIGINT`/
///   `HUGEINT`, with the `INT*` aliases) is mutually compatible — a wider
///   provided column losslessly holds the needed values;
/// - any `DECIMAL`/`NUMERIC` is compatible with any other `DECIMAL`/`NUMERIC`
///   (precision/scale relaxation — DuckDB casts between them);
/// - the string family (`VARCHAR`/`TEXT`/`STRING`/`CHAR`/`BPCHAR`) is one class.
///
/// Anything else requires the (normalized) base types to be equal.
fn type_compatible(a: &str, b: &str) -> bool {
    let (ba, bb) = (type_base(a), type_base(b));
    if ba == bb {
        return true;
    }
    const INT_FAMILY: &[&str] = &[
        "TINYINT", "SMALLINT", "INTEGER", "BIGINT", "HUGEINT", "INT", "INT1", "INT2", "INT4",
        "INT8", "SHORT", "LONG",
    ];
    const DECIMAL_FAMILY: &[&str] = &["DECIMAL", "NUMERIC"];
    const STRING_FAMILY: &[&str] = &["VARCHAR", "TEXT", "STRING", "CHAR", "BPCHAR"];

    let same_family = |fam: &[&str]| fam.contains(&ba.as_str()) && fam.contains(&bb.as_str());
    same_family(INT_FAMILY) || same_family(DECIMAL_FAMILY) || same_family(STRING_FAMILY)
}

/// Structural compatibility check: every `needed` column must be present in
/// `provided` (matched by name), and the matched pair's types must be
/// [`type_compatible`]. Extra `provided` columns are ignored.
///
/// On failure returns [`FormatError::SchemaIncompatible`] whose message lists
/// the missing columns and/or the type mismatches, e.g.
/// `"missing: foo; type mismatch: id BIGINT vs VARCHAR"`.
///
/// Pure (no engine) so its unit tests are cheap.
///
/// v1 tightening follow-up: the caller currently passes the FULL base
/// `schema_fingerprint` as `needed`. Ideally only the columns actually
/// referenced by downstream derivations would be required, so an unrelated
/// dropped/retyped column wouldn't block a replay whose derivations never
/// touch it. Recorded as a known conservative over-requirement.
pub fn compat_check(needed: &[ColumnFingerprint], provided: &[ColumnFingerprint]) -> Result<()> {
    let by_name: HashMap<&str, &str> = provided
        .iter()
        .map(|c| (c.name.as_str(), c.r#type.as_str()))
        .collect();

    let mut missing: Vec<&str> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();

    for n in needed {
        match by_name.get(n.name.as_str()) {
            None => missing.push(n.name.as_str()),
            Some(provided_ty) => {
                if !type_compatible(&n.r#type, provided_ty) {
                    mismatches.push(format!("{} {} vs {}", n.name, n.r#type, provided_ty));
                }
            }
        }
    }

    if missing.is_empty() && mismatches.is_empty() {
        return Ok(());
    }

    let mut parts: Vec<String> = Vec::new();
    if !missing.is_empty() {
        parts.push(format!("missing: {}", missing.join(", ")));
    }
    if !mismatches.is_empty() {
        parts.push(format!("type mismatch: {}", mismatches.join(", ")));
    }
    Err(FormatError::SchemaIncompatible(parts.join("; ")))
}

/// Re-runs a parsed package's recipe against fresh source files.
pub struct ReplayEngine;

impl ReplayEngine {
    /// Replay `parsed`'s recipe against `new_sources` using a fresh `engine`,
    /// returning a refreshed [`PackageContents`] (same recipe *shape*, updated
    /// counts + schemas). The caller persists it via [`Writer`].
    ///
    /// `new_sources` maps each source's `logical_name` to the new file path.
    /// Every source in the package MUST have a replacement.
    ///
    /// Steps:
    /// 1. **Rebind sources** — register each new file, rename the imported
    ///    table to the base [`RecipeTable`]'s name (so derived SQL resolves),
    ///    and [`compat_check`] the new schema against the recorded fingerprint.
    /// 2. **Re-exec derived tables** in topological (Kahn) order.
    /// 3. **Rebuild** a [`PackageContents`] with refreshed schemas + row counts.
    ///
    /// # Errors
    /// - [`FormatError::SchemaIncompatible`] — a source has no replacement, or
    ///   a new source's schema is structurally incompatible, or the recipe DAG
    ///   has a cycle.
    /// - [`FormatError::Engine`] — any engine operation failed.
    pub async fn replay(
        parsed: &ParsedPackage,
        new_sources: &HashMap<String, PathBuf>,
        engine: &dyn QueryEngine,
    ) -> Result<PackageContents> {
        // --- 1. Rebind sources -> base tables. ---
        for source in &parsed.sources.sources {
            let new_path = new_sources.get(&source.logical_name).ok_or_else(|| {
                FormatError::SchemaIncompatible(format!(
                    "no replacement provided for source '{}'",
                    source.logical_name
                ))
            })?;

            // The base RecipeTable that references this source by id.
            let base = parsed
                .recipe
                .tables
                .iter()
                .find(|t| t.source_ref.as_deref() == Some(source.id.as_str()))
                .ok_or_else(|| {
                    FormatError::SchemaIncompatible(format!(
                        "source '{}' is not referenced by any base table",
                        source.id
                    ))
                })?;

            let imported = engine
                .register_file_as_table(new_path, RegisterOpts::default())
                .await?;

            // Derived SQL references the ORIGINAL table names; rename the freshly
            // imported table to match the recipe's base name so they resolve.
            if imported.name != base.name {
                engine
                    .rename_table(&imported.name, &base.name, None)
                    .await?;
            }

            // Fingerprint the (renamed) base table and structurally compare.
            let cols = engine.describe_table(&base.name, None).await?;
            let provided = fingerprint_schema(&cols);
            // v1: conservatively require the FULL recorded base fingerprint
            // (tightening follow-up noted on `compat_check`).
            compat_check(&source.schema_fingerprint, &provided)?;
        }

        // --- 2. Re-exec derived tables in topological order. ---
        for table in topo_order_derived(&parsed.recipe.tables)? {
            // A `Derived` table MUST carry a derivation (Reader does not enforce
            // this invariant, so a hand-corrupted package gets a clean error here
            // rather than a panic).
            let derivation = table.derivation.as_ref().ok_or_else(|| {
                FormatError::SchemaIncompatible(format!(
                    "derived table '{}' has no derivation",
                    table.name
                ))
            })?;
            let (sql, origin) = match derivation {
                Derivation::Sql { sql, .. } => (sql.clone(), DerivedOrigin::Sql(sql.clone())),
                Derivation::Transform { parent, ops } => {
                    let sql = compile_view_sql(&dat0_engine::quote_ident(parent), ops)
                        .map_err(|e| FormatError::SchemaIncompatible(e.to_string()))?;
                    (
                        sql,
                        DerivedOrigin::Transform {
                            parent: parent.clone(),
                            ops: ops.clone(),
                        },
                    )
                }
            };
            // Drop any stale table of this name first so the recreate is
            // idempotent. `drop_table` is a bare `DROP TABLE` (no IF EXISTS), so
            // only drop when it actually exists (a fresh replay engine has none).
            if engine.describe_table(&table.name, None).await.is_ok() {
                engine.drop_table(&table.name, None).await?;
            }
            engine.create_table(&table.name, &sql, origin).await?;
        }

        // --- 3. Rebuild PackageContents with refreshed schemas + counts. ---
        let mut recipe = parsed.recipe.clone();
        for t in &mut recipe.tables {
            let cols = engine.describe_table(&t.name, None).await?;
            t.schema = fingerprint_schema(&cols);
            t.row_count = count_rows(engine, &t.name).await?;
        }

        // Refresh each source's fingerprint + row_count from its (re-registered)
        // base table. Build a base-name lookup from the refreshed recipe.
        let base_by_source: HashMap<&str, &RecipeTable> = recipe
            .tables
            .iter()
            .filter(|t| t.kind == TableKind::Base)
            .filter_map(|t| t.source_ref.as_deref().map(|s| (s, t)))
            .collect();
        let mut sources = parsed.sources.clone();
        for src in &mut sources.sources {
            if let Some(base) = base_by_source.get(src.id.as_str()) {
                src.schema_fingerprint = base.schema.clone();
                src.row_count = base.row_count;
            }
        }

        Ok(PackageContents {
            workspace_id: parsed.manifest.workspace_id,
            // The format crate has no app time helper; reuse the original
            // package's timestamp (the caller may restamp on write).
            created_at: parsed.manifest.created_at.clone(),
            recipe,
            sources,
            views: parsed.views.clone(),
            queries: parsed.queries.clone(),
            // T2 stub: T3 owns chart replay (pass-through of parsed.charts).
            charts: Charts { charts: Vec::new() },
        })
    }
}

/// Exact `count(*)` of a materialized table, via the engine's scalar path.
/// Downcasts the single Int64 cell (the grid/inspector/promote pattern).
async fn count_rows(engine: &dyn QueryEngine, name: &str) -> Result<u64> {
    use duckdb::arrow::array::{Array, Int64Array};
    let sql = format!("SELECT count(*) FROM {}", dat0_engine::quote_ident(name));
    let res = engine.execute(&sql).await?;
    let n = res
        .batches
        .first()
        .and_then(|b| b.column(0).as_any().downcast_ref::<Int64Array>())
        .filter(|a| !a.is_empty())
        .map(|a| a.value(0))
        .unwrap_or(0);
    Ok(n.max(0) as u64)
}

/// The parent table names a derived table's [`Derivation`] references. A free
/// fn (not a closure) so the borrow checker ties the returned `&str`s to the
/// table's lifetime explicitly rather than inferring a too-short one.
fn derived_parents(t: &RecipeTable) -> Vec<&str> {
    match &t.derivation {
        Some(Derivation::Sql { parents, .. }) => parents.iter().map(String::as_str).collect(),
        Some(Derivation::Transform { parent, .. }) => vec![parent.as_str()],
        None => vec![],
    }
}

/// Kahn topological sort over the *derived* tables of a recipe, ordered so a
/// table's parents are created before it. Base tables are roots (already
/// registered) and are excluded from the output. Returns an error on a cycle.
fn topo_order_derived(tables: &[RecipeTable]) -> Result<Vec<&RecipeTable>> {
    // Index derived tables by name. Base tables are pre-satisfied roots.
    let derived: HashMap<&str, &RecipeTable> = tables
        .iter()
        .filter(|t| t.kind == TableKind::Derived)
        .map(|t| (t.name.as_str(), t))
        .collect();

    // Indegree = number of derived parents still pending.
    // children[parent] = derived tables that depend on `parent`.
    let mut indegree: HashMap<&str, usize> = derived.keys().map(|name| (*name, 0usize)).collect();
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for (&name, t) in &derived {
        // A table's parents that are THEMSELVES derived constrain order (parents
        // that are base tables are already satisfied). Resolve each parent name
        // to the `derived` map's key so the `&str` carries the slice lifetime.
        for parent in derived_parents(t) {
            if let Some((&pkey, _)) = derived.get_key_value(parent) {
                *indegree.get_mut(name).expect("name is a derived key") += 1;
                children.entry(pkey).or_default().push(name);
            }
        }
    }

    // Seed the queue with indegree-0 derived tables, in stable recipe order.
    let mut queue: Vec<&str> = tables
        .iter()
        .filter(|t| t.kind == TableKind::Derived)
        .map(|t| t.name.as_str())
        .filter(|n| indegree.get(n).copied() == Some(0))
        .collect();

    let mut ordered: Vec<&RecipeTable> = Vec::with_capacity(derived.len());
    let mut seen: HashSet<&str> = HashSet::new();
    let mut head = 0;
    while head < queue.len() {
        let name = queue[head];
        head += 1;
        if !seen.insert(name) {
            continue;
        }
        ordered.push(derived[name]);
        if let Some(kids) = children.get(name) {
            for &kid in kids {
                if let Some(d) = indegree.get_mut(kid) {
                    *d -= 1;
                    if *d == 0 {
                        queue.push(kid);
                    }
                }
            }
        }
    }

    if ordered.len() != derived.len() {
        return Err(FormatError::SchemaIncompatible(
            "recipe derivation graph has a cycle".into(),
        ));
    }
    Ok(ordered)
}
