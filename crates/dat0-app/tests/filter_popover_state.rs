//! Headless state-machine tests for `FilterPopoverState`.
//!
//! No GPUI imports — this file only exercises pure logic. Visual widget mount
//! is T10b.

use dat0_app::view::filter_popover::{ColumnType, FilterPopoverState, supported_ops_for};
use dat0_engine::{FilterOp, FilterValue, Scalar, Transformation};

// ---------------------------------------------------------------------------
// Operator-per-type tests (design §10 canonical mapping)
// ---------------------------------------------------------------------------

#[test]
fn numeric_supported_ops_match_spec() {
    let ops = supported_ops_for(ColumnType::Numeric);
    assert!(ops.contains(&FilterOp::Eq));
    assert!(ops.contains(&FilterOp::Between));
    assert!(ops.contains(&FilterOp::In));
    assert!(ops.contains(&FilterOp::IsEmpty));
    assert!(ops.contains(&FilterOp::IsNotEmpty));
    assert!(
        !ops.contains(&FilterOp::Contains),
        "Contains is string-only"
    );
    assert!(!ops.contains(&FilterOp::Regex), "Regex is string-only");
    assert!(!ops.contains(&FilterOp::IsTrue), "IsTrue is bool-only");
    assert!(!ops.contains(&FilterOp::IsFalse), "IsFalse is bool-only");
}

#[test]
fn string_supported_ops_match_spec() {
    let ops = supported_ops_for(ColumnType::String);
    assert!(ops.contains(&FilterOp::Contains));
    assert!(ops.contains(&FilterOp::Regex));
    assert!(ops.contains(&FilterOp::StartsWith));
    assert!(ops.contains(&FilterOp::EndsWith));
    assert!(ops.contains(&FilterOp::NotContains));
    assert!(!ops.contains(&FilterOp::Between), "Between is numeric/date");
    assert!(!ops.contains(&FilterOp::IsTrue), "IsTrue is bool-only");
    assert!(!ops.contains(&FilterOp::IsFalse), "IsFalse is bool-only");
}

#[test]
fn bool_supported_ops_match_spec() {
    let ops = supported_ops_for(ColumnType::Bool);
    assert!(ops.contains(&FilterOp::IsTrue));
    assert!(ops.contains(&FilterOp::IsFalse));
    assert!(ops.contains(&FilterOp::IsEmpty));
    assert_eq!(
        ops.len(),
        3,
        "bool ops are exactly IsTrue, IsFalse, IsEmpty"
    );
    assert!(!ops.contains(&FilterOp::Between));
    assert!(!ops.contains(&FilterOp::Contains));
}

#[test]
fn date_supported_ops_match_spec() {
    let ops = supported_ops_for(ColumnType::Date);
    assert!(ops.contains(&FilterOp::Eq));
    assert!(ops.contains(&FilterOp::Between));
    assert!(ops.contains(&FilterOp::Lt));
    assert!(
        !ops.contains(&FilterOp::Contains),
        "Contains is string-only"
    );
    assert!(!ops.contains(&FilterOp::IsTrue), "IsTrue is bool-only");
}

#[test]
fn timestamp_supported_ops_match_date() {
    assert_eq!(
        supported_ops_for(ColumnType::Timestamp),
        supported_ops_for(ColumnType::Date),
        "Date and Timestamp share the same operator surface"
    );
}

// ---------------------------------------------------------------------------
// Validity gating tests
// ---------------------------------------------------------------------------

#[test]
fn can_apply_false_when_value_empty_for_unary_op() {
    let mut s = FilterPopoverState::new("price".into(), ColumnType::Numeric);
    s.set_op(FilterOp::Eq);
    s.set_value_text("".into());
    assert!(!s.can_apply());
    s.set_value_text("42".into());
    assert!(s.can_apply());
}

#[test]
fn can_apply_true_for_nullary_ops() {
    let mut s = FilterPopoverState::new("notes".into(), ColumnType::String);
    s.set_op(FilterOp::IsEmpty);
    assert!(s.can_apply(), "IsEmpty is always applicable");

    s.set_op(FilterOp::IsNotEmpty);
    assert!(s.can_apply(), "IsNotEmpty is always applicable");
}

