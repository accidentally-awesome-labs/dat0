//! Replay tests (T6): pure `compat_check` unit tests + an engine-backed
//! end-to-end replay of a derived recipe against a NEW, larger source.

use std::collections::HashMap;

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use dat0_format::replay::*;
use dat0_format::*;

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

#[test]
fn compat_check_passes_when_referenced_columns_present() {
    let needed = vec![ColumnFingerprint {
        name: "id".into(),
        r#type: "BIGINT".into(),
    }];
    let provided = vec![
        ColumnFingerprint {
            name: "id".into(),
            r#type: "BIGINT".into(),
        },
        ColumnFingerprint {
            name: "extra".into(),
            r#type: "VARCHAR".into(),
        }, // ignored
    ];
    assert!(compat_check(&needed, &provided).is_ok());
}

#[test]
fn compat_check_fails_on_missing_referenced_column() {
    let needed = vec![ColumnFingerprint {
        name: "id".into(),
        r#type: "BIGINT".into(),
    }];
    let provided = vec![ColumnFingerprint {
        name: "other".into(),
        r#type: "BIGINT".into(),
    }];
    let err = compat_check(&needed, &provided).unwrap_err();
    assert!(matches!(err, FormatError::SchemaIncompatible(_)));
}

#[test]
fn compat_check_passes_on_widening_int_family_and_decimal() {
    // INT-width family is mutually compatible; DECIMAL precision/scale relaxes.
    let needed = vec![
        ColumnFingerprint {
            name: "id".into(),
            r#type: "INTEGER".into(),
        },
        ColumnFingerprint {
            name: "amt".into(),
            r#type: "DECIMAL(10,2)".into(),
        },
        ColumnFingerprint {
            name: "label".into(),
            r#type: "VARCHAR".into(),
        },
    ];
    let provided = vec![
        ColumnFingerprint {
            name: "id".into(),
            r#type: "BIGINT".into(),
        },
        ColumnFingerprint {
            name: "amt".into(),
            r#type: "DECIMAL(18,4)".into(),
        },
        ColumnFingerprint {
            name: "label".into(),
            r#type: "TEXT".into(),
        },
    ];
    assert!(compat_check(&needed, &provided).is_ok());
}

#[test]
fn compat_check_fails_on_real_type_mismatch() {
    let needed = vec![ColumnFingerprint {
        name: "id".into(),
        r#type: "BIGINT".into(),
    }];
    let provided = vec![ColumnFingerprint {
        name: "id".into(),
        r#type: "VARCHAR".into(),
    }];
    let err = compat_check(&needed, &provided).unwrap_err();
    match err {
        FormatError::SchemaIncompatible(msg) => {
            assert!(msg.contains("id"), "msg should name the column: {msg}");
        }
        other => panic!("expected SchemaIncompatible, got {other:?}"),
    }
}

