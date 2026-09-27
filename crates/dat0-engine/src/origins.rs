//! Where each table came from, and how its file was read, kept in the
//! database beside the tables (PD-031, PD-037).
//!
//! The engine held both in memory only, so an engine opened on an existing
//! database — a recovered session, a workspace, a moved file — knew its
//! tables but not their sources. A table made by SQL or saved from a view
//! lost its derivation, one with no tab lost its file, and a file the import
//! wizard read with a dialect of its own was read again without it. Each
//! change is now written to `__dat0_meta_origins` by the operation that makes
//! it, under the same lock, and `init` reads the rows back.
//!
//! Best-effort by design: the table is the user's data and its origin is
//! commentary on it, so a row that cannot be written or read is logged and
//! never fails the operation it describes.
//!
//! An attached table's origin is not kept: it is its attachment's, which the
//! session records and attaches again when it opens.

use std::collections::HashMap;

use crate::types::{FileRead, TableOrigin};

/// Record `name`'s origin, and for a file how it was read, replacing any row
/// it had.
pub(crate) fn save(
    conn: &duckdb::Connection,
    name: &str,
    origin: &TableOrigin,
    read: Option<&FileRead>,
) {
    if matches!(origin, TableOrigin::Attached { .. }) {
        return;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let origin = serde_json::to_string(origin)?;
        let read = read.map(serde_json::to_string).transpose()?;
        conn.execute(
            "INSERT OR REPLACE INTO __dat0_meta_origins (name, origin, file_read) \
             VALUES (?, ?, ?)",
            duckdb::params![name, origin, read],
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!(table = %name, error = %e, "origins: could not record the origin");
    }
}

/// Record how `name`'s file was read, on the row its registration wrote.
pub(crate) fn save_read(conn: &duckdb::Connection, name: &str, read: &FileRead) {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let read = serde_json::to_string(read)?;
        conn.execute(
            "UPDATE __dat0_meta_origins SET file_read = ? WHERE name = ?",
            duckdb::params![read, name],
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!(table = %name, error = %e, "origins: could not record how the file was read");
    }
}

/// `name` is gone: so is its row.
pub(crate) fn forget(conn: &duckdb::Connection, name: &str) {
    if let Err(e) = conn.execute(
        "DELETE FROM __dat0_meta_origins WHERE name = ?",
        duckdb::params![name],
    ) {
        tracing::warn!(table = %name, error = %e, "origins: could not forget the origin");
    }
}

/// `old` is called `new` now, and its row goes with it: read, then written
/// under the new name, rather than updated in place, since the name is the
/// key.
pub(crate) fn rename(conn: &duckdb::Connection, old: &str, new: &str) {
    let result = (|| -> duckdb::Result<()> {
        let row = match conn.query_row(
            "SELECT origin, file_read FROM __dat0_meta_origins WHERE name = ?",
            duckdb::params![old],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        ) {
            Ok(row) => Some(row),
            Err(duckdb::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };
        conn.execute(
            "DELETE FROM __dat0_meta_origins WHERE name = ? OR name = ?",
            duckdb::params![old, new],
        )?;
        if let Some((origin, read)) = row {
            conn.execute(
                "INSERT INTO __dat0_meta_origins (name, origin, file_read) VALUES (?, ?, ?)",
                duckdb::params![new, origin, read],
            )?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!(%old, %new, error = %e, "origins: could not rename the origin");
    }
}

/// Every table's recorded origin and file read, for an engine opening its
/// database. Rows whose table is gone — dropped by SQL the engine did not
/// run — are removed first, so a new table of that name inherits nothing.
/// A row that does not parse is left out and logged.
pub(crate) fn load(
    conn: &duckdb::Connection,
) -> (HashMap<String, TableOrigin>, HashMap<String, FileRead>) {
    let mut origins = HashMap::new();
    let mut reads = HashMap::new();
    if let Err(e) = conn.execute_batch(
        "DELETE FROM __dat0_meta_origins WHERE name NOT IN (
             SELECT table_name FROM information_schema.tables
             WHERE table_catalog = current_database() AND table_schema = 'main'
         );",
    ) {
        tracing::warn!(error = %e, "origins: could not prune the rows of gone tables");
    }
    let rows = (|| -> duckdb::Result<Vec<(String, String, Option<String>)>> {
        let mut stmt = conn.prepare("SELECT name, origin, file_read FROM __dat0_meta_origins")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<duckdb::Result<Vec<_>>>()?;
        Ok(rows)
    })();
    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "origins: could not read the recorded origins");
            return (origins, reads);
        }
    };
    for (name, origin, read) in rows {
        match serde_json::from_str::<TableOrigin>(&origin) {
            Ok(TableOrigin::Attached { .. }) => {}
            Ok(o) => {
                origins.insert(name.clone(), o);
            }
            Err(e) => {
                tracing::warn!(table = %name, error = %e, "origins: an origin does not parse, left out");
                continue;
            }
        }
        if let Some(read) = read {
            match serde_json::from_str::<FileRead>(&read) {
                Ok(r) => {
                    reads.insert(name, r);
                }
                Err(e) => {
                    tracing::warn!(table = %name, error = %e, "origins: a file read does not parse, left out");
                }
            }
        }
    }
    (origins, reads)
}
