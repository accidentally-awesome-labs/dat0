//! `DuckDBEngine` — sole `QueryEngine` impl in v1.
//!
//! Implementation lands across T2 (bootstrap), T3 (migrations), T4–T6 (register_file),
//! T7–T8 (execute family), T9 (catalog), T10 (export), T11 (attach/detach).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::types::{EngineStatus, MemoryBudget};

/// The concrete engine type. Per spec §2.1.
#[allow(dead_code)] // T2+ wires these; T1 ships the type so `lib.rs` re-export compiles.
pub struct DuckDBEngine {
    pub(crate) conn: Arc<Mutex<duckdb::Connection>>,
    pub(crate) interrupt: Arc<duckdb::InterruptHandle>,
    pub(crate) budget: MemoryBudget,
    pub(crate) scratch_path: PathBuf,
    pub(crate) status: Arc<RwLock<EngineStatus>>,
}

impl DuckDBEngine {
    /// Construction lands in T2. This stub is here only to make `lib.rs`'s
    /// public re-export compile in T1.
    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn __t1_stub_marker(&self) {}
}
