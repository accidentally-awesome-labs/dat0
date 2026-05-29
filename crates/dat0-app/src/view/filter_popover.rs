//! Compact-inline filter popover state machine + dispatch.
//!
//! This module owns:
//! - `ColumnType` — coarse-grained column type derived from DuckDB type literals.
//! - `supported_ops_for` — operator surface per `ColumnType` (presentation order).
//! - `FilterPopoverState` — all mutable state for an open filter popover.
//!
//! **No GPUI widget mount in this file.** That is T10b. This file is pure logic
//! so it can be tested headlessly. T10b will import `FilterPopoverState` and
//! wrap it in a GPUI entity.

use dat0_engine::{FilterOp, FilterValue, Scalar, Transformation};
use regex::Regex;

// ---------------------------------------------------------------------------
// ColumnType
// ---------------------------------------------------------------------------

/// Coarse-grained column type derived from DuckDB's type literal.
///
/// The popover uses this to select which operators to show and how to parse
/// user-entered text into a `Scalar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Numeric,
    String,
    Bool,
    Date,
    Timestamp,
}

impl ColumnType {
    /// Map a DuckDB type literal (as returned by `DESCRIBE`) to a
    /// `ColumnType`. Anything unrecognised falls back to `String` — at worst
    /// the user sees Contains/Regex where numeric ops would be better, which is
    /// safe and non-destructive.
    pub fn from_duckdb_type(s: &str) -> Self {
        let upper = s.to_ascii_uppercase();
        // Trim any parameter suffix, e.g. "DECIMAL(18,4)" → "DECIMAL".
        let base = upper.split('(').next().unwrap_or(&upper).trim();
        match base {
            "INTEGER" | "INT" | "INT4" | "INT2" | "INT8" | "BIGINT" | "SMALLINT" | "TINYINT"
            | "UBIGINT" | "UINTEGER" | "USMALLINT" | "UTINYINT" | "HUGEINT" | "UHUGEINT"
            | "FLOAT" | "REAL" | "DOUBLE" | "DECIMAL" | "NUMERIC" | "FLOAT4" | "FLOAT8" => {
                Self::Numeric
            }
            "BOOLEAN" | "BOOL" => Self::Bool,
            "DATE" => Self::Date,
            "TIMESTAMP"
            | "DATETIME"
            | "TIMESTAMP_S"
            | "TIMESTAMP_MS"
            | "TIMESTAMP_NS"
            | "TIMESTAMP WITH TIME ZONE"
            | "TIMESTAMPTZ" => Self::Timestamp,
            _ => Self::String,
        }
    }
}

// ---------------------------------------------------------------------------
// Operator surface
// ---------------------------------------------------------------------------

/// Ordered operator list for a given `ColumnType`.
///
/// The order is the presentation order in the popover dropdown.
///
/// Design §10 canonical mapping:
/// - Numeric: Eq Neq Lt Lte Gt Gte Between In IsEmpty IsNotEmpty
/// - String: Eq Neq Contains NotContains StartsWith EndsWith In Regex IsEmpty IsNotEmpty
/// - Bool: IsTrue IsFalse IsEmpty (exactly 3)
/// - Date/Timestamp: Eq Neq Lt Lte Gt Gte Between In IsEmpty IsNotEmpty
pub fn supported_ops_for(ct: ColumnType) -> Vec<FilterOp> {
    use FilterOp::*;
    match ct {
        ColumnType::Numeric => vec![Eq, Neq, Lt, Lte, Gt, Gte, Between, In, IsEmpty, IsNotEmpty],
        ColumnType::String => vec![
            Eq,
            Neq,
            Contains,
            NotContains,
            StartsWith,
            EndsWith,
            In,
            Regex,
            IsEmpty,
            IsNotEmpty,
        ],
        ColumnType::Bool => vec![IsTrue, IsFalse, IsEmpty],
        ColumnType::Date | ColumnType::Timestamp => {
            vec![Eq, Neq, Lt, Lte, Gt, Gte, Between, In, IsEmpty, IsNotEmpty]
        }
    }
}

// ---------------------------------------------------------------------------
// FilterPopoverState
// ---------------------------------------------------------------------------

