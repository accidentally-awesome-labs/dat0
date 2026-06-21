use dat0_app::update;

#[test]
fn semver_compare_is_numeric_and_v_tolerant() {
    assert!(update::newer_than("0.1.0", "0.1.1"));
    assert!(update::newer_than("v1.2.0", "v1.10.0")); // numeric, not lexical
    assert!(!update::newer_than("1.0.0", "1.0.0"));
    assert!(!update::newer_than("2.0.0", "1.9.9"));
}

#[test]
fn fetch_latest_parses_tag_name() {
    let mut server = mockito::Server::new();
    let m = server.mock("GET", "/latest")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"tag_name":"v0.2.0","name":"dat0 0.2.0"}"#)
        .create();
    let url = format!("{}/latest", server.url());
    let tag = update::fetch_latest(&url).unwrap();
    assert_eq!(tag, "v0.2.0");
    m.assert();
}
