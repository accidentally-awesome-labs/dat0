use dat0_app::update::{download, manifest::ArtifactEntry};
use sha2::{Digest, Sha256};

#[test]
fn downloads_and_verifies_sha256() {
    let body = b"hello dat0 update payload";
    let hash = format!("{:x}", Sha256::digest(body));
    let mut s = mockito::Server::new();
    let _m = s.mock("GET", "/art").with_body(body.as_slice()).create();
    let tmp = tempfile::tempdir().unwrap();
    let entry = ArtifactEntry {
        url: format!("{}/art", s.url()),
        sha256: hash,
        size: body.len() as u64,
    };
    let path = download::download_verified(&entry, tmp.path(), |_, _| {}).unwrap();
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).unwrap(), body);
}

#[test]
fn rejects_sha256_mismatch() {
    let mut s = mockito::Server::new();
    let _m = s
        .mock("GET", "/art")
        .with_body("tampered".as_bytes())
        .create();
    let tmp = tempfile::tempdir().unwrap();
    let entry = ArtifactEntry {
        url: format!("{}/art", s.url()),
        sha256: "00".repeat(32),
        size: 8,
    };
    assert!(download::download_verified(&entry, tmp.path(), |_, _| {}).is_err());
}
