//! RL2: the embedded update-signing public key must not be the test fixture.
//!
//! This test is the only thing standing between a real release and a signature
//! chain that every shipped client rejects. `crates/dat0-core/assets/minisign-public-key.txt`
//! is compiled into the binary by `include_str!` (`src/update/manifest.rs:28`)
//! and is the sole trust anchor `verify_manifest` (`manifest.rs:32-42`) uses. If
//! the shipped binary carries the development test key while CI signs
//! `latest.json` with the real `MINISIGN_SECRET_KEY`, `PublicKey::verify`
//! fails with `UnexpectedKeyId` on every client, in the field, silently — the
//! updater simply never offers the update. Nothing else in the tree notices.
//!
//! **This test FAILS until RL1 step 1 is done, by design.** It is the tripwire
//! for a human-only action (key generation) that cannot be automated here.
//!
//! It is therefore `#[ignore]`d: a test that is *known* to fail cannot sit in
//! the workspace run, because a permanently red CI teaches everyone to ignore
//! CI, which costs more than this gate is worth. Ignoring it keeps the gate
//! exactly where `docs/release-prerequisites.md` already puts it — a command a
//! human runs when closing RL1:
//!
//! ```text
//! cargo test -p dat0-core --test update_key_is_production -- --ignored
//! ```
//!
//! Delete the `#[ignore]` in the same commit that lands the production key. At
//! that point it passes, and it belongs in every run.

use dat0_core::update::manifest::EMBEDDED_PUBKEY;

/// The development key committed for `tests/update_manifest.rs`. A minisign
/// `.pub` file is two lines — an untrusted comment, then the base64 key — so
/// the comparison target is line 2.
const TEST_FIXTURE: &str = include_str!("fixtures/update/test-minisign.pub");

fn fixture_key_line() -> &'static str {
    TEST_FIXTURE
        .lines()
        .nth(1)
        .expect("test-minisign.pub is a two-line minisign public key file")
        .trim()
}

#[test]
#[ignore = "fails until RL1 step 1 generates the production signing key; run with --ignored"]
fn embedded_pubkey_is_not_the_test_fixture() {
    let embedded = EMBEDDED_PUBKEY.trim();

    assert!(
        !embedded.is_empty(),
        "crates/dat0-core/assets/minisign-public-key.txt is empty. \
         The file must contain EXACTLY ONE line: the base64 public key from \
         `dat0-minisign.pub`. See RL1 step 1 in docs/release-prerequisites.md."
    );

    assert_ne!(
        embedded,
        fixture_key_line(),
        "\n\
         The embedded update-signing public key is byte-identical to the \
         DEVELOPMENT TEST KEY in crates/dat0-core/tests/fixtures/update/test-minisign.pub.\n\
         Shipping this key means every client rejects every real release manifest.\n\
         \n\
         REMEDIATION — RL1 step 1 (human-only; see docs/release-prerequisites.md):\n\
         \x20 1. rsign generate -W -p dat0-minisign.pub -s dat0-minisign.key\n\
         \x20 2. Copy the SECOND line of dat0-minisign.pub (the base64 key, no comment)\n\
         \x20    into crates/dat0-core/assets/minisign-public-key.txt as its ONLY line.\n\
         \x20 3. Store the full contents of dat0-minisign.key as the GitHub Actions\n\
         \x20    secret MINISIGN_SECRET_KEY. Never commit the secret key.\n"
    );
}
