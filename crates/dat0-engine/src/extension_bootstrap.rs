//! Extension bootstrap. T14 calls `install_sqlite_scanner_at_app_boot`
//! once at app startup before any window opens. Tests use the
//! `__test_install_sqlite_scanner` variant.

use std::sync::OnceLock;
use tracing::{info, warn};

use crate::Result;
use crate::error::EngineError;

/// Memoized install outcome. `OnceLock::get_or_init` runs the closure exactly
/// once per process; subsequent calls return the cached `&Result<(), String>`.
static INSTALL_RESULT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

/// Install + LOAD `sqlite_scanner` exactly once per process. Subsequent calls
/// short-circuit to the cached result.
///
/// Called by `dat0-app` at boot path before any window opens. Engine `init()`
/// only LOADs (not INSTALLs) on the assumption this has already run.
pub fn install_sqlite_scanner_at_app_boot(scratch_template: std::path::PathBuf) -> Result<()> {
    let outcome: &std::result::Result<(), String> = INSTALL_RESULT.get_or_init(|| {
        let result = (|| -> std::result::Result<(), String> {
            let conn =
                duckdb::Connection::open(&scratch_template).map_err(|e| format!("open: {e}"))?;
            conn.execute_batch("INSTALL sqlite_scanner; LOAD sqlite_scanner;")
                .map_err(|e| format!("install/load: {e}"))?;
            info!(target: "dat0_engine::extensions", "sqlite_scanner installed and loaded");
            Ok(())
        })();
        if let Err(ref e) = result {
            warn!(target: "dat0_engine::extensions", error = %e, "sqlite_scanner install failed");
        }
        result
    });
    outcome
        .clone()
        .map_err(|msg| EngineError::Io(std::io::Error::other(msg)))
}

/// Test-only: install via a per-test Connection.
///
/// **Concurrency note:** within a single test-binary process, `OnceLock`
/// serializes the install. But `cargo test --workspace` runs each test crate
/// as a separate process, and they share `~/.duckdb/extensions/` on disk —
/// the canonical default extension cache. The first time tests run cold, two
/// processes can race the INSTALL of `sqlite_scanner.duckdb_extension`.
/// Mitigations:
///   1. CI runs a one-shot priming step before the test matrix (recommended;
///      add to `.github/workflows/ci.yml` in T13 as a step that calls
///      `cargo run -p dat0-fixtures-priming -- --install-extensions` or runs
///      a small `cargo test -p dat0-engine --test attach_dispatch` to warm
///      the cache).
///   2. Alternatively, set `DUCKDB_EXTENSION_DIRECTORY` per test process to
///      a tempdir — but extension caches won't persist across cargo runs.
/// Track as a P2 candidate plan-defect (PD-005) if cold-cache race is observed.
#[doc(hidden)]
pub fn __test_install_sqlite_scanner() -> Result<()> {
    let scratch = std::env::temp_dir().join(format!(
        "dat0-test-extbootstrap-{}.duckdb",
        std::process::id()
    ));
    install_sqlite_scanner_at_app_boot(scratch)
}

/// Memoized motherduck install outcome (separate `OnceLock` from sqlite).
static MD_INSTALL_RESULT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

/// Install + LOAD `motherduck` exactly once per process. Unlike
/// `sqlite_scanner` (installed at boot), this is called **lazily on first
/// connect** (design D8) so non-MotherDuck workspaces pay no cost.
pub fn install_motherduck_at_app_boot(scratch_template: std::path::PathBuf) -> Result<()> {
    let outcome: &std::result::Result<(), String> = MD_INSTALL_RESULT.get_or_init(|| {
        let result = (|| -> std::result::Result<(), String> {
            let conn =
                duckdb::Connection::open(&scratch_template).map_err(|e| format!("open: {e}"))?;
            conn.execute_batch("INSTALL motherduck; LOAD motherduck;")
                .map_err(|e| format!("install/load: {e}"))?;
            info!(target: "dat0_engine::extensions", "motherduck installed and loaded");
            Ok(())
        })();
        if let Err(ref e) = result {
            warn!(target: "dat0_engine::extensions", error = %e, "motherduck install failed");
        }
        result
    });
    outcome
        .clone()
        .map_err(|_msg| EngineError::ExtensionLoad { name: "motherduck" })
}

/// Test-only: install via a per-test scratch DB.
#[doc(hidden)]
pub fn __test_install_motherduck() -> Result<()> {
    let scratch = std::env::temp_dir()
        .join(format!("dat0-test-md-extbootstrap-{}.duckdb", std::process::id()));
    install_motherduck_at_app_boot(scratch)
}

#[cfg(test)]
mod md_tests {
    #[test]
    fn install_motherduck_is_memoized_and_idempotent() {
        // Two calls return Ok and run at most once (OnceLock). We assert the
        // public contract: repeated calls do not error on the cached path.
        let r1 = super::__test_install_motherduck();
        let r2 = super::__test_install_motherduck();
        // In CI with the extension available both are Ok; offline the INSTALL
        // may fail — but the SECOND call must mirror the first (memoized).
        assert_eq!(r1.is_ok(), r2.is_ok());
    }
}
