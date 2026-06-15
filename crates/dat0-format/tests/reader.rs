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

fn make_contents() -> PackageContents {
    PackageContents {
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
        charts: Charts { charts: vec![] },
    }
}

#[tokio::test]
async fn reader_round_trips_model_and_verifies_checksums() {
    let (dir, engine) = engine_with_table().await;
    let contents = make_contents();
    let out = dir.path().join("out.dat0");
    Writer::write(&contents, &engine, &out).await.unwrap();
    engine.close().await.unwrap();

    let parsed = Reader::open(&out).unwrap();
    assert_eq!(parsed.manifest.format_version, dat0_format::FORMAT_VERSION);
    assert_eq!(parsed.recipe.tables.len(), 1);
    assert_eq!(parsed.recipe.tables[0].name, "sales");

    // Extract data and confirm the parquet is non-empty + present.
    let extract_dir = tempfile::tempdir().unwrap();
    parsed.extract_data_to(extract_dir.path()).unwrap();
    let pq_path = extract_dir.path().join("data/sales.parquet");
    assert!(
        pq_path.exists(),
        "data/sales.parquet should exist after extraction"
    );
    assert!(
        std::fs::metadata(&pq_path).unwrap().len() > 0,
        "data/sales.parquet should be non-empty"
    );
}

#[test]
fn reader_rejects_future_major_version() {
    use std::io::Write;

    // Hand-craft a zip whose manifest.json has format_version: 2.
    // The reader must hit UnsupportedVersion BEFORE checksum verification,
    // so we only need a valid manifest.json entry — no data, no checksums.
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("future.dat0");

    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Minimal manifest with format_version: 2 (future major).
    let manifest = PackageManifest {
        format_version: 2,
        kind: PACKAGE_KIND.into(),
        dat0_version: "99.0.0".into(),
        package_id: uuid::Uuid::now_v7(),
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2030-01-01T00:00:00Z".into(),
        table_count: 0,
        checksums: std::collections::BTreeMap::new(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    zip.start_file("manifest.json", opts).unwrap();
    zip.write_all(&manifest_bytes).unwrap();
    zip.finish().unwrap();

    let result = Reader::open(&zip_path);
    assert!(
        matches!(
            result,
            Err(FormatError::UnsupportedVersion { found: 2, .. })
        ),
        "expected UnsupportedVersion{{found:2}}, got: {result:?}"
    );
}
