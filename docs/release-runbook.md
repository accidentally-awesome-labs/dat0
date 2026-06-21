# dat0 Release Runbook

> Operational reference for cutting a signed, notarized dat0 release.
> Covers the full secret inventory, cut-a-release steps, artifact verification,
> and pointers to clean-VM UAT.
>
> Pipeline file: `.github/workflows/release.yml` (commit `f2dae56`).
> Security ops (cert renewal, key rotation): see `docs/security-runbook.md`.

---

## Secret inventory

All secrets live in the GitHub repository's **Settings → Secrets and variables →
Actions** store. Never commit any of these values to source control.

| Secret name | What it is | How to populate |
|---|---|---|
| `APPLE_DEV_ID_CERT_P12` | Base64-encoded Developer ID Application `.p12` exported from Keychain Access. Includes the private key + the full cert chain. | `base64 -i ~/Desktop/dat0-dev-id.p12` → paste the output. |
| `CERT_PASSWORD` | Password chosen when exporting the `.p12` from Keychain Access. | Plain string; set when you do the export. |
| `KEYCHAIN_PASSWORD` | Password used to create the ephemeral CI build keychain (`build.keychain`). Choose any strong value — it is used only within the runner. | Generate once; store alongside the other Apple secrets. |
| `AC_API_KEY_P8` | Base64-encoded App Store Connect API key (`.p8` file). Obtained from App Store Connect → Users and Access → Integrations → App Store Connect API. | `base64 -i ~/Desktop/AuthKey_XXXX.p8` → paste the output. |
| `AC_KEY_ID` | The key ID displayed next to the `.p8` in App Store Connect (e.g. `ABC1234567`). | Copy from the App Store Connect UI. |
| `AC_ISSUER_ID` | The issuer UUID shown at the top of the App Store Connect API keys page (e.g. `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`). | Copy from the App Store Connect UI. |
| `GPG_PRIVATE_KEY` | ASCII-armored GPG private key used to sign `dat0.AppImage`. Export with `gpg --armor --export-secret-keys <fingerprint>`. | Paste the full `-----BEGIN PGP PRIVATE KEY BLOCK-----` block. |
| `GPG_PASSPHRASE` | Passphrase protecting the GPG private key (if set). | See the **Linux GPG signing — passphrase wiring** note below. |
| `DAT0_SIGN_IDENTITY` | The full `Developer ID Application: …` string as it appears in `security find-identity -v -p codesigning`. | Copy verbatim, including the parenthetical Team ID suffix. |

The notary tool (`xcrun notarytool`) receives its credentials at runtime via
environment variables set in the CI **"Bundle + sign + notarize"** step:
`AC_KEY_ID`, `AC_ISSUER_ID`, and `AC_API_KEY_PATH` (the path `/tmp/ac_api_key.p8`
to the decoded key file written by the preceding "Notary API key file" step).

### macOS entitlements (important)

The hardened-runtime signature uses **no entitlements**. The `dat0.app` binary,
DuckDB, and the Metal rendering path all operate cleanly under the hardened
runtime with an **empty entitlements set** — no JIT, no unsigned-memory, no
special allowances required. This is the minimal, most-hardened signing posture.
If a future dependency requires entitlements, add them in the xtask `sign` step
and document them here.

---

## Linux GPG signing — passphrase wiring (verify at dry run)

> **KNOWN DRY-RUN BLOCKER. Confirm this before cutting the first release.**

In `release.yml`, `GPG_PASSPHRASE` is set only in the **"Import GPG key"**
step's `env:` block. GitHub Actions step-scoped `env:` does **not** propagate
to later steps. The actual signing command (`gpg --batch --yes --detach-sign
dat0.AppImage`) runs in the separate **"Bundle + sign"** step and currently
passes no passphrase.

If the CI signing key is passphrase-protected, `gpg --batch --detach-sign`
will fail non-interactively at the dry run.

**Resolution options (pick one before shipping):**

**(a) Recommended — provision a passphraseless dedicated CI signing subkey.**
Generate a dedicated subkey with no passphrase specifically for CI use:
```
gpg --quick-add-key <fingerprint> ed25519 sign 2y
```
Export only this subkey as `GPG_PRIVATE_KEY`. The `GPG_PASSPHRASE` secret
becomes vestigial; `--batch --detach-sign` just works.

