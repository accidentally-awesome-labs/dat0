# dat0 Release Prerequisites (RL1)

> **Human-only.** Every item on this page requires key material, a paid Apple
> enrolment, or write access to the GitHub Secrets store. None of it can be
> automated from inside the repository, and none of it has been done yet.
>
> Nothing in `release.yml` can produce a usable release until this checklist is
> complete. Work top to bottom; each step ends in a verification you can run.
>
> See also: `docs/release-runbook.md` (cutting a release),
> `docs/security-runbook.md` (rotation, renewal, storage policy).

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

*Also verified:* `crates/dat0-core/assets/minisign-public-key.txt` is currently
**byte-identical** to line 2 of
`crates/dat0-core/tests/fixtures/update/test-minisign.pub` — both are
`RWR/aKYzRk3oeZJDzLCZ/nooGJs2wLOVhTKMMaqJvOEWyFKpf53Ir9RW`. The shipped binary
therefore trusts the development test key.
`crates/dat0-core/tests/update_key_is_production.rs` fails on this today, by
design, and is the gate that closes when this step is done.

### Commands

```bash
# -W = passwordless, DELIBERATELY. docs/security-runbook.md:170-190 records the
# reasoning: the P10a GPG key hit a passphrase-propagation failure in CI, the
# minisign key is disposable (rotate on compromise), and the GitHub Secret
# encryption is the real access control — a passphrase adds nothing and
# reintroduces that failure class.
rsign generate -W -p dat0-minisign.pub -s dat0-minisign.key

# The .pub file is two lines: an untrusted comment, then the base64 key.
# ONLY the second line goes into the repository.
sed -n '2p' dat0-minisign.pub > crates/dat0-core/assets/minisign-public-key.txt

# Verify the file is exactly one line of base64 and nothing else.
wc -l < crates/dat0-core/assets/minisign-public-key.txt   # must print 1
cat crates/dat0-core/assets/minisign-public-key.txt       # must NOT contain "untrusted comment"
```

Then, in **Settings → Secrets and variables → Actions**, create secret
`MINISIGN_SECRET_KEY` with the **full contents** of `dat0-minisign.key`
(all lines, including the `untrusted comment:` header — `rsign sign -s` reads
the whole file).

```bash
pbcopy < dat0-minisign.key      # macOS; paste into the GitHub Secret form
shred -u dat0-minisign.key 2>/dev/null || rm -P dat0-minisign.key
```

**Never commit `dat0-minisign.key`.** Keep one offline backup in a password
manager (`docs/security-runbook.md:193-196`).

### Verification

```bash
# `#[ignore]`d until this step lands, so it needs --ignored to run at all.
cargo test -p dat0-core --test update_key_is_production -- --ignored  # must PASS after this step
# Then DELETE the #[ignore] in the same commit: it belongs in every run once it passes.
cargo test -p dat0-core --test update_manifest            # must still PASS (uses the test fixture, unaffected)
```

---

## 2. GPG signing subkey for the Linux AppImage

**Why.** `xtask/src/linux.rs`'s `gpg_sign` produces the detached
`dat0.AppImage.sig` that `xtask verify --linux` and every user's
`gpg --verify` check. Without an imported key the Linux job fails at signing.

`docs/security-runbook.md:120-123` recommends a **passphraseless dedicated CI
subkey** — the "Empty or unset" branch of `docs/release-runbook.md:67-71` — and
that is why `xtask/src/linux.rs` omits `--pinentry-mode loopback
--passphrase-fd 0` when `DAT0_GPG_PASSPHRASE` is empty or unset: passing those
flags with an empty passphrase makes gpg **fail** on a passphraseless key
rather than skip the prompt.

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
# docs/security-runbook.md:132-134 names this exact path, and it DOES NOT EXIST
# in the tree today (verified: no docs/*.asc file is present).
gpg --armor --export <fingerprint> > docs/dat0-signing-key.asc
git add docs/dat0-signing-key.asc
```

