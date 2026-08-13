//! Error type for the `.dat0` package format (reader/writer/diff/replay).

use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum FormatError {
    #[error("unsupported package format version {found} (this dat0 reads major {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("checksum mismatch for {entry}")]
    ChecksumMismatch { entry: String },
    /// A zip entry name that would escape the extraction root once joined:
    /// an absolute path, a Windows drive prefix, or any `..` component.
    ///
    /// This is a package-level rejection, not a per-entry skip, because a
    /// `.dat0` produced by this writer can never contain one — every entry it
    /// writes is a fixed sidecar name or `data/<table>.parquet`. An entry that
    /// escapes therefore means the archive was hand-edited, and the honest
    /// response to a hand-edited archive is to refuse the whole thing rather
    /// than to read the parts that look benign.
    #[error("unsafe entry path in package: {entry}")]
    UnsafeEntryPath { entry: String },
    #[error("source schema incompatible: {0}")]
    SchemaIncompatible(String),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("engine error: {0}")]
    Engine(#[from] dat0_engine::EngineError),
}

pub type Result<T> = std::result::Result<T, FormatError>;
