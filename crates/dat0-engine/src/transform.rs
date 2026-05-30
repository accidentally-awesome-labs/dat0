//! Typed Transformation enum for ViewModel + lineage serialization (design §3).
//!
//! Serde-derived. Wire format is self-describing JSONL so it can be consumed
//! verbatim by:
//!   - P4a session.json schema v2 (per-tab active stack)
//!   - P7 .dat0/lineage/transforms.jsonl (materialized lineage)
//!   - P8 .dat0 format (replay-on-new-source)
//!
//! Engine does NOT validate column existence or type compatibility at
//! construction; that's the render-time gate (see `crate::render`).
//!
//! ## Serde wire format (tagged — design §3)
//!
//! `Transformation` uses `#[serde(tag = "kind", rename_all = "snake_case")]` —
//! each JSONL row is self-describing.
//!
//! `FilterValue` uses `#[serde(tag = "kind", rename_all = "snake_case")]`
//! (internally tagged). Variants on the wire:
//!   - `{ "kind": "scalar", "value": <Scalar> }`
//!   - `{ "kind": "range",  "lo": <Scalar>, "hi": <Scalar>, "inclusive": bool }`
//!   - `{ "kind": "list",   "values": [<Scalar>, ...] }`
//!   - `{ "kind": "none" }`
//!
//! `Scalar` uses `#[serde(tag = "type", content = "value", rename_all = "snake_case")]`
//! (adjacent tagged). Variants on the wire:
//!   - `{ "type": "null" }`
//!   - `{ "type": "bool",      "value": true }`
//!   - `{ "type": "int",       "value": 42 }`
//!   - `{ "type": "float",     "value": 10.0 }`
//!   - `{ "type": "str",       "value": "hello" }`
//!   - `{ "type": "date",      "value": "2026-01-01" }`
//!   - `{ "type": "timestamp", "value": "2026-01-01 00:00:00" }`
//!
//! This avoids the `#[serde(untagged)]` collision where `Scalar::Str`,
//! `Scalar::Date`, and `Scalar::Timestamp` all serialize as plain JSON strings
//! (making round-trip ambiguous), and where `FilterValue::None` would collide
//! with `FilterValue::Scalar(Scalar::Null)` (both serialize as JSON `null`).
//! See PD-014 in `docs/deferrals.md`.
//!
//! Cross-language consumers (e.g. future Python .dat0 reader for P8) can route
//! on the discriminator fields without Rust-side knowledge.
//!
//! See design §3 for the full wire-format specification and examples.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transformation {
    Filter {
        column: String,
        op: FilterOp,
        value: FilterValue,
    },
    Sort {
        keys: Vec<SortKey>,
    },
    // P4b will add: Edit, RowDelete
    // P4c will add: Reorder, Rename, Delete (column)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    Between,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    In,
    Regex,
    IsEmpty,
    IsNotEmpty,
    IsTrue,
    IsFalse,
}

/// Value attached to a filter predicate.
///
/// Internally tagged on `"kind"` (design §3):
/// - `Scalar { value }` — single typed value
/// - `Range { lo, hi, inclusive }` — used by `Between`
/// - `List { values }` — used by `In`
/// - `None` — nullary ops (IsEmpty, IsNotEmpty, IsTrue, IsFalse)
///
/// The tagged shape is required because a naive `#[serde(untagged)]` approach
/// would make `FilterValue::None` collide with `FilterValue::Scalar(Scalar::Null)`
/// (both would round-trip as JSON `null`). See PD-014.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterValue {
    Scalar {
        value: Scalar,
    },
    Range {
        lo: Scalar,
        hi: Scalar,
        inclusive: bool,
    },
    List {
        values: Vec<Scalar>,
    },
    None,
}

/// Scalar value for filter predicates.
///
/// Adjacent-tagged on `"type"`/`"value"` (design §3). Keeps DuckDB type
/// literals explicit so the render path (`compile_view_sql`) always knows
/// which SQL literal form to emit.
///
/// The tagged shape is required because `#[serde(untagged)]` would collapse
/// `Scalar::Str`, `Scalar::Date`, and `Scalar::Timestamp` into identical plain
/// JSON strings, making deserialization ambiguous. See PD-014.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Scalar {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// ISO-8601 yyyy-mm-dd. Validated at parse: see `Scalar::validate_date`.
    Date(String),
    /// ISO-8601 RFC 3339. Validated at parse: see `Scalar::validate_timestamp`.
    Timestamp(String),
}

impl Scalar {
    /// Validate that `s` matches `yyyy-mm-dd`. Cheap byte-pattern check, no
    /// calendar arithmetic (DuckDB does its own validation on parse). Returns
    /// the original string on success.
    pub fn validate_date(s: &str) -> Result<&str, &'static str> {
        let b = s.as_bytes();
        if b.len() == 10
            && b[0..4].iter().all(|c| c.is_ascii_digit())
            && b[4] == b'-'
            && b[5..7].iter().all(|c| c.is_ascii_digit())
            && b[7] == b'-'
            && b[8..10].iter().all(|c| c.is_ascii_digit())
        {
            Ok(s)
        } else {
            Err("expected yyyy-mm-dd")
        }
    }

    /// Validate that `s` parses as ISO-8601 RFC 3339 (DuckDB-compatible).
    /// Accepts `yyyy-mm-dd hh:mm:ss[.fff][+hh:mm|Z]`. Cheap byte-pattern
    /// check; DuckDB does the full parse on render.
    pub fn validate_timestamp(s: &str) -> Result<&str, &'static str> {
        let b = s.as_bytes();
        // Minimum length: yyyy-mm-dd hh:mm:ss = 19 bytes.
        if b.len() < 19 {
            return Err("timestamp too short");
        }
        Self::validate_date(&s[..10])?;
        if b[10] != b' ' && b[10] != b'T' {
            return Err("expected space or T between date and time");
        }
        if !(b[11..13].iter().all(|c| c.is_ascii_digit())
            && b[13] == b':'
            && b[14..16].iter().all(|c| c.is_ascii_digit())
            && b[16] == b':'
            && b[17..19].iter().all(|c| c.is_ascii_digit()))
        {
            return Err("expected hh:mm:ss");
        }
        Ok(s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortKey {
    pub column: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}
