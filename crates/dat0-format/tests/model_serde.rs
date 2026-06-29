use dat0_format::*;

#[test]
fn manifest_round_trips() {
    let m = PackageManifest {
        format_version: FORMAT_VERSION,
        kind: PACKAGE_KIND.into(),
        dat0_version: "0.1.0".into(),
        package_id: uuid::Uuid::nil(),
        workspace_id: uuid::Uuid::nil(),
        created_at: "2026-06-13T00:00:00Z".into(),
        table_count: 1,
        checksums: Default::default(),
    };
    let j = serde_json::to_string(&m).unwrap();
    assert_eq!(m, serde_json::from_str(&j).unwrap());
}

#[test]
fn derivation_tagged_round_trips() {
    let d = Derivation::Sql {
        sql: "SELECT 1".into(),
        parents: vec!["t".into()],
    };
    let j = serde_json::to_value(&d).unwrap();
    assert_eq!(j["kind"], "sql");
    assert_eq!(d, serde_json::from_value(j).unwrap());
}

// ---------------------------------------------------------------------------
// insta proof slice — gate the canonical recipe.json wire format.
// ---------------------------------------------------------------------------

/// PROOF SLICE for the "do-now" UAT-automation tier (research 2026-06-29), 4th
/// target: the `.dat0` package's **canonical** `recipe.json` — the normative
/// on-disk contract (`docs/dat0-format-v1.md`) a non-Rust reader routes on.
///
/// Snapshots the WHOLE `Recipe` for a representative package (one base table +
/// two derived tables exercising BOTH derivation shapes — `Sql` and `Transform`
/// with an embedded op stack) using the exact serializer the Writer applies to
/// `recipe.json` (`serde_json::to_vec_pretty`, writer.rs:84). It therefore gates
/// the full format contract a per-field assert would miss: the `base`/`derived`
/// `TableKind` rename, the `kind: sql`/`kind: transform` `Derivation` tag, the
/// `ColumnFingerprint` `r#type` → `"type"` rename, the `skip_serializing_if`
/// asymmetry (base emits `source_ref` + no `derivation`; derived the reverse),
/// and the embedded `Transformation` wire form.
///
/// `Recipe` carries NO uuids/timestamps/paths (those live in `PackageManifest`,
/// not snapshotted here), so every value is a fixed literal → byte-identical on
/// macOS + Linux CI. Committed `.snap` is the regression baseline; insta never
/// auto-creates snapshots under `CI`.
#[test]
fn recipe_json_wire_format_is_snapshot_gated() {
    use dat0_engine::transform::{
        FilterOp, FilterValue, Scalar, SortDirection, SortKey, Transformation,
    };

    let recipe = Recipe {
        tables: vec![
            RecipeTable {
                id: "t_sales".into(),
                name: "sales".into(),
                kind: TableKind::Base,
                schema: vec![
                    ColumnFingerprint {
                        name: "region".into(),
                        r#type: "VARCHAR".into(),
                    },
                    ColumnFingerprint {
                        name: "amount".into(),
                        r#type: "DOUBLE".into(),
                    },
                ],
                row_count: 1000,
                data: "data/sales.parquet".into(),
                source_ref: Some("src_sales".into()),
                derivation: None,
            },
            RecipeTable {
                id: "t_top".into(),
                name: "top_sales".into(),
                kind: TableKind::Derived,
                schema: vec![ColumnFingerprint {
                    name: "region".into(),
                    r#type: "VARCHAR".into(),
                }],
                row_count: 10,
                data: "data/top_sales.parquet".into(),
                source_ref: None,
                derivation: Some(Derivation::Sql {
                    sql: "SELECT region FROM sales ORDER BY amount DESC LIMIT 10".into(),
                    parents: vec!["t_sales".into()],
                }),
            },
            RecipeTable {
                id: "t_eu".into(),
                name: "eu_sales".into(),
                kind: TableKind::Derived,
                schema: vec![
                    ColumnFingerprint {
                        name: "region".into(),
                        r#type: "VARCHAR".into(),
                    },
                    ColumnFingerprint {
                        name: "amount".into(),
                        r#type: "DOUBLE".into(),
                    },
                ],
                row_count: 250,
                data: "data/eu_sales.parquet".into(),
                source_ref: None,
                derivation: Some(Derivation::Transform {
                    parent: "t_sales".into(),
                    ops: vec![
                        Transformation::Filter {
                            column: "region".into(),
                            op: FilterOp::Eq,
                            value: FilterValue::Scalar {
                                value: Scalar::Str("eu".into()),
                            },
                        },
                        Transformation::Sort {
                            keys: vec![SortKey {
                                column: "amount".into(),
                                direction: SortDirection::Desc,
                            }],
                        },
                    ],
                }),
            },
        ],
    };

    // Mirror the exact serializer the Writer applies to recipe.json (to_vec_pretty).
    let json = serde_json::to_string_pretty(&recipe).unwrap();
    insta::assert_snapshot!("recipe_json_wire_format", json);
}
