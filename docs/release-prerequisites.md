# dat0 Release Prerequisites

These are human provisioning requirements for a production signed release,
not requirements for an ordinary contributor checkout. They remain outstanding
until maintainers record actual evidence; this page does not claim completion.
Unsigned main-only workflow dispatch still builds and smoke-tests both platforms.

The [release preflight](../xtask/src/release.rs) reports missing requirements:
tag findings are errors; manual main-run findings are warnings. The workflow
first validates canonical repository/ref and tag ancestry before accessing
release material. See the [release runbook](release-runbook.md),
[security operations](security-runbook.md), and [documentation index](README.md).

## Status

| # | Prerequisite | Blocks | Done |
|---|---|---|---|
| 1 | Production minisign key pair | Every auto-update on every platform | ☐ |
| 2 | GPG signing subkey + published public key | Linux AppImage signature | ☐ |
| 3 | Apple Developer ID + notary credentials | macOS DMG (Gatekeeper) | ☐ |
| 4 | `nyc_taxi.parquet` release asset + its SHA-256 | Remote sample download | ☐ |
| 5 | GlitchTip DSN + API credentials | Crash reporting, `crash-e2e` gate | ☐ |

---

## 1. Production minisign update-signing key

**Why it blocks everything.** `crates/dat0-core/assets/minisign-public-key.txt`
is compiled into the binary by `include_str!` at
`crates/dat0-core/src/update/manifest.rs:28`:

```rust
pub const EMBEDDED_PUBKEY: &str = include_str!("../../assets/minisign-public-key.txt");
```

*Verified against the tree:* that line is exactly `include_str!` of that path,
and `verify_manifest` (`manifest.rs:32-42`) passes it to
`PublicKey::from_base64(pubkey_b64.trim())`. `from_base64` decodes the WHOLE
string, so the file must contain **only** the 56-character base64 key line — a
minisign `.pub` file's leading `untrusted comment:` line would make it fail to
decode. The trailing newline is fine; `verify_manifest` trims.

The current embedded key is the development fixture key. The production-key
test is deliberately ignored in the ordinary contributor suite until
provisioning is complete; run it explicitly for production release verification.

### Commands

```bash
# Generate only as part of authorized maintainer provisioning.
# Keep private material in an operator-selected protected directory.
# Use the storage/rotation policy in security-runbook.md.
rsign generate -W -p dat0-minisign.pub -s dat0-minisign.key

# The .pub file is two lines: an untrusted comment, then the base64 key.
# ONLY the second line goes into the repository.
sed -n '2p' dat0-minisign.pub > crates/dat0-core/assets/minisign-public-key.txt

# Verify the file is exactly one line of base64 and nothing else.
wc -l < crates/dat0-core/assets/minisign-public-key.txt   # must print 1
cat crates/dat0-core/assets/minisign-public-key.txt       # must NOT contain "untrusted comment"
```

Then, in **Settings → Environments → release → Secrets**, create secret
`MINISIGN_SECRET_KEY` with the **full contents** of `dat0-minisign.key`
(all lines, including the `untrusted comment:` header — `rsign sign -W -s` reads
the whole file).

```bash
pbcopy < dat0-minisign.key      # macOS; paste into the GitHub Secret form
shred -u dat0-minisign.key 2>/dev/null || rm -P dat0-minisign.key
```

**Never commit `dat0-minisign.key`.** Keep an offline backup in a protected
password manager; see [security operations](security-runbook.md).

### Verification

```bash
# `#[ignore]`d until this step lands, so it needs --ignored to run at all.
cargo test -p dat0-core --test update_key_is_production -- --ignored  # must PASS after this step
# Retain the ordinary-suite ignore until production provisioning is reviewed.
cargo test -p dat0-core --test update_manifest            # must still PASS (uses the test fixture, unaffected)
```

---

## 2. GPG signing subkey for the Linux AppImage

**Why.** `xtask/src/linux.rs`'s `gpg_sign` produces the detached
`dat0.AppImage.sig` that `xtask verify --linux` and every user's
`gpg --verify` check. Without an imported key the Linux job fails at signing.

The [security runbook](security-runbook.md) recommends a passphraseless dedicated
CI subkey. `xtask/src/linux.rs` omits `--pinentry-mode loopback --passphrase-fd 0`
when `DAT0_GPG_PASSPHRASE` is empty or unset. Passing those flags with an empty
passphrase makes GPG fail on a passphraseless key rather than skip the prompt.

### Commands

```bash
# Find the primary key fingerprint you want to add a CI subkey to.
gpg --list-secret-keys --keyid-format=long --with-colons | awk -F: '/^fpr/ {print $10; exit}'

