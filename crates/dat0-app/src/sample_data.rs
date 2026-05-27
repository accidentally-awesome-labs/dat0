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
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use sha2::{Digest, Sha256};

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

/// Download `url` to `$state_root/samples/<dest_filename>`, verifying the
/// SHA-256 of the response body against `expected_sha_hex` before the file
/// lands at its final path.
///
/// Contract:
///
/// 1. **Cache hit short-circuits.** If the destination already exists, the
///    function returns its path immediately *without* hitting the network
///    or re-verifying the on-disk bytes. The cache is trusted because the
///    only writer is `fetch_remote` itself, and writes are atomic (see
///    point 3).
/// 2. **Status check first.** Non-2xx HTTP responses fail before any
///    bytes are buffered or written to disk — the error message includes
///    the numeric status code so the T2 banner can surface `"... 404 ..."`.
/// 3. **Atomic write.** The verified body is written to
///    `<dest>.part`, fsync'd, then renamed into place. A failed checksum
///    leaves no partial file (the `.part` path is never reached on
///    mismatch).
/// 4. **Case-insensitive sha compare.** `expected_sha_hex` may be upper-
///    or lower-case hex; both are normalised before equality.
///
/// Errors carry an `anyhow` chain — caller composes them into
/// `fetch_failed_banner` for the offline-failure UX.
pub async fn fetch_remote(
    url: &str,
    expected_sha_hex: &str,
    state_root: &Path,
    dest_filename: &str,
) -> anyhow::Result<PathBuf> {
    let dir = state_root.join("samples");
    std::fs::create_dir_all(&dir).context("create samples dir")?;
    let dest = dir.join(dest_filename);
    if dest.exists() {
        // Trusted-cache short-circuit: re-verifying every load would
        // re-read potentially gigabytes off disk on every startup, which
        // is not what users mean by "open the cached sample." The only
        // writer is this function (atomic rename below), so a cache file
        // is by construction the byte-for-byte verified payload.
        return Ok(dest);
    }

    let resp = reqwest::get(url)
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP status {} for {url}", resp.status()));
    }
    let bytes = resp.bytes().await.context("read response body")?;

    let mut h = Sha256::new();
    h.update(&bytes);
    let got_sha = format!("{:x}", h.finalize());
    let expected_norm = expected_sha_hex.to_lowercase();
    if got_sha != expected_norm {
        return Err(anyhow!(
            "checksum mismatch: expected {expected_norm} got {got_sha} for {url}"
        ));
    }

    // Atomic write: bytes → <dest>.part → fsync → rename(<dest>.part, dest).
    // The .part suffix keeps a half-written download from being mistaken
    // for a valid cache entry by a future short-circuit check.
    let tmp_path = dir.join(format!("{dest_filename}.part"));
    {
        let mut f = std::fs::File::create(&tmp_path).context("create temp file")?;
        f.write_all(&bytes).context("write bytes")?;
        f.sync_all().context("fsync")?;
    }
    std::fs::rename(&tmp_path, &dest).context("rename into place")?;

    Ok(dest)
}

/// Compose a T2 [`crate::error_ux::Banner`] for the offline / fetch-failed
/// UX: title + "Couldn't download from <url>: <err>" body + a `Retry`
/// primary action wired to the `sample_data.retry_taxi` registry id.
///
/// The error chain is rendered with `{err}` (Display) not `{err:?}` so the
/// banner body stays single-line and user-friendly. Callers that want the
/// full backtrace should also `tracing::warn!("…", error = ?err)`.
pub fn fetch_failed_banner(url: &str, err: &anyhow::Error) -> crate::error_ux::Banner {
    crate::error_ux::Banner::error(
        "Sample data download failed",
        format!("Couldn't download from {url}: {err}"),
    )
    .with_primary("Retry", "sample_data.retry_taxi")
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
