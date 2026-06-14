//! Error type for the `.dat0` package format (reader/writer/diff/replay).

use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum FormatError {
    #[error("unsupported package format version {found} (this dat0 reads major {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("checksum mismatch for {entry}")]
    ChecksumMismatch { entry: String },
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
