//! AI BYOK secure plumbing (P9c-1). Pure kernel (provider/request/wire/ssrf)
//! + key storage + settings + a single network seam (transport) + GPUI panel.

pub mod provider;

pub use provider::{Provider, WireKind};
