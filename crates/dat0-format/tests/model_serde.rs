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