/// End-to-end: build a package with `sales` (base, from CSV → has a
/// `PackageSource`) + `monthly` (derived SQL over sales). Replay against a NEW
/// CSV that has one EXTRA column and MORE rows → the derived `monthly`
/// recomputes against the larger source (row_count == 10) and the extra column
/// is ignored by `compat_check`.
#[tokio::test]
async fn replay_reexecutes_derived_against_new_larger_source() {
    let dir = tempfile::tempdir().unwrap();

    // --- 1. Original sales.csv: header `id` + 3 rows. ---
    let orig_csv = dir.path().join("sales.csv");
    std::fs::write(&orig_csv, "id\n1\n2\n3\n").unwrap();

    let e = DuckDBEngine::new(dir.path().join("r.duckdb"), budget()).unwrap();
    e.init().await.unwrap();
    let info = e
        .register_file_as_table(&orig_csv, RegisterOpts::default())
        .await
        .unwrap();
    // Ensure the base table is named `sales` (the derived SQL references it).
    if info.name != "sales" {
        e.rename_table(&info.name, "sales", None).await.unwrap();
    }
    let sales_cols = e.describe_table("sales", None).await.unwrap();
    let sales_fp: Vec<ColumnFingerprint> = sales_cols
        .iter()
        .filter(|c| !c.name.starts_with("__dat0"))
        .map(|c| ColumnFingerprint {
            name: c.name.clone(),
            r#type: c.data_type.clone(),
        })
        .collect();

    // A row-preserving derived table so its row_count tracks the source size
    // (this is what proves replay re-ran the derivation against the new data).
    let monthly_sql = "SELECT id FROM sales WHERE id > 0";
    e.create_table(
        "monthly",
        monthly_sql,
        DerivedOrigin::Sql(monthly_sql.into()),
    )
    .await
    .unwrap();

    // --- 2. Hand-build PackageContents (T5 finding: build from a live engine). ---
    let contents = PackageContents {
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-06-13T00:00:00Z".into(),
        recipe: Recipe {
            tables: vec![
                RecipeTable {
                    id: "t_sales".into(),
                    name: "sales".into(),
                    kind: TableKind::Base,
                    schema: sales_fp.clone(),
                    row_count: 3,
                    data: "data/sales.parquet".into(),
                    source_ref: Some("src_sales".into()),
                    derivation: None,
                },
                RecipeTable {
                    id: "t_monthly".into(),
                    name: "monthly".into(),
                    kind: TableKind::Derived,
                    schema: vec![ColumnFingerprint {
                        name: "id".into(),
                        r#type: "BIGINT".into(),
                    }],
                    row_count: 3,
                    data: "data/monthly.parquet".into(),
                    source_ref: None,
                    derivation: Some(Derivation::Sql {
                        sql: monthly_sql.into(),
                        parents: vec!["sales".into()],
                    }),
                },
            ],
        },
        sources: Sources {
            sources: vec![PackageSource {
                id: "src_sales".into(),
                logical_name: "sales.csv".into(),
                original_uri: orig_csv.display().to_string(),
                schema_fingerprint: sales_fp.clone(),
                content_hash: String::new(),
                row_count: 3,
            }],
        },
        views: Views { views: vec![] },
        queries: Queries { queries: vec![] },
    };

    // --- 3. Write the package, close the engine. ---
    let pkg = dir.path().join("orig.dat0");
    Writer::write(&contents, &e, &pkg).await.unwrap();
    e.close().await.unwrap();

    // --- 4. Parse it back; build a NEW sales.csv with an extra column + 10 rows. ---
    let parsed = Reader::open(&pkg).unwrap();

    let new_csv = dir.path().join("new").join("sales.csv");
    std::fs::create_dir_all(new_csv.parent().unwrap()).unwrap();
    let mut body = String::from("id,region\n");
    for i in 1..=10 {
        body.push_str(&format!("{i},us\n"));
    }
    std::fs::write(&new_csv, body).unwrap();

    let e2 = DuckDBEngine::new(dir.path().join("r2.duckdb"), budget()).unwrap();
    e2.init().await.unwrap();
    let new_sources: HashMap<String, std::path::PathBuf> =
        HashMap::from([("sales.csv".to_string(), new_csv.clone())]);
    let result = ReplayEngine::replay(&parsed, &new_sources, &e2)
        .await
        .unwrap();

    // --- 5. Assert: monthly recomputed over the larger source. ---
    let monthly = result
        .recipe
        .tables
        .iter()
        .find(|t| t.name == "monthly")
        .expect("monthly in result recipe");
    assert_eq!(
        monthly.row_count, 10,
        "monthly should re-run count(*) over the new 10-row sales: {monthly:?}"
    );

    // sales schema in the result reflects the new (2-column) source.
    let sales = result
        .recipe
        .tables
        .iter()
        .find(|t| t.name == "sales")
        .expect("sales in result recipe");
    let names: Vec<&str> = sales.schema.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"id") && names.contains(&"region"),
        "sales schema should reflect the new source columns: {names:?}"
    );
    assert_eq!(sales.row_count, 10, "sales row_count refreshed to 10");

    // The source fingerprint + row_count are refreshed too.
    let src = &result.sources.sources[0];
    assert_eq!(src.row_count, 10);
    assert!(src.schema_fingerprint.iter().any(|c| c.name == "region"));

    e2.close().await.unwrap();
}