**(b) Enable gpg loopback pinentry in the sign step.**
Add `GPG_PASSPHRASE` to the "Bundle + sign" step's `env:` and amend the
signing invocation (small follow-up to T8's gpg call):
```bash
echo "$GPG_PASSPHRASE" | gpg --batch --yes --pinentry-mode loopback \
    --passphrase-fd 0 --detach-sign dat0.AppImage
```
Also add `allow-loopback-pinentry` to the `gpg-agent.conf` written during
the import step.

**UAT checklist item (§ Dry run, step 4):** confirm which option is in effect
and that `dat0.AppImage.sig` is a valid detached signature before cutting a
production tag.

---

## Artifact layout (produced by `release.yml`)

| Artifact | Job | Contents |
|---|---|---|
| `dat0.dmg` | `macos` | Signed + notarized + stapled universal DMG (arm64 + x86_64 lipo'd binary) |
| `dat0.AppImage` | `linux` | Signed Linux AppImage (x86_64) |
| `dat0.AppImage.sig` | `linux` | Detached binary GPG signature (`.sig`, NOT `.asc`) |
| `SHA256SUMS` | `publish` | SHA-256 checksums for the DMG, AppImage, and `.sig` |

---

## Cut a release — step by step

### 1. Bump the version

Edit `Cargo.toml` (root, `[workspace.package]` section). The current version
is `0.1.0`. Bump it following semver:

```toml
[workspace.package]
version = "1.0.0"
```

### 2. Regenerate NOTICE if dependencies changed

If you added, removed, or updated any Cargo dependency since the last release,
regenerate `NOTICE.md` **on a Linux host** (PD-003: `cargo about generate`
output is not byte-identical across macOS and Linux; the CI gate runs on Linux,
so the committed NOTICE must match what Linux generates):

```bash
cargo about generate -c about.toml docs/about-template.hbs > NOTICE.md
```

If no dependency changed since the last regeneration, skip this step — the
existing `NOTICE.md` is already in sync (confirmed by the CI `notice` job).

### 3. Commit and tag

```bash
git add Cargo.toml Cargo.lock NOTICE.md   # add NOTICE.md only if regenerated
git commit --signoff -m "chore: bump version to 1.0.0"
git tag -a v1.0.0 -m "dat0 v1.0.0"
git push origin main
git push origin v1.0.0
```

### 4. Watch the release workflow

The `v1.0.0` tag push triggers `release.yml`. Monitor the run:

```bash
gh run list --workflow=release.yml --limit 5
gh run watch <run-id>
```

Jobs run in order: `macos` → `linux` (both parallel) → `publish` (tag-only).
If any job fails, check its log for the failing step. Common failure modes are
documented in the troubleshooting section below.

### 5. Watch the post-merge main run (project CI lesson)

The push to `main` (step 3) also triggers `ci.yml`. The macOS grid-scroll
bench is push-to-main-only and is the first indicator of a disk-reclaim
regression. Confirm it goes green:

```bash
gh run list --workflow=ci.yml --limit 3
gh run watch <main-run-id>
```

Do **not** declare a release green until both `release.yml` and the post-merge
`ci.yml` main run are confirmed green.

### 6. Verify the release artifacts

After `publish` completes, confirm the GitHub Release page has:
- `dat0.dmg`
- `dat0.AppImage`
- `dat0.AppImage.sig`
- `SHA256SUMS`

Download and verify locally:

```bash
# macOS
shasum -a 256 dat0.dmg | grep -F "$(grep dat0.dmg SHA256SUMS | awk '{print $1}')"

# Linux AppImage + signature
sha256sum dat0.AppImage | grep -F "$(grep dat0.AppImage SHA256SUMS | awk '{print $1}')"
gpg --verify dat0.AppImage.sig dat0.AppImage   # must print "Good signature"
```

### 7. Run the clean-VM UAT

See `docs/plans/2026-06-17-dat0-p10a-uat.md` for the full manual checklist.
Do not mark a release shipped until all UAT items are checked.

---

## Workflow dispatch — dry run

To test the pipeline without publishing a release, trigger `release.yml`
manually via `workflow_dispatch`:

```bash
gh workflow run release.yml
```

All jobs run normally but the `publish` job is skipped (gated on
`github.ref_type == 'tag'`). Artifacts are uploaded and available for download
from the Actions run summary. Use this for:

- First-time pipeline validation (esp. the GPG passphrase wiring check above).
- Testing cert/key rotation after a renewal.
- Confirming a notarization path still works after an Xcode update.

Record the run URL in the release notes or in the team chat for traceability.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `security import` fails (cert not trusted) | `APPLE_DEV_ID_CERT_P12` is stale or the cert expired | Re-export the `.p12` from a Mac with the valid cert installed; update the secret. |
| `codesign` exits non-zero (`The specified item could not be found`) | `DAT0_SIGN_IDENTITY` string does not exactly match the cert CN | Run `security find-identity -v -p codesigning` on the signing Mac; copy verbatim. |
| `notarytool submit` returns `Invalid` status | Entitlement or binary issue | Run `xcrun notarytool log <submission-id>` to see the notarization report. |
| `gpg --detach-sign` exits non-zero / prompts | Passphrase not wired to the sign step | See **Linux GPG signing — passphrase wiring** section above. |
| `publish` job skipped on a tag push | Preceding `macos` or `linux` job failed | Fix the failing job first; re-push the tag after fixing the source. |
| `notice` CI gate warns after a dep change | NOTICE.md regenerated on macOS, not Linux | Regenerate on Linux; commit the result (see step 2 above). |

---

## See also

- `docs/security-runbook.md` — cert renewal cadence, key rotation, SECURITY.md process
- `docs/plans/2026-06-17-dat0-p10a-uat.md` — clean-VM UAT checklist
- `docs/ci-mac-vm-runner.md` — tart-based self-hosted macOS runner (D-013)
- `.github/workflows/release.yml` — the pipeline itself
- `xtask/src/` — the `bundle-macos`, `sign-macos`, `bundle-linux`, `verify` subcommands
