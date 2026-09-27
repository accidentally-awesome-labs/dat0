//! Step 7: the publish job checks the signed update manifest as the app will
//! before it uploads anything. A `MINISIGN_SECRET_KEY` that is not the pair of
//! the key the app compiles in signs without complaint; every client would
//! then refuse the update.
//!
//! The manifest, signature and throwaway key are dat0-core's round-trip
//! fixtures, which `update_manifest_roundtrip.rs` verifies with the app's own
//! `verify_manifest`.

use std::path::Path;
use xtask::manifest;

fn fixture(name: &str) -> String {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/dat0-core/tests/fixtures/update");
    std::fs::read_to_string(dir.join(name)).unwrap()
}

/// The key line of a minisign `.pub` file, as the app's key file holds it.
fn key_of(pub_file: &str) -> String {
    fixture(pub_file).lines().nth(1).unwrap().to_string()
}

#[test]
fn a_manifest_signed_by_the_trusted_key_passes() {
    let key = key_of("roundtrip/throwaway-minisign.pub");
    manifest::verify(
        fixture("roundtrip/latest.json").as_bytes(),
        &fixture("roundtrip/latest.json.minisig"),
        // The app's key file ends in a newline.
        &format!("{key}\n"),
    )
    .unwrap();
}

#[test]
fn a_manifest_signed_by_another_key_is_refused() {
    let err = manifest::verify(
        fixture("roundtrip/latest.json").as_bytes(),
        &fixture("roundtrip/latest.json.minisig"),
        &key_of("test-minisign.pub"),
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("does not verify against the key dat0 trusts"),
        "{err}"
    );
}

#[test]
fn a_manifest_changed_after_signing_is_refused() {
    let changed = fixture("roundtrip/latest.json").replace("12345", "12346");
    assert!(
        manifest::verify(
            changed.as_bytes(),
            &fixture("roundtrip/latest.json.minisig"),
            &key_of("roundtrip/throwaway-minisign.pub"),
        )
        .is_err()
    );
}
