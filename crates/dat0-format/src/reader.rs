//! Reader — parses a `.dat0` zip back into a [`ParsedPackage`], verifying
//! the format version and all checksums in `manifest.json` before returning.
//!
//! Data bytes are NOT eagerly extracted; call [`ParsedPackage::extract_data_to`]
//! to pull the `data/` subtree out on demand.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::FORMAT_VERSION;
use crate::error::{FormatError, Result};
use crate::model::{Charts, PackageManifest, ParsedPackage, Queries, Recipe, Sources, Views};

/// Reads and validates a `.dat0` package file.
pub struct Reader;

impl Reader {
    /// Open a `.dat0` zip at `path`, verify its format version and all
    /// checksums recorded in `manifest.json`, then return the parsed model.
    ///
    /// The version check is performed BEFORE checksum verification so that a
    /// package written by a future major version is rejected with
    /// [`FormatError::UnsupportedVersion`] rather than a confusing
    /// [`FormatError::ChecksumMismatch`] or parse error.
    ///
    /// # Errors
    /// - [`FormatError::UnsupportedVersion`] — manifest's `format_version` ≠ [`FORMAT_VERSION`].
    /// - [`FormatError::ChecksumMismatch`] — a data/recipe entry's sha256 doesn't match.
    /// - [`FormatError::Zip`] — malformed zip or missing entry.
    /// - [`FormatError::Io`] — I/O failures.
    /// - [`FormatError::Json`] — JSON deserialization failures.
    pub fn open(path: &Path) -> Result<ParsedPackage> {
        let file = std::fs::File::open(path).map_err(|e| FormatError::Io {
            path: path.into(),
            source: e,
        })?;
        let mut zip = zip::ZipArchive::new(file)?;

        // 1. Read and version-check the manifest FIRST.
        let manifest: PackageManifest = read_json(&mut zip, "manifest.json")?;
        if manifest.format_version != FORMAT_VERSION {
            return Err(FormatError::UnsupportedVersion {
                found: manifest.format_version,
                supported: FORMAT_VERSION,
            });
        }

        // 2. Read the JSON sidecars.
        let recipe: Recipe = read_json(&mut zip, "recipe.json")?;
        let sources: Sources = read_json(&mut zip, "sources.json")?;
        let views: Views = read_json(&mut zip, "views.json")?;
        let queries: Queries = read_json(&mut zip, "queries.json")?;

        // 3. Verify all checksums recorded in the manifest.
        for (entry, expected) in &manifest.checksums {
            let mut f = zip.by_name(entry)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(|e| FormatError::Io {
                path: entry.into(),
                source: e,
            })?;
            let got = format!("sha256:{:x}", Sha256::digest(&buf));
            if &got != expected {
                return Err(FormatError::ChecksumMismatch {
                    entry: entry.clone(),
                });
            }
        }

        Ok(ParsedPackage {
            manifest,
            recipe,
            sources,
            views,
            queries,
            // T2 stub: T3 owns reading charts.json from the package.
            charts: Charts { charts: Vec::new() },
            zip_path: path.to_path_buf(),
        })
    }
}

impl ParsedPackage {
    /// Extract all `data/` entries from the package zip into `dir`.
    ///
    /// The extracted layout mirrors the zip: `dir/data/<name>.parquet`.
    /// Parent directories are created as needed.
    ///
    /// # Errors
    /// - [`FormatError::Zip`] — malformed zip.
    /// - [`FormatError::Io`] — I/O failures during extraction.
    pub fn extract_data_to(&self, dir: &Path) -> Result<()> {
        let file = std::fs::File::open(&self.zip_path).map_err(|e| FormatError::Io {
            path: self.zip_path.clone(),
            source: e,
        })?;
        let mut zip = zip::ZipArchive::new(file)?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            let name = f.name().to_string();
            if let Some(rest) = name.strip_prefix("data/") {
                // Skip directory entries (rest would be empty string).
                if rest.is_empty() {
                    continue;
                }
                let out = dir.join("data").join(rest);
                // parent always exists when rest is non-empty; create_dir_all handles nesting.
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).map_err(|e| FormatError::Io {
                    path: out.clone(),
                    source: e,
                })?;
                std::fs::write(&out, &buf).map_err(|e| FormatError::Io {
                    path: out,
                    source: e,
                })?;
            }
        }
        Ok(())
    }
}

/// Deserialize a JSON entry from the zip archive by name.
///
/// The returned file handle is dropped before the next `by_name` call, which
/// is the zip crate's borrowing contract for sequential random-access reads.
fn read_json<T: serde::de::DeserializeOwned>(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<T> {
    let mut f = zip.by_name(name)?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| FormatError::Io {
        path: name.into(),
        source: e,
    })?;
    Ok(serde_json::from_str(&s)?)
}