# Passphrase-less signing subkey, 2-year validity.
gpg --quick-add-key <fingerprint> ed25519 sign 2y

# Export the SUBKEY only (note the trailing `!` — it pins the export to that
# subkey instead of the whole primary key).
gpg --armor --export-secret-subkeys '<subkey-fingerprint>!' > dat0-ci-signing.key

# Public key, committed so users can verify artifacts independently.
# Publish the public key for independent user verification when provisioned.
gpg --armor --export <fingerprint> > docs/dat0-signing-key.asc
git add docs/dat0-signing-key.asc
```

Create `GPG_PRIVATE_KEY` in the protected `release` environment from
`dat0-ci-signing.key`, then remove the temporary export after securing its backup.
Leave `GPG_PASSPHRASE` unset for a passphraseless subkey; the workflow maps it
to `DAT0_GPG_PASSPHRASE`, and empty is treated as no passphrase.

### Verification

```bash
# On a scratch file, with the CI subkey selected:
echo hi > /tmp/t && gpg --batch --yes --detach-sign /tmp/t && gpg --verify /tmp/t.sig /tmp/t
```

---

## 3. Apple Developer ID + notarization

**Why.** Without a Developer ID signature and a notarization ticket, macOS
Gatekeeper refuses to launch the app and `xtask verify --macos`
(`spctl --assess --type install`) fails.

### Steps

1. Enrol in the Apple Developer Program (paid, ~24-48 h approval).
2. Certificates → **Developer ID Application** → create, download, install into
   the login keychain of a Mac you control.
3. Export the certificate **with its private key** as a `.p12`
   (Keychain Access → right-click → Export; set a strong export password).
4. App Store Connect → Users and Access → Integrations → App Store Connect API
   → create a key of type **Developer**; download the `.p8` **immediately**
   (Apple shows it once).

### Secrets

```bash
# PROTECTED_DIR is an operator-selected protected directory.
base64 -i "$PROTECTED_DIR/dat0-dev-id.p12" | pbcopy   # → APPLE_DEV_ID_CERT_P12
base64 -i "$PROTECTED_DIR/AuthKey_XXXX.p8" | pbcopy   # → AC_API_KEY_P8
security find-identity -v -p codesigning         # → DAT0_SIGN_IDENTITY (copy verbatim)
```

| Secret | Value |
|---|---|
| `APPLE_DEV_ID_CERT_P12` | base64 of the `.p12` |
| `CERT_PASSWORD` | the `.p12` export password |
| `KEYCHAIN_PASSWORD` | any strong random string; only used to create the runner's ephemeral `build.keychain` |
| `AC_API_KEY_P8` | base64 of the `.p8` |
| `AC_KEY_ID` | key ID beside the `.p8` in App Store Connect |
| `AC_ISSUER_ID` | issuer UUID at the top of the API keys page |
| `DAT0_SIGN_IDENTITY` | the full `Developer ID Application: … (TEAMID)` string, verbatim |

Install these secrets in the protected `release` environment. Remove temporary
`.p12` and `.p8` exports after confirming provisioning and protected backups.

---

## 4. `nyc_taxi.parquet` sample asset

**Why.** `crates/dat0-core/src/sample_data.rs:46` ships
`pub const NYC_TAXI_SHA256: &str = "FILL_AT_T8";` — verified against the tree.
`NYC_TAXI_URL` (`:41`) already points at release tag `sample-data-v1`, so the
remote sample either fails its integrity check or, worse, is accepted against a
placeholder.

```bash
# Publish only a reviewed public sample; check for an existing release first.
gh release view sample-data-v1 --repo accidentally-awesome-labs/dat0
# If absent, an authorized maintainer creates it:
gh release create sample-data-v1 --repo accidentally-awesome-labs/dat0 \
  --title "dat0 sample data v1" --notes "NYC taxi Parquet sample"
gh release upload sample-data-v1 nyc_taxi.parquet --repo accidentally-awesome-labs/dat0

