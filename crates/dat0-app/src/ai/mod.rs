//! AI BYOK secure plumbing (P9c-1). Pure kernel (provider/request/wire/ssrf)
//! + key storage + settings + a single network seam (transport) + GPUI panel.

pub mod key_store;
pub mod panel;
pub mod prompt;
pub mod provider;
pub mod request;
pub mod schema_ctx;
pub mod settings;
pub mod sse;
pub mod ssrf;
pub mod transport;
pub mod wire;

pub use provider::{Provider, WireKind};
pub use request::{AiRequest, ColumnSchema, SampleRows, SchemaContext, TableSchema};
pub use settings::AiSettings;
