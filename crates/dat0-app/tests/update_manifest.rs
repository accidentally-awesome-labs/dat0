use dat0_app::update::manifest::{self, UpdateManifest};

const PUBKEY: &str = include_str!("fixtures/update/test-minisign.pub");
const MANIFEST: &[u8] = include_bytes!("fixtures/update/latest.json");
const SIG: &str = include_str!("fixtures/update/latest.json.minisig");

fn pubkey_line() -> &'static str {
    // a minisign .pub file is two lines: a comment, then the base64 key.
    PUBKEY.lines().last().unwrap()
}

#[test]
fn verifies_and_parses_a_valid_signed_manifest() {
    let m: UpdateManifest = manifest::verify_manifest(MANIFEST, SIG, pubkey_line()).unwrap();
    assert_eq!(m.version, "0.2.0");
    assert_eq!(m.macos.url, "https://example.invalid/dat0.app.tar.gz");
    assert_eq!(m.linux.size, 200);
}

#[test]
fn rejects_a_tampered_manifest() {
    let mut tampered = MANIFEST.to_vec();
    tampered[0] ^= 0x01; // flip a byte
    assert!(manifest::verify_manifest(&tampered, SIG, pubkey_line()).is_err());
}

#[test]
fn rejects_a_wrong_key() {
    // a different valid minisign pubkey (any other base64 key) must not verify.
    let other = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    assert!(manifest::verify_manifest(MANIFEST, SIG, other).is_err());
}
