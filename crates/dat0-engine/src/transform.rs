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

/// Fixed row-identity column injected at import (T3) and referenced literally by
/// the edit/delete overlay render and by the engine's `ensure_rowid` migration.
///
/// Single source of truth: this is the ONLY definition of the literal
/// `__dat0_rowid`. `render.rs` imports it, the engine's `ensure_rowid` uses it,
/// and it is re-exported as `dat0_engine::ROWID_COL` for the app crate (T5).
///
/// Not quoted when interpolated into SQL: it is an internal sentinel name with
/// no special characters and never collides with user columns (the
/// double-underscore prefix is reserved; a colliding source column is renamed
/// to `<ROWID_COL>__src` by `ensure_rowid`). It maps to the `RowKey::Surrogate`
/// variant below.
pub const ROWID_COL: &str = "__dat0_rowid";

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
    Edit {
        cells: Vec<CellEdit>,
    },
    RowDelete {
        rows: Vec<RowKey>,
    },
    /// Reorder the visible columns. `columns` is the FULL visible source-column
    /// order after this step (excludes `__dat0_rowid` and any deleted columns).
    /// Display-only in Option B: never affects `compile_view_sql` output.
    Reorder {
        columns: Vec<String>,
    },
    /// Rename a column's DISPLAY label. `column` is the stable source identity
    /// (base-table column name); `to` is the new display name. Display-only.
    Rename {
        column: String,
        to: String,
    },
    /// Hide columns from the visible projection. `columns` are source
    /// identities. Display-only; the underlying data column is untouched (so a
    /// pre-existing filter/sort on it still compiles).
    DeleteColumn {
        columns: Vec<String>,
    },
}

/// Partition of a transform stack for live re-import replay (P7c D3).
///
/// Column-keyed ops (`Filter`/`Sort`/`Reorder`/`Rename`/`DeleteColumn`) reference
/// column names and replay cleanly onto a re-imported base. Rowid-keyed ops
/// (`Edit`/`RowDelete`) reference `__dat0_rowid` surrogates ([`ROWID_COL`]) that a
/// re-CTAS regenerates, so they cannot be safely carried over and are dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaySplit {
    pub replayable: Vec<Transformation>,
    pub dropped_edits: usize,
    pub dropped_deletes: usize,
}

impl ReplaySplit {
    pub fn has_dropped(&self) -> bool {
        self.dropped_edits > 0 || self.dropped_deletes > 0
    }
}

/// Split a transform stack into the column-keyed ops that survive a re-import
/// and counts of the rowid-keyed ops that must be discarded.
///
/// The match is intentionally EXHAUSTIVE (one arm per `Transformation` variant,
/// no catch-all) so that adding a new variant forces a compile error here until
/// it is explicitly classified. This is a correctness guard: a rowid-keyed op
/// that silently leaked into `replayable` would re-bind stale `__dat0_rowid`
/// surrogates against a regenerated base and corrupt data.
pub fn split_replayable(ops: &[Transformation]) -> ReplaySplit {
    let mut replayable = Vec::new();
    let mut dropped_edits = 0;
    let mut dropped_deletes = 0;
    for op in ops {
        match op {
            // Rowid-keyed: reference `__dat0_rowid` surrogates (regenerated on
            // re-CTAS) → cannot survive, dropped.
            Transformation::Edit { .. } => dropped_edits += 1,
            Transformation::RowDelete { .. } => dropped_deletes += 1,
            // Column-keyed / structural: reference column names, replay cleanly.
            Transformation::Filter { .. }
            | Transformation::Sort { .. }
            | Transformation::Reorder { .. }
            | Transformation::Rename { .. }
            | Transformation::DeleteColumn { .. } => replayable.push(op.clone()),
        }
    }
    ReplaySplit {
        replayable,
        dropped_edits,
        dropped_deletes,
    }
}

/// A visible column for projection rendering: its stable `source` identity and
/// its current `display` label. Used by export (`render_export_select`) and by
/// the app's grid `ColumnView` fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionColumn {
    pub source: String,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellEdit {
    pub row: RowKey,
    pub column: String,
    pub value: Scalar,
}

/// Row identity for Edit / RowDelete. Tagged so P7 can add a semantic-PK
/// variant (`Pk { col, val }`) with no wire-format break. P4b ships only
/// `Surrogate`, mapping to the `__dat0_rowid` column injected at import.
///
/// SERDE NOTE: Internally-tagged enums (`#[serde(tag = "kind")]`) do NOT
/// support newtype variants wrapping a primitive (e.g. `Surrogate(i64)`).
/// Serde rejects this at compile time: "cannot serialize tagged newtype
/// variant ... containing an integer". The struct-variant form
/// `Surrogate { id: i64 }` produces `{"kind":"surrogate","id":7}` on the
/// wire, which satisfies all three requirements:
///   (1) round-trips correctly,
///   (2) the `row` object carries `"kind":"surrogate"`,
///   (3) forward-compatible for P7 adding `Pk { col: String, val: Scalar }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowKey {
    Surrogate { id: i64 },
    // P7 adds: Pk { col: String, val: Scalar },
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

#[cfg(test)]
mod replay_split_tests {
    use super::*;

    fn edit() -> Transformation {
        Transformation::Edit {
            cells: vec![CellEdit {
                row: RowKey::Surrogate { id: 1 },
                column: "a".into(),
                value: Scalar::Null,
            }],
        }
    }
    fn filter() -> Transformation {
        Transformation::Filter {
            column: "a".into(),
            op: FilterOp::Eq,
            value: FilterValue::Scalar {
                value: Scalar::Str("x".into()),
            },
        }
    }
    fn delete() -> Transformation {
        Transformation::RowDelete {
            rows: vec![RowKey::Surrogate { id: 7 }],
        }
    }

    #[test]
    fn keeps_column_keyed_drops_rowid_keyed() {
        let stack = vec![
            filter(),
            edit(),
            delete(),
            Transformation::Sort { keys: vec![] },
        ];
        let split = split_replayable(&stack);
        assert_eq!(split.replayable.len(), 2, "filter + sort survive");
        assert!(matches!(split.replayable[0], Transformation::Filter { .. }));
        assert!(matches!(split.replayable[1], Transformation::Sort { .. }));
        assert_eq!(split.dropped_edits, 1);
        assert_eq!(split.dropped_deletes, 1);
        assert!(split.has_dropped());
    }

    #[test]
    fn empty_when_nothing_to_drop() {
        let split = split_replayable(&[filter()]);
        assert!(!split.has_dropped());
        assert_eq!(split.replayable.len(), 1);
    }
}
