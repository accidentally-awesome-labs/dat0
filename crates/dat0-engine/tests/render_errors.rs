use dat0_engine::{FilterOp, FilterValue, RenderError, Scalar, Transformation, compile_view_sql};

const BASE: &str = "\"main\".\"t\"";

#[test]
fn empty_in_list_errors() {
    let ops = [Transformation::Filter {
        column: "x".into(),
        op: FilterOp::In,
        value: FilterValue::List { values: vec![] },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap_err(),
        RenderError::EmptyInList
    );
}

#[test]
fn invalid_regex_errors() {
    let ops = [Transformation::Filter {
        column: "name".into(),
        op: FilterOp::Regex,
        value: FilterValue::Scalar {
            value: Scalar::Str("[unclosed".into()),
        },
    }];
    match compile_view_sql(BASE, &ops).unwrap_err() {
        RenderError::InvalidRegex(msg) => {
            assert!(!msg.is_empty(), "InvalidRegex should carry compiler msg");
        }
        other => panic!("expected InvalidRegex, got {:?}", other),
    }
}

#[test]
fn between_without_range_errors() {
    let ops = [Transformation::Filter {
        column: "x".into(),
        op: FilterOp::Between,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    }];
    assert_eq!(
        compile_view_sql(BASE, &ops).unwrap_err(),
        RenderError::MismatchedRange("expected Range")
    );
}

#[test]
fn contains_on_int_errors() {
    let ops = [Transformation::Filter {
        column: "x".into(),
        op: FilterOp::Contains,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    }];
    match compile_view_sql(BASE, &ops).unwrap_err() {
        RenderError::UnsupportedOpForType { op, value_shape } => {
            assert_eq!(op, FilterOp::Contains);
            assert_eq!(value_shape, "Scalar");
        }
        other => panic!("expected UnsupportedOpForType, got {:?}", other),
    }
}

#[test]
fn is_empty_with_scalar_errors() {
    let ops = [Transformation::Filter {
        column: "x".into(),
        op: FilterOp::IsEmpty,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    }];
    match compile_view_sql(BASE, &ops).unwrap_err() {
        RenderError::UnsupportedOpForType { op, .. } => {
            assert_eq!(op, FilterOp::IsEmpty);
        }
        other => panic!("expected UnsupportedOpForType, got {:?}", other),
    }
}

#[test]
fn in_with_scalar_errors() {
    let ops = [Transformation::Filter {
        column: "x".into(),
        op: FilterOp::In,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    }];
    match compile_view_sql(BASE, &ops).unwrap_err() {
        RenderError::UnsupportedOpForType { op, .. } => {
            assert_eq!(op, FilterOp::In);
        }
        other => panic!("expected UnsupportedOpForType, got {:?}", other),
    }
}