#[test]
fn can_apply_true_for_bool_nullary_ops() {
    let mut s = FilterPopoverState::new("active".into(), ColumnType::Bool);
    s.set_op(FilterOp::IsTrue);
    assert!(s.can_apply());
    s.set_op(FilterOp::IsFalse);
    assert!(s.can_apply());
}

#[test]
fn can_apply_between_requires_both_bounds() {
    let mut s = FilterPopoverState::new("price".into(), ColumnType::Numeric);
    s.set_op(FilterOp::Between);
    assert!(!s.can_apply(), "no bounds");
    s.set_range_lo("10".into());
    assert!(!s.can_apply(), "only lo");
    s.set_range_hi("100".into());
    assert!(s.can_apply(), "both bounds");
}

#[test]
fn can_apply_in_requires_nonempty_list() {
    let mut s = FilterPopoverState::new("city".into(), ColumnType::String);
    s.set_op(FilterOp::In);
    assert!(!s.can_apply(), "empty list");
    s.list_values.push("SF".into());
    assert!(s.can_apply());
}

#[test]
fn can_apply_regex_requires_validity() {
    let mut s = FilterPopoverState::new("name".into(), ColumnType::String);
    s.set_op(FilterOp::Regex);
    // No text yet — regex_valid is None.
    assert!(!s.can_apply());

    s.set_value_text("[unclosed".into());
    s.revalidate_regex();
    assert!(!s.can_apply(), "invalid regex blocks apply");

    s.set_value_text("^A.*".into());
    s.revalidate_regex();
    assert!(s.can_apply(), "valid regex allows apply");
}

// ---------------------------------------------------------------------------
// Build / Transformation emission tests
// ---------------------------------------------------------------------------

#[test]
fn build_emits_typed_filter_for_eq_numeric_int() {
    let mut s = FilterPopoverState::new("price".into(), ColumnType::Numeric);
    s.set_op(FilterOp::Eq);
    s.set_value_text("42".into());
    let t = s.build().unwrap();
    match t {
        Transformation::Filter { column, op, value } => {
            assert_eq!(column, "price");
            assert_eq!(op, FilterOp::Eq);
            assert!(
                matches!(
                    value,
                    FilterValue::Scalar {
                        value: Scalar::Int(42)
                    }
                ),
                "expected Int(42), got {value:?}"
            );
        }
        _ => panic!("expected Filter"),
    }
}

#[test]
fn build_emits_typed_filter_for_between_float() {
    let mut s = FilterPopoverState::new("price".into(), ColumnType::Numeric);
    s.set_op(FilterOp::Between);
    s.set_range_lo("10.5".into());
    s.set_range_hi("99.5".into());
    s.set_range_inclusive(true);
    let t = s.build().unwrap();
    if let Transformation::Filter {
        value: FilterValue::Range { lo, hi, inclusive },
        ..
    } = t
    {
        assert!(matches!(lo, Scalar::Float(_)), "lo should be Float");
        assert!(matches!(hi, Scalar::Float(_)), "hi should be Float");
        assert!(inclusive);
    } else {
        panic!("expected Range filter");
    }
}

#[test]
fn build_emits_typed_filter_for_in_list_string() {
    let mut s = FilterPopoverState::new("city".into(), ColumnType::String);
    s.set_op(FilterOp::In);
    s.list_values = vec!["SF".into(), "NYC".into(), "LA".into()];
    let t = s.build().unwrap();
    if let Transformation::Filter {
        value: FilterValue::List { values: items },
        ..
    } = t
    {
        assert_eq!(items.len(), 3);
        assert!(
            matches!(&items[0], Scalar::Str(s) if s == "SF"),
            "first item should be Str(SF)"
        );
    } else {
        panic!("expected List filter");
    }
}

#[test]
fn build_returns_none_when_can_apply_false() {
    let s = FilterPopoverState::new("price".into(), ColumnType::Numeric);
    // Default op is Eq, value_text is empty → can_apply == false.
    assert!(s.build().is_none());
}

#[test]
fn build_emits_nullary_filter_for_is_empty() {
    let mut s = FilterPopoverState::new("notes".into(), ColumnType::String);
    s.set_op(FilterOp::IsEmpty);
    let t = s.build().unwrap();
    assert!(
        matches!(
            t,
            Transformation::Filter {
                value: FilterValue::None,
                ..
            }
        ),
        "IsEmpty should produce FilterValue::None"
    );
}

// ---------------------------------------------------------------------------
// Pre-population (from_existing) tests
// ---------------------------------------------------------------------------

