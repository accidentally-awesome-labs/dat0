//! Forward-only, append-only migration runner. Per spec §2.6.
//!
//! In P2 the runner targets per-engine scratch DBs only. Workspace-DB
//! concurrent-open race is a P3 entry-time review item.

use tracing::{info, warn};

use crate::error::EngineError;

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub up: fn(&duckdb::Connection) -> std::result::Result<(), duckdb::Error>,
}

/// Production migrations. Forward-only, append-only. Never edit a shipped entry.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    up: m001_init,
}];

/// Apply all migrations whose version is greater than the current applied version.
/// Idempotent — safe to call on every `init()`. Each migration runs inside a
/// transaction; failure rolls back and surfaces as `EngineError::Migration`.
pub fn apply_migrations(
    conn: &duckdb::Connection,
    migrations: &[Migration],
) -> std::result::Result<u32, EngineError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS __dat0_meta_migrations (
            version    INTEGER PRIMARY KEY,
            name       VARCHAR NOT NULL,
            applied_at TIMESTAMP DEFAULT current_timestamp
        );",
    )?;

    let current: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM __dat0_meta_migrations",
        [],
        |r| r.get::<_, u32>(0),
    )?;

    let started = std::time::Instant::now();

    for m in migrations.iter().filter(|m| m.version > current) {
        let migration_started = std::time::Instant::now();

        // DuckDB does not yet support nested transactions everywhere; use
        // an explicit BEGIN/COMMIT/ROLLBACK pair.
        conn.execute_batch("BEGIN;")?;
        let res = (m.up)(conn).and_then(|_| {
            conn.execute(
                "INSERT INTO __dat0_meta_migrations (version, name) VALUES (?, ?)",
                duckdb::params![m.version, m.name],
            )?;
            Ok(())
        });
        match res {
            Ok(()) => {
                conn.execute_batch("COMMIT;")?;
                let dur_ms = migration_started.elapsed().as_millis();
                info!(
                    target: "dat0_engine::migrations",
                    version = m.version,
                    name = m.name,
                    duration_ms = dur_ms as u64,
                    "migration applied"
                );
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                warn!(
                    target: "dat0_engine::migrations",
                    version = m.version,
                    name = m.name,
                    error = %e,
                    "migration failed; rolled back"
                );
                return Err(EngineError::Migration {
                    version: m.version,
                    name: m.name.to_string(),
                    source: e,
                });
            }
        }
    }

    let final_version = migrations.last().map(|m| m.version).unwrap_or(0);
    info!(
        target: "dat0_engine::migrations",
        from = current,
        to = final_version,
        total_duration_ms = started.elapsed().as_millis() as u64,
        "migrations complete"
    );
    Ok(final_version)
}

fn m001_init(conn: &duckdb::Connection) -> std::result::Result<(), duckdb::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS __dat0_meta (
            key   VARCHAR PRIMARY KEY,
            value VARCHAR NOT NULL
        );
        INSERT OR IGNORE INTO __dat0_meta (key, value) VALUES ('dat0_workspace_version', '1');",
    )?;
    Ok(())
}

/// Test re-export so `tests/migrations.rs` can construct custom migration sets.
#[doc(hidden)]
pub fn __test_only_m001_init(conn: &duckdb::Connection) -> std::result::Result<(), duckdb::Error> {
    m001_init(conn)
}
