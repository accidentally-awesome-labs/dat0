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
/// Write `orig.dat0` into `dir`: a `sales` base table (3 rows) and a derived
/// `monthly` whose recipe step is `step_sql`.
///
/// The writing engine builds `monthly` from a harmless query — writing never
/// runs recipe SQL — so a test can plant any step and see what replay does
/// with it.
async fn write_package(dir: &std::path::Path, step_sql: &str) -> std::path::PathBuf {
    // --- 1. Original sales.csv: header `id` + 3 rows. ---
    let orig_csv = dir.join("sales.csv");
    std::fs::write(&orig_csv, "id\n1\n2\n3\n").unwrap();

    let e = DuckDBEngine::new(dir.join("r.duckdb"), budget()).unwrap();
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
                        sql: step_sql.into(),
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
        charts: Charts { charts: vec![] },
    };

    let pkg = dir.join("orig.dat0");
    Writer::write(&contents, &e, &pkg).await.unwrap();
    e.close().await.unwrap();
    pkg
}

/// A `sales.csv` of `rows` rows with an extra `region` column, outside `dir`'s
/// work area, for replay to rebind the package's source to.
fn new_sales(dir: &std::path::Path, rows: u32) -> HashMap<String, std::path::PathBuf> {
    let new_csv = dir.join("new").join("sales.csv");
    std::fs::create_dir_all(new_csv.parent().unwrap()).unwrap();
    let mut body = String::from("id,region\n");
    for i in 1..=rows {
        body.push_str(&format!("{i},us\n"));
    }
    std::fs::write(&new_csv, body).unwrap();
    HashMap::from([("sales.csv".to_string(), new_csv)])
}

/// A fresh engine for one replay, living in its own `work` directory — the
/// directory replay confines it to.
async fn replay_engine(work: &std::path::Path) -> DuckDBEngine {
    let e = DuckDBEngine::new(work.join("r2.duckdb"), budget()).unwrap();
    e.init().await.unwrap();
    e
}

#[tokio::test]
async fn replay_reexecutes_derived_against_new_larger_source() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = write_package(dir.path(), "SELECT id FROM sales WHERE id > 0").await;

    // --- Parse it back; rebind to a NEW sales.csv with an extra column + 10 rows. ---
    let parsed = Reader::open(&pkg).unwrap();
    let new_sources = new_sales(dir.path(), 10);
    let work = tempfile::tempdir().unwrap();
    let e2 = replay_engine(work.path()).await;
    let result = ReplayEngine::replay(&parsed, &new_sources, &e2, work.path())
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

#[tokio::test]
async fn replay_refuses_a_step_that_is_not_a_single_query() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = write_package(dir.path(), "SELECT id FROM sales; DROP TABLE sales").await;
    let parsed = Reader::open(&pkg).unwrap();
    let work = tempfile::tempdir().unwrap();
    let e2 = replay_engine(work.path()).await;

    let err = ReplayEngine::replay(&parsed, &new_sales(dir.path(), 3), &e2, work.path())
        .await
        .expect_err("two statements in one step must not run");
    match err {
        FormatError::RecipeStepRefused { table, .. } => assert_eq!(table, "monthly"),
        other => panic!("expected FormatError::RecipeStepRefused, got {other:?}"),
    }
    // Refused before it ran: the base table the second statement names is intact.
    assert!(e2.describe_table("sales", None).await.is_ok());
    e2.close().await.unwrap();
}

#[tokio::test]
async fn a_replayed_step_cannot_reach_outside_its_work_directory() {
    // A single query that reads a file is still a single query; what stops it
    // is the confinement, which is what this pins.
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let file = elsewhere.path().join("elsewhere.csv");
    std::fs::write(&file, "id\n42\n").unwrap();
    let step = format!("SELECT * FROM read_csv('{}')", file.display());
    let pkg = write_package(dir.path(), &step).await;
    let parsed = Reader::open(&pkg).unwrap();
    let work = tempfile::tempdir().unwrap();
    let e2 = replay_engine(work.path()).await;

    let err = ReplayEngine::replay(&parsed, &new_sales(dir.path(), 3), &e2, work.path())
        .await
        .expect_err("a step must not read outside the replay's work directory");
    assert!(matches!(err, FormatError::Engine(_)), "got {err:?}");
    assert!(
        e2.describe_table("monthly", None).await.is_err(),
        "the step must not have produced a table"
    );
    e2.close().await.unwrap();
}

#[tokio::test]
async fn the_writer_refuses_a_table_name_that_is_not_a_file_name() {
    // Replay writes back the recipe it read, so the Writer re-checks what the
    // Reader did: a name becomes a path the moment Parquet is staged.
    let work = tempfile::tempdir().unwrap();
    let e = replay_engine(work.path()).await;
    let contents = PackageContents {
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-09-25T00:00:00Z".into(),
        recipe: Recipe {
            tables: vec![RecipeTable {
                id: "t_x".into(),
                name: "../x".into(),
                kind: TableKind::Base,
                schema: vec![],
                row_count: 0,
                data: data_entry("../x"),
                source_ref: None,
                derivation: None,
            }],
        },
        sources: Sources { sources: vec![] },
        views: Views { views: vec![] },
        queries: Queries { queries: vec![] },
        charts: Charts { charts: vec![] },
    };
    let dest = work.path().join("out.dat0");
    let err = Writer::write_using(&contents, &e, &dest, work.path())
        .await
        .expect_err("an unsafe name must not be written");
    assert!(
        matches!(err, FormatError::UnsafeTableName { .. }),
        "got {err:?}"
    );
    assert!(!dest.exists(), "nothing is written for a refused package");
    e.close().await.unwrap();
}