#[test]
fn from_existing_eq_numeric_pre_populates() {
    let existing = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    };
    let s = FilterPopoverState::from_existing("price".into(), ColumnType::Numeric, &existing);
    assert!(s.pre_populated, "should be marked pre_populated");
    assert_eq!(s.op, FilterOp::Eq);
    assert_eq!(s.value_text, "42");
}

#[test]
fn from_existing_between_pre_populates_both_bounds() {
    let existing = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Between,
        value: FilterValue::Range {
            lo: Scalar::Float(10.0),
            hi: Scalar::Float(99.5),
            inclusive: true,
        },
    };
    let s = FilterPopoverState::from_existing("price".into(), ColumnType::Numeric, &existing);
    assert!(s.pre_populated);
    assert_eq!(s.range_lo, "10");
    assert_eq!(s.range_hi, "99.5");
    assert!(s.range_inclusive);
}

#[test]
fn from_existing_in_list_pre_populates_values() {
    let existing = Transformation::Filter {
        column: "city".into(),
        op: FilterOp::In,
        value: FilterValue::List {
            values: vec![Scalar::Str("SF".into()), Scalar::Str("NYC".into())],
        },
    };
    let s = FilterPopoverState::from_existing("city".into(), ColumnType::String, &existing);
    assert!(s.pre_populated);
    assert_eq!(s.op, FilterOp::In);
    assert_eq!(s.list_values, vec!["SF", "NYC"]);
}

// ---------------------------------------------------------------------------
// ColumnType::from_duckdb_type mapping tests
// ---------------------------------------------------------------------------

#[test]
fn column_type_from_duckdb_recognises_numeric_aliases() {
    assert_eq!(ColumnType::from_duckdb_type("INTEGER"), ColumnType::Numeric);
    assert_eq!(ColumnType::from_duckdb_type("BIGINT"), ColumnType::Numeric);
    assert_eq!(ColumnType::from_duckdb_type("DOUBLE"), ColumnType::Numeric);
    assert_eq!(ColumnType::from_duckdb_type("FLOAT"), ColumnType::Numeric);
    assert_eq!(ColumnType::from_duckdb_type("DECIMAL"), ColumnType::Numeric);
    // Parameterised variant.
    assert_eq!(
        ColumnType::from_duckdb_type("DECIMAL(18,4)"),
        ColumnType::Numeric
    );
    assert_eq!(ColumnType::from_duckdb_type("VARCHAR"), ColumnType::String);
    assert_eq!(ColumnType::from_duckdb_type("TEXT"), ColumnType::String);
    assert_eq!(ColumnType::from_duckdb_type("BOOLEAN"), ColumnType::Bool);
    assert_eq!(ColumnType::from_duckdb_type("BOOL"), ColumnType::Bool);
    assert_eq!(ColumnType::from_duckdb_type("DATE"), ColumnType::Date);
    assert_eq!(
        ColumnType::from_duckdb_type("TIMESTAMP"),
        ColumnType::Timestamp
    );
    // Unknown falls back to String.
    assert_eq!(ColumnType::from_duckdb_type("BLOB"), ColumnType::String);
    assert_eq!(ColumnType::from_duckdb_type("INTERVAL"), ColumnType::String);
}

// ---------------------------------------------------------------------------
// Cancel / clear closure shape tests
// ---------------------------------------------------------------------------

#[test]
fn cancel_is_noop() {
    let s = FilterPopoverState::new("x".into(), ColumnType::Numeric);
    // Must not panic; no state change.
    s.cancel();
}

#[test]
fn clear_filter_true_when_pre_populated() {
    let existing = Transformation::Filter {
        column: "x".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(1),
        },
    };
    let s = FilterPopoverState::from_existing("x".into(), ColumnType::Numeric, &existing);
    assert!(
        s.clear_filter(),
        "clear should signal caller to remove the filter"
    );
}

#[test]
fn clear_filter_false_when_new() {
    let s = FilterPopoverState::new("x".into(), ColumnType::Numeric);
    assert!(
        !s.clear_filter(),
        "no existing filter → nothing to clear at the vm level"
    );
}

// ---------------------------------------------------------------------------
// apply_transformation alias
// ---------------------------------------------------------------------------

