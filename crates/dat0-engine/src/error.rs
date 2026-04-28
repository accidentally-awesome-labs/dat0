//! Engine error type. Per spec §2.10.

use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum EngineError {
    #[error("DuckDB error: {0}")]
    DuckDb(#[from] duckdb::Error),

    #[error("Arrow error: {0}")]
    Arrow(#[from] duckdb::arrow::error::ArrowError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),

    #[error("Unsupported file format: {0}")]
    UnsupportedFormat(String),

    #[error("Unknown ATTACH scheme: {0}; supported: sqlite:")]
    UnknownAttachScheme(String),

    #[error("Feature not yet implemented: {feature}")]
    NotImplemented { feature: &'static str },

    #[error("Migration {version} ({name}) failed: {source}")]
    Migration {
        version: u32,
        name: String,
        #[source]
        source: duckdb::Error,
    },

    #[error("Query interrupted")]
    Interrupted,

    #[error("Engine is closed or closing; new operations rejected")]
    EngineClosed,

    #[error("Engine connection mutex poisoned (prior panic in worker thread)")]
    EnginePoisoned,

    #[error("Engine is in Failed state: {0}")]
    EngineFailed(String),
}
