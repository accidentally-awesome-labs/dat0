//! Model types for the `.dat0` package format (design D-format-v1).
//!
//! These are the on-disk JSON contract for a `.dat0` package. The wire shapes
//! here are normative — see `docs/dat0-format-v1.md` and the published JSON
//! Schema at `docs/schemas/dat0-manifest-v1.schema.json`. Tagged enums follow
//! the PD-014 self-describing convention so a non-Rust reader can route on the
//! `kind` discriminator.

use dat0_engine::transform::Transformation;
use serde::{Deserialize, Serialize};

pub const PACKAGE_KIND: &str = "package";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    pub format_version: u32, // == crate::FORMAT_VERSION
    pub kind: String,        // PACKAGE_KIND
    pub dat0_version: String,
    pub package_id: uuid::Uuid,   // now_v7
    pub workspace_id: uuid::Uuid, // origin workspace, provenance
    pub created_at: String,       // opaque creation timestamp (dat0 writes epoch-seconds)
    pub table_count: u32,
    pub checksums: std::collections::BTreeMap<String, String>, // "data/sales.parquet" -> "sha256:…"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnFingerprint {
    pub name: String,
    pub r#type: String,
} // DuckDB type literal

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    Base,
    Derived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Derivation {
    Sql {
        sql: String,
        parents: Vec<String>,
    },
    Transform {
        parent: String,
        ops: Vec<Transformation>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecipeTable {
    pub id: String,   // stable, e.g. "t_sales"
    pub name: String, // DuckDB table name
    pub kind: TableKind,
    pub schema: Vec<ColumnFingerprint>,
    pub row_count: u64,
    pub data: String, // "data/<name>.parquet"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>, // base only -> PackageSource.id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivation: Option<Derivation>, // derived only
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recipe {
    pub tables: Vec<RecipeTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageSource {
    pub id: String,           // "src_sales"
    pub logical_name: String, // "sales.csv"
    pub original_uri: String, // informational
    pub schema_fingerprint: Vec<ColumnFingerprint>,
    pub content_hash: String, // sha256 of source bytes at export
    pub row_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sources {
    pub sources: Vec<PackageSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageView {
    // mirrors dat0_app::session::Tab (portable subset)
    pub table_name: String,
    #[serde(default)]
    pub transform_stack: Vec<Transformation>,
    #[serde(default)]
    pub undo_cursor: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Views {
    pub views: Vec<PackageView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageQuery {
    pub id: uuid::Uuid,
    pub name: String,
    pub sql: String,
    pub saved_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Queries {
    pub queries: Vec<PackageQuery>,
}

/// Writer input — the portable contents, assembled by the app from a Session.
/// Data bytes are NOT here; the writer pulls them from `engine` per RecipeTable.name.
#[derive(Debug, Clone)]
pub struct PackageContents {
    pub workspace_id: uuid::Uuid,
    pub created_at: String,
    pub recipe: Recipe,
    pub sources: Sources,
    pub views: Views,
    pub queries: Queries,
}

/// Reader output.
#[derive(Debug, Clone)]
pub struct ParsedPackage {
    pub manifest: PackageManifest,
    pub recipe: Recipe,
    pub sources: Sources,
    pub views: Views,
    pub queries: Queries,
    pub(crate) zip_path: std::path::PathBuf, // for lazy data extraction
}