#[test]
fn apply_transformation_matches_build() {
    let mut s = FilterPopoverState::new("price".into(), ColumnType::Numeric);
    s.set_op(FilterOp::Eq);
    s.set_value_text("7".into());
    assert_eq!(s.apply_transformation(), s.build());
}

// ---------------------------------------------------------------------------
// T11 IN-list integration tests (against a live DuckDB engine)
// ---------------------------------------------------------------------------

use std::sync::Arc;
use tempfile::TempDir;

use dat0_app::view::distinct_values::{TOP_N, banner_needed, fetch_top_n};
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};

async fn engine_with_cities(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
    let csv = tmp.path().join("cities.csv");
    let mut s = String::from("city,n\n");
    let cities = ["SF", "NYC", "LA", "CHI", "HOU"];
    for (i, city) in cities.iter().enumerate() {
        // Different repeat counts so ORDER BY COUNT is non-trivial.
        for _ in 0..((i + 1) * 10) {
            s.push_str(&format!("{},{}\n", city, i));
        }
    }
    std::fs::write(&csv, s).unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let name = engine.get_tables().await.unwrap()[0].name.clone();
    (Arc::new(engine), name)
}

#[tokio::test]
async fn fetch_top_n_returns_count_descending() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = engine_with_cities(&tmp).await;
    let (values, total) = fetch_top_n(engine, &table, "city").await.unwrap();
    assert_eq!(total, 5, "5 distinct cities");
    assert_eq!(
        values[0].value, "HOU",
        "HOU has the most rows in the fixture"
    );
    assert!(values[0].count > values[1].count);
}

#[tokio::test]
async fn fetch_top_n_caps_at_top_n() {
    // Build a fixture with 70 distinct values.
    let tmp = TempDir::new().unwrap();
    let csv = tmp.path().join("many.csv");
    let mut s = String::from("v\n");
    for i in 0..70 {
        s.push_str(&format!("val_{}\n", i));
    }
    std::fs::write(&csv, s).unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let table = engine.get_tables().await.unwrap()[0].name.clone();

    let (values, total) = fetch_top_n(Arc::new(engine), &table, "v").await.unwrap();
    assert_eq!(total, 70);
    assert_eq!(values.len() as u64, TOP_N, "capped at TOP_N");
}

#[tokio::test]
async fn fetch_top_n_empty_table_returns_zero() {
    let tmp = TempDir::new().unwrap();
    let csv = tmp.path().join("empty.csv");
    // Header-only CSV — no rows.
    std::fs::write(&csv, "city\n").unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let table = engine.get_tables().await.unwrap()[0].name.clone();

    let (values, total) = fetch_top_n(Arc::new(engine), &table, "city").await.unwrap();
    assert_eq!(total, 0, "empty table → 0 distinct");
    assert!(values.is_empty(), "empty table → no candidates");
    assert!(!banner_needed(total), "empty result → no banner");
}

#[tokio::test]
async fn fetch_top_n_banner_triggered_when_over_top_n() {
    // 70 distinct values → total (70) > TOP_N (50) → banner needed.
    let tmp = TempDir::new().unwrap();
    let csv = tmp.path().join("many2.csv");
    let mut s = String::from("v\n");
    for i in 0..70 {
        s.push_str(&format!("val_{}\n", i));
    }
    std::fs::write(&csv, s).unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    engine
        .register_file(&csv, RegisterOpts::default())
        .await
        .unwrap();
    let table = engine.get_tables().await.unwrap()[0].name.clone();

    let (_values, total) = fetch_top_n(Arc::new(engine), &table, "v").await.unwrap();
    assert!(
        banner_needed(total),
        "total ({total}) > TOP_N ({TOP_N}) → banner required"
    );
}

#[test]
fn manual_entry_append_deduplicates() {
    // Simulate the manual-entry append logic: value is added once only.
    let mut list_values: Vec<String> = vec!["SF".into(), "NYC".into()];
    let new_val = "LA".to_string();
    let dup_val = "SF".to_string();

    // Append new value — should succeed.
    if !list_values.contains(&new_val) {
        list_values.push(new_val.clone());
    }
    assert!(list_values.contains(&new_val), "LA should be appended");

    // Append duplicate — should be a no-op.
    let before_len = list_values.len();
    if !list_values.contains(&dup_val) {
        list_values.push(dup_val.clone());
    }
    assert_eq!(
        list_values.len(),
        before_len,
        "duplicate SF should not be added again"
    );
}
