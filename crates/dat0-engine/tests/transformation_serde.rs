//! Round-trip + wire-format tests for `Transformation` and its sub-types.
//!
//! Catches any future change to the serde wire format that would break P7
//! transforms.jsonl + P8 .dat0 replay.
//!
//! Wire format snapshot (design §3 amended 2026-05-29):
//! - `FilterValue` internally tagged on `"kind"`
//! - `Scalar` adjacent-tagged on `"type"`/`"value"`
//!
//! Tests 1-14: round-trip coverage per variant.
//! Tests 15-16: regression guards for the formerly-colliding pairs (PD-014).

use dat0_engine::{FilterOp, FilterValue, Scalar, SortDirection, SortKey, Transformation};

fn round_trip(t: Transformation) -> Transformation {
    let json = serde_json::to_string(&t).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

// ── 1. Filter Eq Int ─────────────────────────────────────────────────────────

#[test]
fn filter_eq_int_round_trip() {
    let t = Transformation::Filter {
        column: "age".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Int(42),
        },
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 2. Filter Between Float ───────────────────────────────────────────────────

#[test]
fn filter_between_float_round_trip() {
    let t = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Between,
        value: FilterValue::Range {
            lo: Scalar::Float(10.00),
            hi: Scalar::Float(99.99),
            inclusive: true,
        },
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 3. Filter IN string list ──────────────────────────────────────────────────

#[test]
fn filter_in_string_list_round_trip() {
    let t = Transformation::Filter {
        column: "city".into(),
        op: FilterOp::In,
        value: FilterValue::List {
            values: vec![
                Scalar::Str("SF".into()),
                Scalar::Str("NYC".into()),
                Scalar::Str("LA".into()),
            ],
        },
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 4. Filter Regex ────────────────────────────────────────────────────────────

#[test]
fn filter_regex_round_trip() {
    let t = Transformation::Filter {
        column: "name".into(),
        op: FilterOp::Regex,
        value: FilterValue::Scalar {
            value: Scalar::Str("^A.*".into()),
        },
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 5. Filter IsEmpty (nullary) ───────────────────────────────────────────────

#[test]
fn filter_is_empty_nullary_round_trip() {
    let t = Transformation::Filter {
        column: "notes".into(),
        op: FilterOp::IsEmpty,
        value: FilterValue::None,
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 6. Filter IsTrue (nullary) ────────────────────────────────────────────────

#[test]
fn filter_is_true_bool_round_trip() {
    let t = Transformation::Filter {
        column: "active".into(),
        op: FilterOp::IsTrue,
        value: FilterValue::None,
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 7. Filter Lt Date ─────────────────────────────────────────────────────────

#[test]
fn filter_lt_date_round_trip() {
    let t = Transformation::Filter {
        column: "created".into(),
        op: FilterOp::Lt,
        value: FilterValue::Scalar {
            value: Scalar::Date("2026-01-01".into()),
        },
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 8. Filter Gte Timestamp ───────────────────────────────────────────────────

#[test]
fn filter_gte_timestamp_round_trip() {
    let t = Transformation::Filter {
        column: "ts".into(),
        op: FilterOp::Gte,
        value: FilterValue::Scalar {
            value: Scalar::Timestamp("2026-01-01 00:00:00".into()),
        },
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 9. Sort single key ────────────────────────────────────────────────────────

#[test]
fn sort_single_key_round_trip() {
    let t = Transformation::Sort {
        keys: vec![SortKey {
            column: "price".into(),
            direction: SortDirection::Desc,
        }],
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 10. Sort multi-key ────────────────────────────────────────────────────────

#[test]
fn sort_multi_key_round_trip() {
    let t = Transformation::Sort {
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
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 11. Filter Eq Null ────────────────────────────────────────────────────────

#[test]
fn filter_eq_null_round_trip() {
    // NB: render layer rewrites Eq+Null → IS NULL; serde-level round-trip
    // should still preserve the user-authored shape.
    let t = Transformation::Filter {
        column: "deleted_at".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Null,
        },
    };
    assert_eq!(round_trip(t.clone()), t);
}

// ── 12. Wire format matches design §3 spec example ────────────────────────────

#[test]
fn wire_format_matches_design_spec() {
    // Design §3 Between Float example (amended 2026-05-29):
    // { "kind": "filter", "column": "price", "op": "between",
    //   "value": { "kind": "range",
    //              "lo":   { "type": "float", "value": 10.0 },
    //              "hi":   { "type": "float", "value": 99.99 },
    //              "inclusive": true } }
    let t = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Between,
        value: FilterValue::Range {
            lo: Scalar::Float(10.00),
            hi: Scalar::Float(99.99),
            inclusive: true,
        },
    };
    let json = serde_json::to_string(&t).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Outer transformation shape
    assert_eq!(v["kind"], "filter", "outer kind: {json}");
    assert_eq!(v["column"], "price", "column: {json}");
    assert_eq!(v["op"], "between", "op: {json}");

    // FilterValue internally-tagged shape
    assert_eq!(v["value"]["kind"], "range", "value.kind: {json}");
    assert_eq!(v["value"]["inclusive"], true, "value.inclusive: {json}");

    // Scalar adjacent-tagged shape inside lo/hi
    assert_eq!(v["value"]["lo"]["type"], "float", "lo.type: {json}");
    // serde_json emits 10.0 for f64(10.00) — lossless f64 round-trip
    assert_eq!(v["value"]["lo"]["value"], 10.0, "lo.value: {json}");
    assert_eq!(v["value"]["hi"]["type"], "float", "hi.type: {json}");
    assert_eq!(v["value"]["hi"]["value"], 99.99, "hi.value: {json}");
}

// ── 13. Validate date format ──────────────────────────────────────────────────

#[test]
fn validate_date_rejects_bad_shape() {
    assert!(Scalar::validate_date("2026-1-1").is_err());
    assert!(Scalar::validate_date("2026/01/01").is_err());
    assert!(Scalar::validate_date("2026-01-01").is_ok());
}

// ── 14. Validate timestamp format ─────────────────────────────────────────────

#[test]
fn validate_timestamp_accepts_iso_8601() {
    assert!(Scalar::validate_timestamp("2026-01-01 00:00:00").is_ok());
    assert!(Scalar::validate_timestamp("2026-01-01T00:00:00").is_ok());
    assert!(Scalar::validate_timestamp("2026-01-01").is_err()); // too short
}

// ── 15. PD-014 regression: Str vs Date produce distinct wire shapes ───────────

#[test]
fn filter_str_value_vs_date_value_are_distinct() {
    // Previously (untagged design): both Scalar::Str("2026-01-01") and
    // Scalar::Date("2026-01-01") would serialize as the plain JSON string
    // "2026-01-01", making round-trip ambiguous. With adjacent tagging they
    // produce distinct objects — { "type":"str",... } vs { "type":"date",... }.
    let t_str = Transformation::Filter {
        column: "label".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Str("2026-01-01".into()),
        },
    };
    let t_date = Transformation::Filter {
        column: "label".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Date("2026-01-01".into()),
        },
    };

    let json_str = serde_json::to_string(&t_str).unwrap();
    let json_date = serde_json::to_string(&t_date).unwrap();

    // Wire shapes must differ
    assert_ne!(
        json_str, json_date,
        "Str and Date must produce distinct JSON"
    );

    // Each must round-trip back to its original Rust value (not cross-convert)
    let rt_str: Transformation = serde_json::from_str(&json_str).unwrap();
    let rt_date: Transformation = serde_json::from_str(&json_date).unwrap();
    assert_eq!(rt_str, t_str, "Str round-trip failed");
    assert_eq!(rt_date, t_date, "Date round-trip failed");

    // Structural check: "type" field carries the discriminator.
    // JSON path: transformation["value"] = FilterValue object { "kind":"scalar", "value":Scalar }
    //            transformation["value"]["value"] = Scalar object { "type":"str"|"date", "value":... }
    let v_str: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    let v_date: serde_json::Value = serde_json::from_str(&json_date).unwrap();
    assert_eq!(
        v_str["value"]["kind"], "scalar",
        "FilterValue kind for Str: {json_str}"
    );
    assert_eq!(
        v_date["value"]["kind"], "scalar",
        "FilterValue kind for Date: {json_date}"
    );
    assert_eq!(
        v_str["value"]["value"]["type"], "str",
        "Scalar type for Str: {json_str}"
    );
    assert_eq!(
        v_date["value"]["value"]["type"], "date",
        "Scalar type for Date: {json_date}"
    );
}

// ── 17. DerivedOrigin::Transform round-trips with Transformation ops ──────────

#[test]
fn derived_origin_transform_round_trips() {
    use dat0_engine::DerivedOrigin;

    let o = DerivedOrigin::Transform {
        parent: "orders".into(),
        ops: vec![
            Transformation::Filter {
                column: "price".into(),
                op: FilterOp::Gte,
                value: FilterValue::Scalar {
                    value: Scalar::Float(10.0),
                },
            },
            Transformation::Sort {
                keys: vec![SortKey {
                    column: "ts".into(),
                    direction: SortDirection::Desc,
                }],
            },
        ],
    };
    let json = serde_json::to_string(&o).unwrap();
    let back: DerivedOrigin = serde_json::from_str(&json).unwrap();
    let DerivedOrigin::Transform { parent, ops } = back else {
        panic!("expected Transform variant after round-trip");
    };
    assert_eq!(parent, "orders");
    assert_eq!(ops.len(), 2);
}

// ── 16. PD-014 regression: FilterValue::None vs Scalar::Null are distinct ─────

#[test]
fn filter_none_vs_scalar_null_are_distinct() {
    // Previously (untagged design): FilterValue::None and
    // FilterValue::Scalar(Scalar::Null) would both serialize as JSON null,
    // making them indistinguishable on the wire. With internal tagging on
    // FilterValue, None becomes { "kind":"none" } and Scalar::Null becomes
    // { "kind":"scalar", "value":{ "type":"null" } }.
    let t_none = Transformation::Filter {
        column: "notes".into(),
        op: FilterOp::IsEmpty,
        value: FilterValue::None,
    };
    let t_null = Transformation::Filter {
        column: "notes".into(),
        op: FilterOp::Eq,
        value: FilterValue::Scalar {
            value: Scalar::Null,
        },
    };

    let json_none = serde_json::to_string(&t_none).unwrap();
    let json_null = serde_json::to_string(&t_null).unwrap();

    // Wire shapes must differ
    assert_ne!(
        json_none, json_null,
        "None and Scalar::Null must produce distinct JSON"
    );

    // Each must round-trip back to its original Rust value
    let rt_none: Transformation = serde_json::from_str(&json_none).unwrap();
    let rt_null: Transformation = serde_json::from_str(&json_null).unwrap();
    assert_eq!(rt_none, t_none, "FilterValue::None round-trip failed");
    assert_eq!(
        rt_null, t_null,
        "FilterValue::Scalar(Null) round-trip failed"
    );

    // Structural check: "kind" field carries the discriminator
    let v_none: serde_json::Value = serde_json::from_str(&json_none).unwrap();
    let v_null: serde_json::Value = serde_json::from_str(&json_null).unwrap();
    assert_eq!(v_none["value"]["kind"], "none");
    assert_eq!(v_null["value"]["kind"], "scalar");
}
