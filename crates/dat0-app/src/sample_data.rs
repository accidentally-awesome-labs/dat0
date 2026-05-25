//! Sample-data catalog + bundled-fixture extraction (P3b T7).
//!
//! Exposes [`entries`] — the canonical list of three sample datasets
//! surfaced by the empty-state hero ([`crate::empty_state`]) — and
//! [`ensure_bundled_extracted`], which writes a bundled byte blob to
//! `$STATE/samples/<filename>` on demand (idempotent: a second call is a
//! no-op once the file exists).
//!
//! ## Sources & licenses
//!
//! - **Iris CSV** — scikit-learn fork, BSD-3-Clause.
//! - **Chinook SQLite** — lerocha/chinook-database, MIT.
//! - **NYC taxi Parquet** — fetched at runtime from a GitHub Release
//!   asset; checksum verified by T8 against [`NYC_TAXI_SHA256`].
//!
//! See `crates/dat0-app/assets/README.md` for URLs and provenance.
//! `NOTICE.md` regeneration is owned by P3b T13's `cargo-about` run.

use std::io;
use std::path::{Path, PathBuf};

/// Iris dataset, bundled via `include_bytes!`. BSD-3 (scikit-learn fork).
pub const IRIS_CSV: &[u8] = include_bytes!("../assets/iris.csv");

/// Chinook sample SQLite, bundled via `include_bytes!`. MIT.
pub const CHINOOK_SQLITE: &[u8] = include_bytes!("../assets/chinook.sqlite");

/// Remote URL for the NYC taxi Parquet sample. Fetched by T8.
pub const NYC_TAXI_URL: &str = "https://github.com/accidentally-awesome-labs/dat0/releases/download/sample-data-v1/nyc_taxi.parquet";

/// SHA-256 of the remote NYC taxi sample. Filled by T8 once the release
/// asset lands; T7 leaves it as a placeholder so [`SampleKind::Remote`]
/// has a stable slot.
pub const NYC_TAXI_SHA256: &str = "FILL_AT_T8";

/// Catalog entry rendered by the empty-state hero. The `kind` discriminant
/// drives whether the sample is extracted from a bundled blob or fetched
/// over the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleEntry {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub kind: SampleKind,
}

/// How a sample is materialized into `$STATE/samples/<dest_filename>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleKind {
    /// Bundled CSV: `bytes` is a slice into the binary (`include_bytes!`).
    BundledCsv {
        bytes: &'static [u8],
        dest_filename: &'static str,
    },
    /// Bundled SQLite: `bytes` is a slice into the binary.
    BundledSqlite {
        bytes: &'static [u8],
        dest_filename: &'static str,
    },
    /// Remote asset: T8 implements the download + SHA-256 verify path.
    Remote {
        url: &'static str,
        sha256: &'static str,
        dest_filename: &'static str,
        approx_size_bytes: u64,
    },
}

/// Canonical list of samples. Order is render order (top-to-bottom in the
/// empty-state hero column).
pub fn entries() -> Vec<SampleEntry> {
    vec![
        SampleEntry {
            title: "Iris",
            subtitle: "150 rows × 5 cols — classic CSV",
            kind: SampleKind::BundledCsv {
                bytes: IRIS_CSV,
                dest_filename: "iris.csv",
            },
        },
        SampleEntry {
            title: "Chinook",
            subtitle: "~1 MB SQLite — multi-table demo",
            kind: SampleKind::BundledSqlite {
                bytes: CHINOOK_SQLITE,
                dest_filename: "chinook.sqlite",
            },
        },
        SampleEntry {
            title: "NYC taxi",
            subtitle: "~50 MB Parquet — remote fetch on first use",
            kind: SampleKind::Remote {
                url: NYC_TAXI_URL,
                sha256: NYC_TAXI_SHA256,
                dest_filename: "nyc_taxi.parquet",
                approx_size_bytes: 50 * 1024 * 1024,
            },
        },
    ]
}

/// Write `bytes` to `$state_root/samples/<filename>` if the file does
/// not already exist. Returns the absolute path to the extracted file.
///
/// Idempotent: a second call with the same arguments returns the same
/// path without touching disk (so `mtime` is preserved — exercised by the
/// `ensure_bundled_extracted_is_idempotent` test).
pub fn ensure_bundled_extracted(
    state_root: &Path,
    bytes: &[u8],
    filename: &str,
) -> io::Result<PathBuf> {
    let samples_dir = state_root.join("samples");
    std::fs::create_dir_all(&samples_dir)?;
    let dest = samples_dir.join(filename);
    if !dest.exists() {
        std::fs::write(&dest, bytes)?;
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn bundled_blobs_non_empty() {
        assert!(!IRIS_CSV.is_empty(), "iris.csv blob is empty");
        assert!(!CHINOOK_SQLITE.is_empty(), "chinook.sqlite blob is empty");
        assert!(
            CHINOOK_SQLITE.len() > 100 * 1024,
            "chinook.sqlite suspiciously small ({} bytes)",
            CHINOOK_SQLITE.len()
        );
    }

    #[test]
    fn ensure_bundled_extracted_is_idempotent() {
        let dir = tempdir().unwrap();
        let p1 = ensure_bundled_extracted(dir.path(), IRIS_CSV, "iris.csv").unwrap();
        let m1 = std::fs::metadata(&p1).unwrap().modified().unwrap();
        // Sleep a hair so any second write would record a different mtime
        // on filesystems with millisecond resolution.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let p2 = ensure_bundled_extracted(dir.path(), IRIS_CSV, "iris.csv").unwrap();
        let m2 = std::fs::metadata(&p2).unwrap().modified().unwrap();
        assert_eq!(p1, p2);
        assert_eq!(m1, m2, "second call should not rewrite the file");
    }

    #[test]
    fn entries_lists_three_samples() {
        let e = entries();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].title, "Iris");
        assert_eq!(e[1].title, "Chinook");
        assert_eq!(e[2].title, "NYC taxi");
        assert!(matches!(e[0].kind, SampleKind::BundledCsv { .. }));
        assert!(matches!(e[1].kind, SampleKind::BundledSqlite { .. }));
        assert!(matches!(e[2].kind, SampleKind::Remote { .. }));
    }
}
