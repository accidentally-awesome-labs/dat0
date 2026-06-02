use dat0_engine::render::render_export_select;
use dat0_engine::transform::ProjectionColumn;

fn col(source: &str, display: &str) -> ProjectionColumn {
    ProjectionColumn {
        source: source.into(),
        display: display.into(),
    }
}

#[test]
fn export_select_aliases_renames_and_omits_identity_alias() {
    let cols = vec![col("amt", "amt"), col("city", "City")];
    let sql = render_export_select("SELECT * FROM \"v_tab1_3\"", &cols);
    // identity column has no alias; renamed column aliases; surrogate absent
    // (caller's `cols` never includes __dat0_rowid).
    assert_eq!(
        sql,
        "SELECT \"amt\", \"city\" AS \"City\" FROM (SELECT * FROM \"v_tab1_3\")"
    );
}

use dat0_engine::types::{DerivedOrigin, ExportFormat, MemoryBudget};
use dat0_engine::{DuckDBEngine, QueryEngine};

async fn engine_with_things() -> (DuckDBEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(
        dir.path().join("a.duckdb"),
        MemoryBudget {
            bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    engine
        .create_table(
            "things",
            "SELECT 1::INTEGER as id, 'a'::VARCHAR as name UNION ALL SELECT 2, 'b'",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();
    (engine, dir)
}

#[tokio::test]
async fn export_query_to_path_writes_renamed_csv() {
    let (engine, dir) = engine_with_things().await;
    let cols = vec![col("id", "id"), col("name", "Label")];
    let select = render_export_select("SELECT * FROM \"things\"", &cols);
    let dest = dir.path().join("out.csv");
    engine
        .export_query_to_path(&select, ExportFormat::Csv, &dest)
        .await
        .unwrap();
    let s = std::fs::read_to_string(&dest).unwrap();
    assert!(s.lines().next().unwrap().contains("Label"), "header: {s}");
    assert!(
        !s.contains("__dat0_rowid"),
        "surrogate must be stripped: {s}"
    );
    assert!(s.contains("\n1,a") || s.contains("1,a"), "data row: {s}");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_query_to_path_writes_json() {
    let (engine, dir) = engine_with_things().await;
    let cols = vec![col("id", "id"), col("name", "Label")];
    let select = render_export_select("SELECT * FROM \"things\"", &cols);
    let dest = dir.path().join("out.json");
    engine
        .export_query_to_path(&select, ExportFormat::Json, &dest)
        .await
        .unwrap();
    let s = std::fs::read_to_string(&dest).unwrap();
    // JSON ARRAY format: should be a JSON array containing "Label" as key
    let parsed: serde_json::Value = serde_json::from_str(&s).expect("must be valid JSON");
    let arr = parsed.as_array().expect("must be JSON array");
    assert!(!arr.is_empty(), "array must not be empty");
    let first = &arr[0];
    assert!(
        first.get("Label").is_some(),
        "first element must have 'Label' key, got: {first}"
    );
    assert!(
        !s.contains("__dat0_rowid"),
        "surrogate must be stripped: {s}"
    );
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_query_to_path_writes_parquet() {
    let (engine, dir) = engine_with_things().await;
    let cols = vec![col("id", "id"), col("name", "Label")];
    let select = render_export_select("SELECT * FROM \"things\"", &cols);
    let dest = dir.path().join("out.parquet");
    engine
        .export_query_to_path(&select, ExportFormat::Parquet, &dest)
        .await
        .unwrap();

    // File must be non-empty (Parquet magic bytes)
    let bytes = std::fs::read(&dest).unwrap();
    assert!(
        bytes.starts_with(b"PAR1"),
        "must be valid Parquet: {:?}",
        &bytes[..4.min(bytes.len())]
    );

    // Re-read via register_file_as_table to verify schema has "Label" column
    use dat0_engine::RegisterOpts;
    let info = engine
        .register_file_as_table(&dest, RegisterOpts::default())
        .await
        .unwrap();

    // Confirm "Label" column exists via describe
    let col_infos = engine.describe_table(&info.name, None).await.unwrap();
    let col_names: Vec<&str> = col_infos.iter().map(|c| c.name.as_str()).collect();
    assert!(
        col_names.contains(&"Label"),
        "re-read parquet must have 'Label' column, got: {col_names:?}"
    );

    // Also verify 2 rows via execute
    let result = engine
        .execute(&format!("SELECT COUNT(*) AS n FROM \"{}\"", info.name))
        .await
        .unwrap();
    let batch = &result.batches[0];
    let count_val = batch
        .column(0)
        .as_any()
        .downcast_ref::<duckdb::arrow::array::Int64Array>()
        .expect("COUNT(*) must be Int64")
        .value(0);
    assert_eq!(
        count_val, 2,
        "parquet file must contain exactly 2 data rows"
    );

    engine.close().await.unwrap();
}