/// All mutable state for an open filter popover.
///
/// The state machine is intentionally free of GPUI. T10b mounts the visible
/// widgets and drives mutations through the methods below.
///
/// **Field visibility**: fields are `pub` so T10b can read them for rendering.
/// All mutations go through named methods.
#[derive(Debug, Clone)]
pub struct FilterPopoverState {
    /// Column being filtered.
    pub column: String,
    /// Coarse type of the column.
    pub column_type: ColumnType,
    /// Currently selected operator.
    pub op: FilterOp,
    /// Single-value text input (used by all non-Between, non-In, non-nullary ops).
    pub value_text: String,
    /// Between lower bound text.
    pub range_lo: String,
    /// Between upper bound text.
    pub range_hi: String,
    /// Whether the Between range is inclusive.
    pub range_inclusive: bool,
    /// IN-list values. Populated via the T11 distinct-values panel; T10 only
    /// carries the buffer that T11 will write to.
    pub list_values: Vec<String>,
    /// `Some(true)` when value_text is a valid regex; `Some(false)` when not;
    /// `None` when op != Regex.
    pub regex_valid: Option<bool>,
    /// True when the popover was opened on a column that already has an active
    /// filter (edit flow).
    pub pre_populated: bool,
}

impl FilterPopoverState {
    // --- Constructors ---

    /// Create a fresh popover for `column` with no pre-existing filter.
    ///
    /// The operator defaults to the first one in `supported_ops_for(column_type)`.
    pub fn new(column: String, column_type: ColumnType) -> Self {
        let op = *supported_ops_for(column_type)
            .first()
            .expect("every ColumnType has at least one operator");
        Self {
            column,
            column_type,
            op,
            value_text: String::new(),
            range_lo: String::new(),
            range_hi: String::new(),
            range_inclusive: true,
            list_values: Vec::new(),
            regex_valid: None,
            pre_populated: false,
        }
    }

    /// Create a popover pre-populated from an existing `Transformation::Filter`.
    ///
    /// Used when the user clicks the funnel icon on a column that already has
    /// an active filter (edit-existing flow, design §6 "re-open").
    ///
    /// `existing` must be a `Transformation::Filter` for `column`; other
    /// variants are silently ignored (the popover opens as if new).
    pub fn from_existing(
        column: String,
        column_type: ColumnType,
        existing: &Transformation,
    ) -> Self {
        let mut state = Self::new(column, column_type);
        state.pre_populated = true;
        if let Transformation::Filter { op, value, .. } = existing {
            state.op = *op;
            match value {
                FilterValue::Scalar { value: s } => {
                    state.value_text = scalar_to_display(s);
                }
                FilterValue::Range { lo, hi, inclusive } => {
                    state.range_lo = scalar_to_display(lo);
                    state.range_hi = scalar_to_display(hi);
                    state.range_inclusive = *inclusive;
                }
                FilterValue::List { values: items } => {
                    state.list_values = items.iter().map(scalar_to_display).collect();
                }
                FilterValue::None => {}
            }
        }
        state
    }

    // --- Mutations ---

    /// Set the operator, clearing value fields that don't apply to the new op.
    pub fn set_op(&mut self, op: FilterOp) {
        self.op = op;
        // Clear regex validity when switching away from Regex.
        if op != FilterOp::Regex {
            self.regex_valid = None;
        }
    }

    /// Update the single-value text field. Caller should follow with
    /// `revalidate_regex()` when op == Regex.
    pub fn set_value_text(&mut self, text: String) {
        self.value_text = text;
    }

    /// Update the Between lower bound text.
    pub fn set_range_lo(&mut self, text: String) {
        self.range_lo = text;
    }

    /// Update the Between upper bound text.
    pub fn set_range_hi(&mut self, text: String) {
        self.range_hi = text;
    }

    /// Toggle Between inclusiveness.
    pub fn set_range_inclusive(&mut self, inclusive: bool) {
        self.range_inclusive = inclusive;
    }

    /// Recompute regex validity from `value_text`. Sets `regex_valid` to
    /// `Some(true/false)` when op == Regex; clears it otherwise.
    pub fn revalidate_regex(&mut self) {
        if self.op == FilterOp::Regex {
            self.regex_valid = Some(Regex::new(&self.value_text).is_ok());
        } else {
            self.regex_valid = None;
        }
    }

    // --- Validity / gating ---

