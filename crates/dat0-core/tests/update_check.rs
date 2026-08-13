use dat0_core::update;
use dat0_core::update::check;

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
    let m = server
        .mock("GET", "/latest")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"tag_name":"v0.2.0","name":"dat0 0.2.0"}"#)
        .create();
    let url = format!("{}/latest", server.url());
    let tag = update::fetch_latest(&url).unwrap();
    assert_eq!(tag, "v0.2.0");
    m.assert();
}

#[test]
fn returns_some_when_remote_is_newer() {
    let mut s = mockito::Server::new();
    let body = include_bytes!("fixtures/update/latest.json"); // version 0.2.0
    let sig = include_str!("fixtures/update/latest.json.minisig");
    let pk = include_str!("fixtures/update/test-minisign.pub")
        .lines()
        .last()
        .unwrap();
    let m = s
        .mock("GET", "/latest.json")
        .with_body(body.as_slice())
        .create();
    let g = s
        .mock("GET", "/latest.json.minisig")
        .with_body(sig)
        .create();
    let mu = format!("{}/latest.json", s.url());
    let su = format!("{}/latest.json.minisig", s.url());
    let out = check::fetch_update(&mu, &su, pk, "0.1.0").unwrap();
    assert_eq!(out.unwrap().version, "0.2.0");
    m.assert();
    g.assert();
}

#[test]
fn returns_none_when_equal() {
    let mut s = mockito::Server::new();
    let body = include_bytes!("fixtures/update/latest.json");
    let sig = include_str!("fixtures/update/latest.json.minisig");
    let pk = include_str!("fixtures/update/test-minisign.pub")
        .lines()
        .last()
        .unwrap();
    let _m = s
        .mock("GET", "/latest.json")
        .with_body(body.as_slice())
        .create();
    let _g = s
        .mock("GET", "/latest.json.minisig")
        .with_body(sig)
        .create();
    let mu = format!("{}/latest.json", s.url());
    let su = format!("{}/latest.json.minisig", s.url());
    assert!(
        check::fetch_update(&mu, &su, pk, "0.2.0")
            .unwrap()
            .is_none()
    );
}

#[test]
fn err_on_bad_signature() {
    let mut s = mockito::Server::new();
    // Byte-flip the manifest body so the signature won't match
    let mut tampered = include_bytes!("fixtures/update/latest.json").to_vec();
    tampered[0] = if tampered[0] == b'{' { b'[' } else { b'{' };
    let sig = include_str!("fixtures/update/latest.json.minisig");
    let pk = include_str!("fixtures/update/test-minisign.pub")
        .lines()
        .last()
        .unwrap();
    let _m = s.mock("GET", "/latest.json").with_body(tampered).create();
    let _g = s
        .mock("GET", "/latest.json.minisig")
        .with_body(sig)
        .create();
    let mu = format!("{}/latest.json", s.url());
    let su = format!("{}/latest.json.minisig", s.url());
    assert!(check::fetch_update(&mu, &su, pk, "0.1.0").is_err());
}
