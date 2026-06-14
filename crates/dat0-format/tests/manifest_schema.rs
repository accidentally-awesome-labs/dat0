//! The published JSON Schema (docs/schemas/dat0-manifest-v1.schema.json) must
//! validate a real serialized `PackageManifest`. Uses the `jsonschema` 0.36 API:
//! `validator_for(&schema)` -> `Validator`, then `Validator::is_valid(&instance)`.

#[test]
fn published_schema_validates_a_real_manifest() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/schemas/dat0-manifest-v1.schema.json"
    ))
    .unwrap();
    let compiled = jsonschema::validator_for(&schema).unwrap();
    let m = dat0_format::PackageManifest {
        format_version: 1,
        kind: "package".into(),
        dat0_version: "0.1.0".into(),
        package_id: uuid::Uuid::now_v7(),
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-06-13T00:00:00Z".into(),
        table_count: 2,
        checksums: Default::default(),
    };
    let instance = serde_json::to_value(&m).unwrap();
    assert!(
        compiled.is_valid(&instance),
        "manifest must validate against published schema"
    );
}