    /// Whether the Apply button should be enabled.
    ///
    /// - Nullary ops (IsEmpty, IsNotEmpty, IsTrue, IsFalse): always true.
    /// - Between: both bounds must be non-empty.
    /// - In: at least one item in `list_values`.
    /// - Regex: `regex_valid == Some(true)`.
    /// - All others: `value_text` must be non-empty.
    pub fn can_apply(&self) -> bool {
        match self.op {
            FilterOp::IsEmpty | FilterOp::IsNotEmpty | FilterOp::IsTrue | FilterOp::IsFalse => true,
            FilterOp::Between => !self.range_lo.is_empty() && !self.range_hi.is_empty(),
            FilterOp::In => !self.list_values.is_empty(),
            FilterOp::Regex => matches!(self.regex_valid, Some(true)),
            _ => !self.value_text.is_empty(),
        }
    }

    // --- Build / dispatch ---

    /// Build the typed `Transformation::Filter` from the current state.
    ///
    /// Returns `None` if `can_apply()` is false.
    pub fn build(&self) -> Option<Transformation> {
        if !self.can_apply() {
            return None;
        }
        let value = match self.op {
            FilterOp::IsEmpty | FilterOp::IsNotEmpty | FilterOp::IsTrue | FilterOp::IsFalse => {
                FilterValue::None
            }
            FilterOp::Between => FilterValue::Range {
                lo: parse_scalar(&self.range_lo, self.column_type),
                hi: parse_scalar(&self.range_hi, self.column_type),
                inclusive: self.range_inclusive,
            },
            FilterOp::In => FilterValue::List {
                values: self
                    .list_values
                    .iter()
                    .map(|s| parse_scalar(s, self.column_type))
                    .collect(),
            },
            FilterOp::Regex => FilterValue::Scalar {
                value: Scalar::Str(self.value_text.clone()),
            },
            _ => FilterValue::Scalar {
                value: parse_scalar(&self.value_text, self.column_type),
            },
        };
        Some(Transformation::Filter {
            column: self.column.clone(),
            op: self.op,
            value,
        })
    }

    /// Closure shape: Apply.
    ///
    /// Builds the `Transformation` and hands it to the caller. Returns the
    /// built `Transformation` so the caller can decide whether to call
    /// `vm.apply(t)` (new filter) or `vm.replace_at_cursor(t)` (edit flow).
    /// Returns `None` when `can_apply()` is false.
    pub fn apply_transformation(&self) -> Option<Transformation> {
        self.build()
    }

    /// Closure shape: Cancel.
    ///
    /// No state change — the caller simply closes the popover entity. This
    /// method is a named marker so T10b can call it explicitly rather than
    /// inlining the no-op.
    pub fn cancel(&self) {
        // Pure no-op: closing is the caller's responsibility (T10b entity drop).
    }

    /// Closure shape: Clear.
    ///
    /// Returns `true` if there was a pre-populated filter to clear (i.e., the
    /// caller should call `vm.replace_at_cursor` / remove the filter from the
    /// stack). Returns `false` if the popover was opened on a column with no
    /// existing filter (nothing to clear).
    pub fn clear_filter(&self) -> bool {
        self.pre_populated
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Parse a user-entered string into a `Scalar` for the given `ColumnType`.
///
/// For Numeric: tries `i64` first, then `f64`, falls back to `Str` (render
/// will surface a type error rather than panicking here).
/// For Date / Timestamp: stores as-is; DuckDB validates on render.
fn parse_scalar(text: &str, ct: ColumnType) -> Scalar {
    match ct {
        ColumnType::Numeric => {
            if let Ok(i) = text.parse::<i64>() {
                Scalar::Int(i)
            } else if let Ok(f) = text.parse::<f64>() {
                Scalar::Float(f)
            } else {
                Scalar::Str(text.to_string())
            }
        }
        ColumnType::String => Scalar::Str(text.to_string()),
        ColumnType::Bool => match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "t" => Scalar::Bool(true),
            "false" | "0" | "f" => Scalar::Bool(false),
            _ => Scalar::Str(text.to_string()),
        },
        ColumnType::Date => Scalar::Date(text.to_string()),
        ColumnType::Timestamp => Scalar::Timestamp(text.to_string()),
    }
}

/// Format a `Scalar` back to a display string for pre-population.
fn scalar_to_display(s: &Scalar) -> String {
    match s {
        Scalar::Null => "NULL".to_string(),
        Scalar::Bool(b) => b.to_string(),
        Scalar::Int(i) => i.to_string(),
        Scalar::Float(f) => f.to_string(),
        Scalar::Str(s) => s.clone(),
        Scalar::Date(s) => s.clone(),
        Scalar::Timestamp(s) => s.clone(),
    }
}
