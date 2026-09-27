//! Running SQL that did not come from the person running dat0.
//!
//! A `.dat0` package carries the SQL that derived its tables, and replaying the
//! package runs it. That SQL arrived with the file, so it runs under two limits:
//!
//! - [`check_single_query_blocking`]: it must be exactly one query — a SELECT,
//!   a set operation, VALUES or a FROM-first query — as DuckDB's own parser
//!   reads it. A table's derivation has no use for anything else.
//! - [`confine_blocking`]: the engine gives up, for the rest of its life, every
//!   file, network and extension access outside one directory. Queries can
//!   still read the tables already loaded, which is all a derivation needs.
//!
//! The confinement is the boundary; the shape check makes a bad recipe fail
//! with a message about the recipe rather than a permission error.

use std::path::Path;

use crate::Result;
use crate::error::EngineError;

/// `Ok` when `sql` is exactly one query; [`EngineError::NotASingleQuery`]
/// naming what it is otherwise.
pub(crate) fn check_single_query_blocking(conn: &duckdb::Connection, sql: &str) -> Result<()> {
    // `json_serialize_sql` is DuckDB's own parser, and it only serializes
    // SELECT-family statements, so one call answers both questions. The
    // `::VARCHAR` cast is required for a bound parameter (see `lineage`).
    let json: String =
        conn.query_row("SELECT json_serialize_sql(?::VARCHAR)", [sql], |r| r.get(0))?;
    let v: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| EngineError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    if v.get("error").and_then(serde_json::Value::as_bool) != Some(false) {
        let why = v
            .get("error_message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("it could not be parsed");
        return Err(EngineError::NotASingleQuery(why.to_string()));
    }
    match v
        .get("statements")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
    {
        Some(1) => Ok(()),
        Some(0) => Err(EngineError::NotASingleQuery("it is empty".to_string())),
        Some(n) => Err(EngineError::NotASingleQuery(format!(
            "it is {n} statements"
        ))),
        None => Err(EngineError::NotASingleQuery(
            "it could not be parsed".to_string(),
        )),
    }
}

/// Deny this connection's database any file, network or extension access
/// outside `dir`, and lock its configuration so nothing can grant it back.
pub(crate) fn confine_blocking(conn: &duckdb::Connection, dir: &Path) -> Result<()> {
    let dir = dir
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(dir.to_path_buf()))?
        .replace('\'', "''");
    // Order matters: `lock_configuration` last, or it would refuse the rest.
    conn.execute_batch(&format!(
        "SET allowed_directories = ['{dir}'];
         SET enable_external_access = false;
         SET autoinstall_known_extensions = false;
         SET autoload_known_extensions = false;
         SET lock_configuration = true;"
    ))?;
    Ok(())
}
