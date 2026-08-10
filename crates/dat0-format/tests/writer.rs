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
        charts: Charts { charts: vec![] },
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

/// The four sidecars QA4 added to the manifest's `checksums` map, plus the two
/// that were always there. Named here so the assertion below reads as a
/// contract rather than as a list of strings.
const CHECKSUMMED_SIDECARS: &[&str] = &[
    "recipe.json",
    "sources.json",
    "views.json",
    "queries.json",
    "charts.json",
];

#[tokio::test]
async fn writer_checksums_every_json_sidecar_and_the_reader_verifies_them() {
    // Until QA4 only `recipe.json` and `data/*.parquet` were checksummed, so
    // `sources.json` / `views.json` / `queries.json` / `charts.json` could be
    // rewritten and the package would still verify clean — `Reader::open`
    // verifies exactly the entries the manifest LISTS (reader.rs:66-80), and
    // those four were not listed. They carry replay source paths and executable
    // SQL, so they are as load-bearing as the recipe.
    let (dir, engine) = engine_with_table().await;
    let contents = PackageContents {
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-08-08T00:00:00Z".into(),
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
    };
    let out = dir.path().join("out.dat0");
    Writer::write(&contents, &engine, &out).await.unwrap();
    engine.close().await.unwrap();

    let f = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(f).unwrap();
    let m: PackageManifest = {
        let mut mf = zip.by_name("manifest.json").unwrap();
        serde_json::from_reader(&mut mf).unwrap()
    };
    for name in CHECKSUMMED_SIDECARS {
        assert!(
            m.checksums.contains_key(*name),
            "manifest must checksum {name}; got keys {:?}",
            m.checksums.keys().collect::<Vec<_>>()
        );
        assert!(
            m.checksums[*name].starts_with("sha256:"),
            "{name} checksum must be sha256-prefixed"
        );
    }
    assert!(m.checksums.contains_key("data/sales.parquet"));

    // And the checksums are CORRECT, not merely present — `Reader::open`
    // recomputes every one of them.
    Reader::open(&out).expect("a freshly written package must verify");
}

#[test]
fn a_tampered_sidecar_now_fails_verification() {
    use std::io::Write;

    // The regression this closes. Hand-build a package whose manifest records
    // the sha of the ORIGINAL sources.json while the archive carries an edited
    // one — exactly what an attacker or a careless zip-editor produces. Before
    // QA4 this opened without complaint.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tampered-sources.dat0");

    let sha = |b: &[u8]| format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(b));

    let recipe_bytes = serde_json::to_vec_pretty(&Recipe { tables: vec![] }).unwrap();
    let honest_sources = serde_json::to_vec_pretty(&Sources { sources: vec![] }).unwrap();
    let views_bytes = serde_json::to_vec_pretty(&Views { views: vec![] }).unwrap();
    let queries_bytes = serde_json::to_vec_pretty(&Queries { queries: vec![] }).unwrap();
    let charts_bytes = serde_json::to_vec_pretty(&Charts { charts: vec![] }).unwrap();
    // Same VALUE, different BYTES — still deserializes as `Sources`, so nothing
    // but the checksum can catch it.
    //
    // Compact rather than pretty: `to_vec_pretty` emits exactly
    // `{\n  "sources": []\n}`, so a hand-written copy of that string was
    // byte-identical to the honest bytes and the "tamper" mutated nothing. The
    // assertion below is what caught it and is why it stays.
    let edited_sources = serde_json::to_vec(&Sources { sources: vec![] }).unwrap();
    assert_ne!(
        edited_sources, honest_sources,
        "the tamper must actually change the bytes, or this test proves nothing"
    );

    let mut checksums = std::collections::BTreeMap::new();
    checksums.insert("recipe.json".to_string(), sha(&recipe_bytes));
    checksums.insert("sources.json".to_string(), sha(&honest_sources));
    checksums.insert("views.json".to_string(), sha(&views_bytes));
    checksums.insert("queries.json".to_string(), sha(&queries_bytes));
    checksums.insert("charts.json".to_string(), sha(&charts_bytes));

    let manifest = PackageManifest {
        format_version: dat0_format::FORMAT_VERSION,
        kind: PACKAGE_KIND.into(),
        dat0_version: "0.0.0".into(),
        package_id: uuid::Uuid::now_v7(),
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-08-08T00:00:00Z".into(),
        table_count: 0,
        checksums,
    };

    let file = std::fs::File::create(&p).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [
        (
            "manifest.json",
            serde_json::to_vec_pretty(&manifest).unwrap(),
        ),
        ("recipe.json", recipe_bytes),
        ("sources.json", edited_sources),
        ("views.json", views_bytes),
        ("queries.json", queries_bytes),
        ("charts.json", charts_bytes),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();

    let err = Reader::open(&p).expect_err("an edited sources.json must not verify");
    match err {
        FormatError::ChecksumMismatch { entry } => assert_eq!(entry, "sources.json"),
        other => panic!("expected ChecksumMismatch{{sources.json}}, got: {other:?}"),
    }
}

#[test]
fn a_pre_change_package_without_sidecar_checksums_still_opens() {
    use std::io::Write;

    // Backward compatibility, and the reason FORMAT_VERSION stays 1. Packages
    // written before QA4 list ONLY recipe.json (and data entries) in
    // `checksums`. `Reader::open` verifies whatever the manifest lists, so the
    // four new entries being absent is not a defect — it is an older writer.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("pre-change.dat0");

    let recipe_bytes = serde_json::to_vec_pretty(&Recipe { tables: vec![] }).unwrap();
    let mut checksums = std::collections::BTreeMap::new();
    checksums.insert(
        "recipe.json".to_string(),
        format!(
            "sha256:{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(&recipe_bytes)
        ),
    );
    let manifest = PackageManifest {
        format_version: dat0_format::FORMAT_VERSION,
        kind: PACKAGE_KIND.into(),
        dat0_version: "0.0.0".into(),
        package_id: uuid::Uuid::now_v7(),
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-06-13T00:00:00Z".into(),
        table_count: 0,
        checksums,
    };

    let file = std::fs::File::create(&p).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [
        (
            "manifest.json",
            serde_json::to_vec_pretty(&manifest).unwrap(),
        ),
        ("recipe.json", recipe_bytes),
        (
            "sources.json",
            serde_json::to_vec_pretty(&Sources { sources: vec![] }).unwrap(),
        ),
        (
            "views.json",
            serde_json::to_vec_pretty(&Views { views: vec![] }).unwrap(),
        ),
        (
            "queries.json",
            serde_json::to_vec_pretty(&Queries { queries: vec![] }).unwrap(),
        ),
        (
            "charts.json",
            serde_json::to_vec_pretty(&Charts { charts: vec![] }).unwrap(),
        ),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();

    let parsed = Reader::open(&p).expect("a pre-QA4 package must still open");
    assert_eq!(
        parsed.manifest.checksums.len(),
        1,
        "the fixture is only meaningful if the four new entries really are absent"
    );
}