Create secret `GPG_PRIVATE_KEY` from the contents of `dat0-ci-signing.key`,
then delete that file. Leave `GPG_PASSPHRASE` **unset** if you followed the
recommended passphraseless path — `release.yml` wires it through as
`DAT0_GPG_PASSPHRASE`, and empty is treated as "no passphrase".

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
base64 -i ~/Desktop/dat0-dev-id.p12   | pbcopy   # → APPLE_DEV_ID_CERT_P12
base64 -i ~/Desktop/AuthKey_XXXX.p8   | pbcopy   # → AC_API_KEY_P8
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

Delete the `.p12` and `.p8` from disk once the secrets are set.

---

## 4. `nyc_taxi.parquet` sample asset (PD-012)

**Why.** `crates/dat0-core/src/sample_data.rs:46` ships
`pub const NYC_TAXI_SHA256: &str = "FILL_AT_T8";` — verified against the tree.
`NYC_TAXI_URL` (`:41`) already points at release tag `sample-data-v1`, so the
remote sample either fails its integrity check or, worse, is accepted against a
placeholder.

```bash
# 1. Publish the asset under the tag the constant already names.
gh release create sample-data-v1 --title "dat0 sample data v1" --notes "NYC taxi Parquet sample" \
  || true
gh release upload sample-data-v1 nyc_taxi.parquet

# 2. Hash it and paste the hex into sample_data.rs:46.
shasum -a 256 nyc_taxi.parquet
```

Replace `"FILL_AT_T8"` with the 64-character hex digest and commit. Then close
PD-012 in `docs/deferrals.md`.

*(`crates/dat0-core/src/sample_data.rs` is not edited by RL1-RL4; this is a
documented hand-off, not a pending code change in this workstream.)*

---

## 5. GlitchTip crash reporting

`docs/release-runbook.md` ("GlitchTip / crash reporting") describes the
instance. Three secrets are needed before `crash-e2e.yml` stops self-skipping:

| Secret | Where to find it |
|---|---|
| `DAT0_GLITCHTIP_DSN_PUBLIC` | GlitchTip → project → Settings → Client Keys (the full `https://…@host/N` URL) |
| `GLITCHTIP_API_TOKEN` | GlitchTip → account settings → API Tokens (read access to the dat0 project) |
| `GLITCHTIP_PROJECT_SLUG` | the slug in the project URL, e.g. `dat0` |

Development and CI builds use the stub `https://stub@glitchtip.invalid/1` from
`.cargo/config.toml`, so nothing is emitted until a release build is compiled
with the real DSN.

---

## After the checklist

```bash
cargo test -p dat0-core --test update_key_is_production   # step 1
cargo test -p dat0-core --test update_manifest_roundtrip  # wire contract (no secrets needed)
gh workflow run release.yml && gh run watch              # steps 2 + 3 end to end
```

The `workflow_dispatch` dry run exercises `macos` and `linux` and skips
`publish` (gated on `github.ref_type == 'tag'`), so it proves the signing and
notarization chain but not manifest signing. That gap is covered locally by
`crates/dat0-core/tests/update_manifest_roundtrip.rs`.

### What the dry run must confirm on the two RL4 fixes

Both were real defects and both are fixed in code; the dry run is where the
fixes get their first execution.

- **AppImage dependency bundling.** `xtask/src/linux.rs` now runs a
  `linuxdeploy --appdir … --executable … --desktop-file … --icon-file …` pass
  before `appimagetool`, and hard-fails if that pass leaves `AppDir/AppRun`
  absent or `AppDir/usr/lib` empty. Previously the AppDir held the bare binary
  and no AppRun at all. The `Verify on clean Ubuntu` step installs only the
  AppImage excludelist's host baseline (`libfontconfig1`, `libx11-6`, `libgl1`,
  glib, …) into the container — every dat0-specific library (`libssl`,
  `libpango`, `libxkbcommon`, `libsecret`, DuckDB's own) must come out of the
  AppDir. **Confirm at the dry run:** that step passes, and that the exact
  baseline package list resolves on `ubuntu:24.04` (noble renamed glib to
  `libglib2.0-0t64`). A missing package name fails loudly and precisely.
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
