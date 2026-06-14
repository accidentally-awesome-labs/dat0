use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use dat0_format::*;

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 256 * 1024 * 1024,
    }
}

async fn engine_with_table() -> (tempfile::TempDir, DuckDBEngine) {
    let dir = tempfile::tempdir().unwrap();
    let e = DuckDBEngine::new(dir.path().join("w.duckdb"), budget()).unwrap();
    e.init().await.unwrap();
    e.execute("CREATE TABLE sales AS SELECT * FROM range(42) AS r(id)")
        .await
        .unwrap();
    (dir, e)
}

#[tokio::test]
async fn writer_emits_expected_zip_entries() {
    let (dir, engine) = engine_with_table().await;
    let contents = PackageContents {
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-06-13T00:00:00Z".into(),
        recipe: Recipe {
            tables: vec![RecipeTable {
                id: "t_sales".into(),
                name: "sales".into(),
                kind: TableKind::Base,
                schema: vec![ColumnFingerprint {
                    name: "id".into(),
                    r#type: "BIGINT".into(),
                }],
                row_count: 42,
                data: "data/sales.parquet".into(),
                source_ref: None,
                derivation: None,
            }],
        },
        sources: Sources { sources: vec![] },
        views: Views { views: vec![] },
        queries: Queries { queries: vec![] },
    };
    let out = dir.path().join("out.dat0");
    Writer::write(&contents, &engine, &out).await.unwrap();
    engine.close().await.unwrap();

    let f = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(f).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    for required in [
        "manifest.json",
        "recipe.json",
        "sources.json",
        "views.json",
        "queries.json",
        "data/sales.parquet",
    ] {
        assert!(
            names.iter().any(|n| n == required),
            "missing {required} in {names:?}"
        );
    }
    let mut mf = zip.by_name("manifest.json").unwrap();
    let m: PackageManifest = serde_json::from_reader(&mut mf).unwrap();
    assert_eq!(m.table_count, 1);
    assert!(m.checksums.contains_key("data/sales.parquet"));
}
