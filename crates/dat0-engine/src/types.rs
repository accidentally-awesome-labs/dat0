//! Engine type surface. Per spec §2.9.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;

use duckdb::arrow::record_batch::RecordBatch;
use futures::Stream;
use serde::{Deserialize, Serialize};

/// Engine lifecycle state.
///
/// Transition contract:
/// - `new()`              -> `Initializing`
/// - `init()` success     -> `Ready`
/// - `init()` failure     -> `Failed(reason)`
/// - `close()` entry      -> `Closing`
/// - `close()` complete   -> `Closed` (errors during cleanup are logged but do not affect the transition)
/// - poisoned mutex       -> `Failed(reason)` (transitioned on first observation)
///
/// In-flight query errors do **not** transition status. The engine remains
/// `Ready` until `close()` is invoked or a panic poisons the connection mutex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineStatus {
    Initializing,
    Ready,
    Closing,
    Closed,
    Failed(String),
}

/// Per-engine memory budget. Caller computes; engine applies via `PRAGMA memory_limit`.
#[derive(Debug, Clone, Copy)]
pub struct MemoryBudget {
    pub bytes: u64,
}

impl MemoryBudget {
    /// Format as a DuckDB pragma string, e.g. "16GB" or "512MB".
    pub fn as_pragma(&self) -> String {
        // DuckDB accepts bytes integers in newer versions but a units-suffixed string is safest.
        let mb = self.bytes / (1024 * 1024);
        format!("{}MB", mb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileFormat {
    Csv,
    Tsv,
    Json,
    Jsonl,
    Ndjson,
    Parquet,
}

impl FileFormat {
    /// Sniff format from a path extension. None means unknown — caller decides.
    pub fn from_extension(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "csv" => Some(FileFormat::Csv),
            "tsv" => Some(FileFormat::Tsv),
            "json" => Some(FileFormat::Json),
            "jsonl" => Some(FileFormat::Jsonl),
            "ndjson" => Some(FileFormat::Ndjson),
            "parquet" | "pq" => Some(FileFormat::Parquet),
            _ => None,
        }
    }
}

/// Per spec §2.9. `encoding` deliberately absent (D-010).
///
/// **Spec deviation (intentional):** spec §2.9 declares `format: FileFormat`
/// with auto-detection externalized to the caller. The plan uses
/// `format: Option<FileFormat>` so the engine handles auto-detect internally
/// (None = sniff from path extension). This is the same end-user contract,
/// shifted one layer: callers can still pass an explicit format. Document in
/// T1 commit message; revisit if P3 import wizard prefers explicit-format
/// dispatch.
#[derive(Debug, Clone, Default)]
pub struct RegisterOpts {
    pub format: Option<FileFormat>, // None = sniff from extension
    pub delimiter: Option<char>,
    pub quote_char: Option<char>,
    pub escape_char: Option<char>,
    pub has_header: Option<bool>,                // None = auto-detect
    pub type_overrides: HashMap<String, String>, // column_name -> DuckDB type literal
    pub sample_rows: Option<u32>,                // None = DuckDB default; Some(0) is invalid
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String, // DuckDB type literal as returned by DESCRIBE
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub enum TableOrigin {
    File(PathBuf),
    Derived(DerivedOrigin),
    Attached { alias: String, source: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DerivedOrigin {
    Sql(String),
    Transform {
        parent: String,
        ops: Vec<crate::transform::Transformation>,
    },
}

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub schema: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count_estimate: Option<u64>,
    pub origin: TableOrigin,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<ColumnInfo>,
    pub batches: Vec<RecordBatch>,
}

#[derive(Debug, Clone)]
pub struct PagedQueryResult {
    /// Exact row count of the *whole* result set, or `None` when the caller
    /// used [`QueryEngine::execute_page`](crate::QueryEngine::execute_page)
    /// and so deliberately paid for no `COUNT(*)`.
    ///
    /// `Some` therefore means two things at once: the total is known, AND the
    /// batch loop was reconciled against it (`execute::paged::run_paged`). On
    /// the `None` path neither holds — see D-030, which records why a
    /// mid-stream Arrow error is indistinguishable from EOF at duckdb-rs
    /// 1.4.4 and thus why the count is the only truncation detector we have.
    pub total_rows: Option<u64>,
    pub offset: u64,
    pub batches: Vec<RecordBatch>,
}

pub type ArrowRecordBatchStream =
    Pin<Box<dyn Stream<Item = Result<RecordBatch, crate::EngineError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Parquet,
}

#[derive(Clone, Default)]
pub struct AttachOpts {
    pub read_only: bool,
    pub schema_filter: Option<Vec<String>>,
    /// MotherDuck token, used only by the `md:` ATTACH path. Never logged:
    /// the manual `Debug` impl below redacts it.
    pub token: Option<String>,
}

impl std::fmt::Debug for AttachOpts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttachOpts")
            .field("read_only", &self.read_only)
            .field("schema_filter", &self.schema_filter)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Opaque, monotonically-minted identity for one in-flight engine query.
///
/// Minted by [`crate::DuckDBEngine::begin_query`]. Holding a token is what
/// lets a canceller say "abort *my* query" instead of "abort whatever is
/// running", which is all a bare `interrupt()` can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryToken(pub u64);

/// Which subsystem issued a query. Cancellation is scoped to a lane so a
/// console Cmd+. cannot abort a grid prefetch, and a superseding view change
/// cannot abort a console run. DuckDB gives one interrupt handle per
/// connection, so the scoping has to live above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLane {
    Console,
    Grid,
    View,
    Other,
}