# 2. Hash it and paste the hex into sample_data.rs:46.
shasum -a 256 nyc_taxi.parquet
```

Replace `"FILL_AT_T8"` with the actual 64-character digest through a DCO-signed
preparation PR. Record asset provenance and successful integrity verification
in that PR; do not publish private datasets.

---

## 5. GlitchTip crash reporting

`docs/release-runbook.md` ("GlitchTip / crash reporting") describes the
instance. Three secrets are needed before `crash-e2e.yml` stops self-skipping:

| Secret | Where to find it |
|---|---|
| `GLITCHTIP_DSN_PUBLIC` | GlitchTip → project → Settings → Client Keys (the full `https://…@host/N` URL). `release.yml` and `crash-e2e.yml` compile it in as `DAT0_GLITCHTIP_DSN_PUBLIC`. |
| `GLITCHTIP_API_TOKEN` | GlitchTip → account settings → API Tokens (read access to the dat0 project) |
| `GLITCHTIP_PROJECT_SLUG` | the slug in the project URL, e.g. `dat0` |

Ordinary development/CI builds use `.cargo/config.toml`'s stub. Release builds
compile the selected DSN; the tag preflight rejects a missing/stub DSN. Put the
release DSN where the release environment jobs consume it. Scope API token and
project slug separately to the main-only crash-e2e workflow, not ordinary PRs.

---

## After the checklist

Submit public-key/hash/source changes through a DCO-signed PR and merge after
the required `CI required` and `DCO` verdicts, not a direct push to main.
The protected `release` environment permits only the `main` branch and `v*`
tags. Before tagging, record provisioned verification in the release PR:

```bash
cargo xtask release-check
cargo test -p dat0-core --test update_key_is_production -- --ignored
cargo test -p dat0-core --test update_manifest_roundtrip
gh workflow run release.yml --repo accidentally-awesome-labs/dat0 --ref main
```

Manual main dispatch always skips `publish`. Without secrets it produces an
ad-hoc-signed macOS disk image and unsigned Linux AppImage, with build and smoke
checks. With provisioned platform secrets it can exercise signing/notarization,
but never production manifest signing/publication. Fixture tests do not prove
production-key signing. Only a canonical `v*` tag push on main history can
publish; tag dispatch and non-main dispatch are rejected before signing.

Complete the runbook's clean-machine installation/Gatekeeper, identity and
checksum/GPG, package association/read-only Inspect, restart/forwarding, updater
round-trip/rejection and synthetic telemetry consent/receipt/redaction checks.
Record unsigned versus provisioned outcomes and anything not exercised.

### What the dry run must confirm

- **The AppImage starts and draws its page on clean systems.** It is built on
  Ubuntu 22.04, the oldest glibc dat0 supports: one built on `ubuntu-latest`
  needed glibc 2.38, which Ubuntu 22.04 and Debian 12 lack. It carries the
  binary, libxdo and libxdo's two X extensions, and takes WebKitGTK, GTK and
  GLib from the host (`xtask::linux::HOST_LIBRARIES` says why: an AppImage
  that carried WebKit opened a blank window on any host whose WebKit differed
  from the build machine's). `scripts/appimage-smoke.sh` starts it on Ubuntu
  22.04, Debian 12 and Ubuntu 24.04, each given `libwebkit2gtk-4.1-0` and
  nothing else dat0 needs, and fails unless it prints its version, WebKit's
  helper processes stay up and the screenshot is not a blank window.
  **Confirm at the dry run:** that step passes; the screenshots are in the
  `appimage-smoke` artefact.
- **The tools are the pinned ones.** appimagetool 1.9.1 and the AppImage
  runtime 20251108 are checked against their SHA-256 before the job that
  holds the GPG key runs them; linuxdeploy is no longer used.
- **macOS update payload is signed.** `sign::sign_and_notarize` now staples the
  ticket to `dat0.app` and rebuilds `dat0.app.tar.gz` from the signed, stapled
  bundle (`macos::tar_app`, which deletes the stale archive and asserts its
  absence before running `tar`). Before this, the tarball the auto-updater
  downloads and swaps into place was the one `macos::bundle` wrote *before*
  signing — the DMG was signed and the update path was not.
  `xtask/tests/sign_args.rs::update_payload_is_retarred_after_notarization`
  locks the ordering. **Confirm at the dry run:** `xcrun stapler validate
  target/macos/dat0.app` passes, and the tarball's mtime is later than the
  DMG's.
