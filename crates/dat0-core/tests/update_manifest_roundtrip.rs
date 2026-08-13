//! RL5: prove the `xtask` → client wire contract for `latest.json` without the
//! production secret.
//!
//! `release.yml`'s `publish` job is gated on `github.ref_type == 'tag'`, so a
//! `workflow_dispatch` dry run NEVER executes the manifest generate-and-sign
//! step. That leaves the single most consequential seam in the update chain —
//! "the bytes `xtask` emits are the bytes the shipped client verifies and
//! parses" — untested by the only pre-tag validation we have. This test closes
//! it locally.
//!
//! **Why the manifest JSON is replicated here rather than called.** `xtask` is
//! a separate workspace crate that `dat0-core` does not (and should not) depend
//! on: `dat0-core` is the shipped product, `xtask` is release machinery, and a
//! product→machinery dependency edge would drag `xtask` into every
//! `cargo test -p dat0-core`. So [`xtask_manifest_json`] replicates
//! `xtask/src/manifest.rs::build_manifest` byte for byte, and
//! [`replica_matches_the_xtask_emitter`] guards the replication by asserting
//! the xtask source still contains the literals the replica was derived from —
//! the same source-ratchet idiom `tests/style_lint.rs` and
//! `tests/window_module_ratchet.rs` use.
//!
//! **Why the signature is a committed fixture.** Minisign signing is Ed25519;
//! `minisign-verify` is verify-only by design (`Cargo.toml:62`) and there is no
//! signer in the dependency tree. Rather than add one, the throwaway keypair
//! was generated once with `minisign -G -W`, used to sign the exact bytes
//! [`xtask_manifest_json`] produces, and only the PUBLIC key plus the signature
//! were committed under `fixtures/update/roundtrip/`. The secret key was
//! discarded — it never existed outside a `mktemp -d`. This is a throwaway
//! test key with no relationship to the production key that RL1 step 1
//! provisions; `tests/update_key_is_production.rs` is what guards that one.

use dat0_core::update::manifest::{self, UpdateManifest};

/// The exact bytes that were signed: `xtask_manifest_json` output for the
/// fixture's parameters, with NO trailing newline (`xtask/src/main.rs`'s
/// `GenManifest` arm does `std::fs::write("target/latest.json", json)`, which
/// writes the `format!` result verbatim).
const SIGNED_MANIFEST: &[u8] = include_bytes!("fixtures/update/roundtrip/latest.json");
const SIGNATURE: &str = include_str!("fixtures/update/roundtrip/latest.json.minisig");
const THROWAWAY_PUBKEY: &str = include_str!("fixtures/update/roundtrip/throwaway-minisign.pub");
/// The emitter this file replicates. Embedded at compile time so the drift
/// guard cannot be defeated by running the test from a different directory.
const XTASK_MANIFEST_SRC: &str = include_str!("../../../xtask/src/manifest.rs");

/// The fixture's parameters. `version` is deliberately not a real release
/// version so nobody mistakes the fixture for a shipped manifest.
const VERSION: &str = "9.9.9-roundtrip";
const MACOS_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MACOS_SIZE: u64 = 12345;
const LINUX_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const LINUX_SIZE: u64 = 67890;

/// Byte-for-byte replica of `xtask::manifest::build_manifest`. Keep in lockstep
/// with `xtask/src/manifest.rs`; `replica_matches_the_xtask_emitter` fails if
/// they drift.
fn xtask_manifest_json(
    version: &str,
    macos_sha: &str,
    macos_size: u64,
    linux_sha: &str,
    linux_size: u64,
) -> String {
    let macos_url = format!(
        "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{}/dat0.app.tar.gz",
        version
    );
    let linux_asset = format!("dat0-{version}-x86_64.AppImage");
    let linux_url = format!(
        "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{}/{}",
        version, linux_asset
    );
    format!(
        r#"{{
  "version": "{}",
  "macos": {{ "url": "{}", "sha256": "{}", "size": {} }},
  "linux": {{ "url": "{}", "sha256": "{}", "size": {} }}
}}"#,
        version, macos_url, macos_sha, macos_size, linux_url, linux_sha, linux_size
    )
}

/// A minisign `.pub` file is a comment line then the base64 key;
/// `verify_manifest` trims but does not strip comments.
fn pubkey_line() -> &'static str {
    THROWAWAY_PUBKEY
        .lines()
        .nth(1)
        .expect("throwaway-minisign.pub is a two-line minisign public key file")
}

#[test]
fn replica_matches_the_xtask_emitter() {
    // If any of these literals move, `xtask_manifest_json` above is stale and
    // this whole test is verifying a manifest shape nobody ships.
    for needle in [
        "/releases/download/v{}/dat0.app.tar.gz",
        r#"let linux_asset = format!("dat0-{version}-x86_64.AppImage");"#,
        "/releases/download/v{}/{}",
        r#""version": "{}","#,
        r#""macos": {{ "url": "{}", "sha256": "{}", "size": {} }},"#,
        r#""linux": {{ "url": "{}", "sha256": "{}", "size": {} }}"#,
        "version, macos_url, macos_sha, macos_size, linux_url, linux_sha, linux_size",
    ] {
        assert!(
            XTASK_MANIFEST_SRC.contains(needle),
            "xtask/src/manifest.rs no longer contains {needle:?}.\n\
             `xtask_manifest_json` in this file is a byte-for-byte replica of that \
             emitter and has drifted. Update the replica, regenerate \
             tests/fixtures/update/roundtrip/latest.json{{,.minisig}} with a fresh \
             throwaway `minisign -G -W` key, and re-run."
        );
    }
}

#[test]
fn fixture_is_exactly_what_xtask_emits() {
    let built = xtask_manifest_json(VERSION, MACOS_SHA, MACOS_SIZE, LINUX_SHA, LINUX_SIZE);
    assert_eq!(
        built.as_bytes(),
        SIGNED_MANIFEST,
        "the signed fixture is no longer the emitter's output — the signature \
         below would then prove nothing about real release bytes"
    );
}

#[test]
fn client_verifies_and_parses_the_signed_xtask_manifest() {
    let m: UpdateManifest = manifest::verify_manifest(SIGNED_MANIFEST, SIGNATURE, pubkey_line())
        .expect("client must accept a correctly signed xtask manifest");

    assert_eq!(m.version, VERSION);
    assert_eq!(
        m.macos.url,
        format!(
            "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{VERSION}/dat0.app.tar.gz"
        )
    );
    assert_eq!(m.macos.sha256, MACOS_SHA);
    assert_eq!(m.macos.size, MACOS_SIZE);
    assert_eq!(
        m.linux.url,
        format!(
            "https://github.com/accidentally-awesome-labs/dat0/releases/download/v{VERSION}/dat0-{VERSION}-x86_64.AppImage"
        )
    );
    assert_eq!(m.linux.sha256, LINUX_SHA);
    assert_eq!(m.linux.size, LINUX_SIZE);
}

#[test]
fn client_rejects_a_one_byte_mutation() {
    // Mutate every byte position in turn: a signature that only catches the
    // first byte is not a signature. Cheap — the manifest is ~500 bytes.
    for i in 0..SIGNED_MANIFEST.len() {
        let mut tampered = SIGNED_MANIFEST.to_vec();
        tampered[i] ^= 0x01;
        assert!(
            manifest::verify_manifest(&tampered, SIGNATURE, pubkey_line()).is_err(),
            "a one-byte mutation at offset {i} must be rejected"
        );
    }
}
