//! Golden-string tests for compile_view_sql. Each test exercises one Transformation
//! shape against a fixed base table name and asserts the exact SQL emitted.

use dat0_engine::{
    FilterOp, FilterValue, Scalar, SortDirection, SortKey, Transformation, compile_view_sql,
};

const BASE: &str = "\"main\".\"orders\"";

#[test]
fn filter_eq_int_renders_equality() {
    let ops = [Transformation::Filter {
        column: "age".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    }];
    let sql = compile_view_sql(BASE, &ops).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM \"main\".\"orders\" WHERE (\"age\" = 42)"
    );
}

#[test]
fn filter_eq_null_rewrites_to_is_null() {
    let ops = [Transformation::Filter {
        column: "deleted_at".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Null,
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"deleted_at\" IS NULL)"
    );
}

#[test]
fn filter_neq_null_rewrites_to_is_not_null() {
    let ops = [Transformation::Filter {
        column: "deleted_at".into(),
        op: FilterOp::Neq,
        value: FilterValue::Scalar {
            value: Scalar::Null,
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"deleted_at\" IS NOT NULL)"
    );
}

#[test]
fn filter_between_float_inclusive_renders_between() {
    let ops = [Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Between,
        value: FilterValue::Range {
            lo: Scalar::Float(10.00),
            hi: Scalar::Float(99.99),
            inclusive: true,
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"price\" BETWEEN 10.0 AND 99.99)"
    );
}

#[test]
fn filter_between_exclusive_renders_pair() {
    let ops = [Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Between,
        value: FilterValue::Range {
            lo: Scalar::Float(10.0),
            hi: Scalar::Float(100.0),
            inclusive: false,
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"price\" > 10.0 AND \"price\" < 100.0)"
    );
}

#[test]
fn filter_in_string_list() {
    let ops = [Transformation::Filter {
        column: "city".into(),
        op: FilterOp::In,
        value: FilterValue::List {
            values: vec![
                Scalar::Str("SF".into()),
                Scalar::Str("NYC".into()),
                Scalar::Str("LA".into()),
            ],
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"city\" IN ('SF', 'NYC', 'LA'))"
    );
}

#[test]
fn filter_contains_escapes_like_metachars() {
    let ops = [Transformation::Filter {
        column: "name".into(),
        op: FilterOp::Contains,
        value: FilterValue::Scalar {
            value: Scalar::Str("5% off".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"name\" LIKE '%5\\% off%' ESCAPE '\\')"
    );
}

#[test]
fn filter_starts_with() {
    let ops = [Transformation::Filter {
        column: "name".into(),
        op: FilterOp::StartsWith,
        value: FilterValue::Scalar {
            value: Scalar::Str("A".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"name\" LIKE 'A%' ESCAPE '\\')"
    );
}

#[test]
fn filter_regex_renders_regexp_matches() {
    let ops = [Transformation::Filter {
        column: "name".into(),
        op: FilterOp::Regex,
        value: FilterValue::Scalar {
            value: Scalar::Str("^A.*".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE regexp_matches(\"name\", '^A.*')"
    );
}

#[test]
fn filter_is_empty_renders_is_null() {
    let ops = [Transformation::Filter {
        column: "notes".into(),
        op: FilterOp::IsEmpty,
        value: FilterValue::None,
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"notes\" IS NULL)"
    );
}

#[test]
fn filter_is_true() {
    let ops = [Transformation::Filter {
        column: "active".into(),
        op: FilterOp::IsTrue,
        value: FilterValue::None,
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"active\" = TRUE)"
    );
}

#[test]
fn sort_single_key_asc() {
    let ops = [Transformation::Sort {
        keys: vec![SortKey {
            column: "price".into(),
            direction: SortDirection::Asc,
        }],
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" ORDER BY \"price\" ASC"
    );
}

#[test]
fn sort_multi_key() {
    let ops = [Transformation::Sort {
        keys: vec![
            SortKey {
                column: "city".into(),
                direction: SortDirection::Asc,
            },
            SortKey {
                column: "price".into(),
                direction: SortDirection::Desc,
            },
        ],
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" ORDER BY \"city\" ASC, \"price\" DESC"
    );
}

#[test]
fn filter_plus_sort_combined() {
    let ops = [
        Transformation::Filter {
            column: "price".into(),
            op: FilterOp::Gte,
            value: FilterValue::Scalar {
                value: Scalar::Float(10.00),
            },
        },
        Transformation::Sort {
            keys: vec![SortKey {
                column: "ts".into(),
                direction: SortDirection::Desc,
            }],
        },
    ];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"price\" >= 10.0) ORDER BY \"ts\" DESC"
    );
}

#[test]
fn multiple_filters_and_joined() {
    let ops = [
        Transformation::Filter {
            column: "price".into(),
            op: FilterOp::Gte,
            value: FilterValue::Scalar {
                value: Scalar::Float(10.00),
            },
        },
        Transformation::Filter {
            column: "city".into(),
            op: FilterOp::Eq,
            value: FilterValue::Scalar {
                value: Scalar::Str("SF".into()),
            },
        },
    ];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"price\" >= 10.0) AND (\"city\" = 'SF')"
    );
}

#[test]
fn last_sort_wins_defensive() {
    let ops = [
        Transformation::Sort {
            keys: vec![SortKey {
                column: "a".into(),
                direction: SortDirection::Asc,
            }],
        },
        Transformation::Sort {
            keys: vec![SortKey {
                column: "b".into(),
                direction: SortDirection::Desc,
            }],
        },
    ];
    // UI should never produce this, but render must be deterministic.
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" ORDER BY \"b\" DESC"
    );
}

#[test]
fn empty_ops_returns_select_star() {
    let ops: [Transformation; 0] = [];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\""
    );
}

#[test]
fn string_value_escapes_single_quote() {
    let ops = [Transformation::Filter {
        column: "name".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Str("O'Brien".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"name\" = 'O''Brien')"
    );
}

#[test]
fn column_with_quote_in_name() {
    let ops = [Transformation::Filter {
        column: "weird\"col".into(),
        op: FilterOp::IsNotEmpty,
        value: FilterValue::None,
    }];
    // quote_ident doubles embedded quotes; the column becomes "weird""col"
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"weird\"\"col\" IS NOT NULL)"
    );
}

#[test]
fn date_scalar_renders_as_date_literal() {
    let ops = [Transformation::Filter {
        column: "created".into(),
        op: FilterOp::Lt,
        value: FilterValue::Scalar {
            value: Scalar::Date("2026-01-01".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"created\" < DATE '2026-01-01')"
    );
}

#[test]
fn timestamp_scalar_renders_as_timestamp_literal() {
    let ops = [Transformation::Filter {
        column: "ts".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Timestamp("2026-01-01 00:00:00".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"ts\" >= TIMESTAMP '2026-01-01 00:00:00')"
    );
}

#[test]
fn float_precision_round_trip() {
    let ops = [Transformation::Filter {
        column: "x".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Float(0.1 + 0.2),
        },
    }];
    // {:?} format preserves the exact f64 representation; this is critical
    // for replay-on-new-source determinism.
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"x\" = 0.30000000000000004)"
    );
}

#[test]
fn filter_ends_with_renders_anchored_like() {
    let ops = [Transformation::Filter {
        column: "name".into(),
        op: FilterOp::EndsWith,
        value: FilterValue::Scalar {
            value: Scalar::Str("son".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"name\" LIKE '%son' ESCAPE '\\')"
    );
}

#[test]
fn filter_not_contains_renders_not_like() {
    let ops = [Transformation::Filter {
        column: "name".into(),
        op: FilterOp::NotContains,
        value: FilterValue::Scalar {
            value: Scalar::Str("foo".into()),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"name\" NOT LIKE '%foo%' ESCAPE '\\')"
    );
}

#[test]
fn empty_sort_keys_emits_no_order_by() {
    let ops = [Transformation::Sort { keys: vec![] }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\""
    );
}

#[test]
fn empty_sort_keys_combined_with_filter_emits_no_order_by() {
    let ops = [
        Transformation::Filter {
            column: "x".into(),
            op: FilterOp::Eq,
            value: FilterValue::Scalar {
                value: Scalar::Int(1),
            },
        },
        Transformation::Sort { keys: vec![] },
    ];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap(),
        "SELECT * FROM \"main\".\"orders\" WHERE (\"x\" = 1)"
    );
}
