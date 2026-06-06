//! Catalog ops: describe, list, create, drop, rename.

use std::collections::HashMap;

use crate::Result;
use crate::types::{ColumnInfo, DerivedOrigin, TableInfo, TableOrigin};

/// Escape a SQL identifier for safe interpolation into a quoted-identifier
/// position. DuckDB doubles `"` to `""` inside `"..."`, the same way SQL
/// string literals double `'` to `''`. Without this, a name containing
/// `"` could break out of the quoted-identifier and inject SQL.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub(crate) fn describe_table(
    conn: &duckdb::Connection,
    name: &str,
    schema: Option<&str>,
) -> Result<Vec<ColumnInfo>> {
    describe_qualified(conn, &qualified_name(name, schema))
}

/// Run `DESCRIBE <fully-qualified>` and map the column rows.
///
/// Shared by the public `describe_table` (default-database, 1- or 2-part name)
/// and by `get_tables`, which qualifies with the catalog/`database_name` so that
/// ATTACHed tables resolve. A `DESCRIBE "main"."items"` for an attached table is
/// a DuckDB Catalog Error ("Table with name items does not exist! Did you mean
/// \"sq.items\"?") because the unqualified 2-part name binds against the DEFAULT
/// database, not the attached catalog — hence the database-aware path below.
fn describe_qualified(conn: &duckdb::Connection, qualified: &str) -> Result<Vec<ColumnInfo>> {
    let mut stmt = conn.prepare(&format!("DESCRIBE {}", qualified))?;
    let cols: Vec<ColumnInfo> = stmt
        .query_map([], |row| {
            Ok(ColumnInfo {
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
    Ok(cols)
}

/// Enumerate `(schema, table)` of an attached catalog by its `database_name`.
///
/// Covers both base tables (`duckdb_tables()`) and views (`duckdb_views()`),
/// excluding internal/temporary objects. Used by `attach` (D-012) to record an
/// `Attached` origin per object; verified against a live SQLite attach where the
/// rows surface with `database_name='<alias>'`, `schema_name='main'`.
pub(crate) fn list_attached_tables(
    conn: &duckdb::Connection,
    database: &str,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT schema_name, table_name FROM duckdb_tables()
         WHERE database_name = ? AND NOT internal AND NOT temporary
         UNION ALL
         SELECT schema_name, view_name FROM duckdb_views()
         WHERE database_name = ? AND NOT internal AND NOT temporary",
    )?;
    let rows = stmt
        .query_map([database, database], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(rows)
}

pub(crate) fn get_tables(
    conn: &duckdb::Connection,
    origins: &HashMap<String, TableOrigin>,
) -> Result<Vec<TableInfo>> {
    // Use DuckDB-native system functions rather than information_schema.tables.
    //
    // Why not information_schema.tables: its SQL definition (visible in
    // duckdb_views()) emits table_type='VIEW' for ALL views — both persistent
    // file-registered views (created by register_file via CREATE OR REPLACE VIEW)
    // and per-tab temp views (created by create_or_replace_view via CREATE OR
    // REPLACE TEMP VIEW). There is no column in information_schema.tables that
    // distinguishes them.
    //
    // Why duckdb_views() works: the table function duckdb_views() exposes a
    // boolean `temporary` column. File-registered views have temporary=false;
    // T13 per-chain views have temporary=true. Filtering NOT temporary cleanly
    // excludes phantom entries before they reach the sidebar.
    //
    // Regression coverage: tests/temp_view_lifecycle.rs ::
    //   get_tables_excludes_temp_views_created_via_create_or_replace_view
    //
    // PD-014 context: T13 calls create_or_replace_view on every chain mutation;
    // without this filter every active tab would inject a phantom sidebar entry.
    //
    // database_name (D-012): we also select `database_name` so attached catalogs
    // (e.g. an ATTACHed SQLite db) are described against the RIGHT database.
    // `describe_table(conn, name, Some("main"))` builds `DESCRIBE "main"."items"`,
    // which binds against the DEFAULT database and errors for an attached table
    // ("Table with name items does not exist! Did you mean \"sq.items\"?"). We
    // therefore qualify DESCRIBE with all three parts: `"db"."schema"."table"`.
    // This is correct for local tables too — the engine's own db carries a real
    // `database_name` and the 3-part name resolves identically. `system`/`temp`
    // are excluded so we never surface DuckDB's internal catalogs.
    let mut stmt = conn.prepare(
        "SELECT database_name, schema_name AS table_schema, table_name
         FROM duckdb_tables()
         WHERE schema_name NOT IN ('information_schema', 'pg_catalog')
           AND database_name NOT IN ('system', 'temp')
           AND NOT internal
           AND NOT temporary
           AND table_name NOT LIKE '__dat0_meta%'
         UNION ALL
         SELECT database_name, schema_name AS table_schema, view_name AS table_name
         FROM duckdb_views()
         WHERE schema_name NOT IN ('information_schema', 'pg_catalog')
           AND database_name NOT IN ('system', 'temp')
           AND NOT internal
           AND NOT temporary
           AND view_name NOT LIKE '__dat0_meta%'",
    )?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .filter_map(std::result::Result::ok)
        .collect();

    let mut tables = Vec::with_capacity(rows.len());
    for (database, schema, name) in rows {
        let qualified = format!(
            "{}.{}.{}",
            quote_ident(&database),
            quote_ident(&schema),
            quote_ident(&name)
        );
        let cols = describe_qualified(conn, &qualified)?;
        let origin = origins
            .get(&name)
            .cloned()
            .unwrap_or(TableOrigin::Derived(DerivedOrigin::Sql(String::new())));
        tables.push(TableInfo {
            name,
            schema,
            columns: cols,
            row_count_estimate: None,
            origin,
        });
    }
    Ok(tables)
}

pub(crate) fn create_table(conn: &duckdb::Connection, name: &str, sql: &str) -> Result<TableInfo> {
    let create_sql = format!("CREATE TABLE {} AS {}", quote_ident(name), sql);
    conn.execute_batch(&create_sql)?;
    let resolved_schema: String = conn
        .query_row(
            "SELECT table_schema FROM information_schema.tables WHERE table_name = ?1 LIMIT 1",
            [name],
            |row| row.get(0),
        )
        // duckdb-rs collapses both "no rows" and DB errors into a single Err.
        // For dat0 Scratch mode the only reachable schema is "main", so falling
        // back to "main" preserves correctness without unwrapping prematurely.
        // P4 (workspace mode) may need to surface the error case explicitly.
        .unwrap_or_else(|_| "main".to_string());
    let columns = describe_table(conn, name, None)?;
    Ok(TableInfo {
        name: name.to_string(),
        schema: resolved_schema,
        columns,
        row_count_estimate: None,
        origin: TableOrigin::Derived(DerivedOrigin::Sql(sql.to_string())),
    })
}

pub(crate) fn drop_table(
    conn: &duckdb::Connection,
    name: &str,
    schema: Option<&str>,
) -> Result<()> {
    let qualified = qualified_name(name, schema);
    conn.execute_batch(&format!("DROP TABLE {}", qualified))?;
    Ok(())
}

pub(crate) fn rename_table(
    conn: &duckdb::Connection,
    old: &str,
    new: &str,
    schema: Option<&str>,
) -> Result<()> {
    let qualified_old = qualified_name(old, schema);
    conn.execute_batch(&format!(
        "ALTER TABLE {} RENAME TO {}",
        qualified_old,
        quote_ident(new)
    ))?;
    Ok(())
}

fn qualified_name(name: &str, schema: Option<&str>) -> String {
    match schema {
        Some(s) => format!("{}.{}", quote_ident(s), quote_ident(name)),
        None => quote_ident(name),
    }
}
