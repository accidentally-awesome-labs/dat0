# dat0 Release Runbook

Public operational reference for preparing, packaging, verifying and publishing a release. There are no published binaries yet; the artifact table below describes the intended release output, not available downloads. [Documentation index](README.md).

See [release prerequisites](release-prerequisites.md) for outstanding human provisioning, [security operations](security-runbook.md) for rotation/private reporting, and [CI](ci.md) for contributor checks. Ordinary contributors do not need signing or service credentials.

## Trust boundary and secret inventory

[Release workflow](../.github/workflows/release.yml) accepts only the canonical `accidentally-awesome-labs/dat0` repository: manual dispatch on `refs/heads/main`, or a push of a `refs/tags/v*` tag. The no-secret `validate-ref` job rejects other refs/events before build or signing. For tags, it fetches main and requires `git merge-base --is-ancestor "$GITHUB_SHA" origin/main`. This checks main ancestry, not merely a tag name.

`gate`, `macos`, `linux` and `publish` use the protected **release** environment, whose deployment policies permit exactly the `main` branch and `v*` tags. Put signing, notary, minisign and GPG material in **Settings → Environments → release → Secrets**; never commit values. Telemetry credentials remain separately scoped to the workflows that consume them. The release DSN must be accessible to release jobs; ordinary CI/heavy builds use the development stub, not a production override.

| Secret | Purpose |
|---|---|
| `APPLE_DEV_ID_CERT_P12` | Base64 Developer ID Application certificate, private key and chain |
| `CERT_PASSWORD` | Certificate export password |
| `KEYCHAIN_PASSWORD` | Ephemeral runner keychain password |
| `AC_API_KEY_P8` | Base64 App Store Connect API private key |
| `AC_KEY_ID`, `AC_ISSUER_ID` | Notary API identifiers |
| `DAT0_SIGN_IDENTITY` | Exact Developer ID Application identity including Team ID |
| `GPG_PRIVATE_KEY` | ASCII-armored dedicated Linux signing subkey |
| `GPG_PASSPHRASE` | Optional passphrase for that subkey |
| `MINISIGN_SECRET_KEY` | Full production update-signing secret key |
| `GLITCHTIP_DSN_PUBLIC` | Crash DSN compiled as `DAT0_GLITCHTIP_DSN_PUBLIC` |

Export private files to an operator-selected protected directory, not a Desktop/shared folder. For example, with `PROTECTED_DIR` set to that directory, use `base64 -i "$PROTECTED_DIR/dat0-dev-id.p12"` or `base64 -i "$PROTECTED_DIR/AuthKey_XXXX.p8"` when provisioning. Protect offline backups and remove temporary exports after successful provisioning.

The [release preflight](../xtask/src/release.rs) checks workspace/tag version, production update key, DSN, sample hash and nine required signing-secret presence flags. It receives presence booleans rather than signing values; the DSN is checked separately. Tag findings stop the release; manual-run findings are warnings. Missing prerequisites are not completed by a successful unsigned run.

## Artifact layout

| Published artifact | Producer | Provisioned release contents |
|---|---|---|
| `dat0-<version>-universal.dmg` | macos | Developer ID signed, notarized, stapled universal macOS app (arm64 + x86_64) |
| `dat0-<version>-x86_64.AppImage` | linux | Linux x86_64 AppImage |
| `dat0-<version>-x86_64.AppImage.sig` | linux | Detached binary GPG signature, not `.asc` |
| `dat0.app.tar.gz` | macos | Signed, notarized and stapled update bundle, re-tarred after stapling |
| `latest.json`, `latest.json.minisig` | publish | Signed updater manifest |
| `SHA256SUMS` | publish | Checksums for DMG, AppImage, signature and tarball |

There is no Linux aarch64 release artifact. Build/upload names under `target/` stay unversioned (`dat0.dmg`, `dat0.AppImage`); publish stages versioned basenames. The tarball is deliberately unversioned because the [manifest generator](../xtask/src/manifest.rs) uses that URL. Artifacts are downloaded from the same workflow run before publication.

## Prepare through a contribution PR

1. Branch from current `upstream/main`; bump `[workspace.package].version` in root `Cargo.toml` and update the lockfile as necessary. Follow [CONTRIBUTING](../CONTRIBUTING.md), including DCO sign-off on every commit. Open a focused release-preparation PR; do not push preparation commits directly to main.
2. If dependencies changed, regenerate NOTICE on Linux with `scripts/notice-regen.sh` using its pinned cargo-about version. It replaces only the generated marker block; never overwrite NOTICE with raw cargo-about output. On macOS use the failed NOTICE check's correction artifact. The reusable `notice` job is always called by CI.
3. Validate relevant contributor checks and obtain `CI required` and external `DCO` success, resolve review threads, and merge normally. Do not use skip directives that leave required checks pending. Observe the resulting main CI run, including main-only live-service checks. Hosted PR checks and eight Linux real-window probes do not require production secrets; optional `run-heavy`/`run-perf` labels are explained in [CI](ci.md).
4. On a supported release desktop, run `cargo xtask perf --check`. All six scenarios need actual measurements and budget verdicts. The helper builds selected release artifacts itself. `SKIP` is not a measurement. See [public budgets](performance-baselines.json) and [CI performance guidance](ci.md). Absolute budget failures or regressions over the recorded host baseline require investigation; never loosen baselines to hide a regression. Any justified baseline update is a separate reviewed change. Attach measurements to the preparation PR.
5. Run the main-only dry run below and record its run URL, revision, unsigned/signed mode and actual outcomes in the release PR. Complete provisioned checks before publishing.

## Workflow dispatch — dry run

```bash
gh workflow run release.yml --repo accidentally-awesome-labs/dat0 --ref main
gh run list --repo accidentally-awesome-labs/dat0 --workflow release.yml --limit 5
gh run watch <run-id> --repo accidentally-awesome-labs/dat0
```

Manual main runs never publish, even if signing material is present. Without secrets, macOS creates an ad-hoc-signed app disk image and Linux an unsigned AppImage. Builds, bundle/version/architecture/Info.plist checks, artifact uploads and Linux smoke checks still run. Linux AppImage smoke covers Ubuntu 22.04, Debian 12 and Ubuntu 24.04; inspect the `appimage-smoke` screenshots, not only exit status. The AppImage bundles selected dependencies but uses host WebKitGTK/GTK/GLib; see [Linux packaging](../xtask/src/linux.rs) and [smoke script](../scripts/appimage-smoke.sh).

With provisioned secrets, manual runs can exercise platform signing and notarization. They still do **not** exercise production manifest signing or publication because `publish` is skipped. Fixture-based [manifest verification tests](../xtask/tests/manifest_verify.rs) and [wire round-trip tests](../crates/dat0-core/tests/update_manifest_roundtrip.rs) do not prove a production key was used. Record explicitly which checks were not exercised. No successful provisioned signing run is claimed here.

## Publish a provisioned release

After preparation has merged and acceptance evidence is complete, fetch upstream main and tag the intended reviewed main-history revision. Only the authorized maintainer may create/update/delete production `v*` tags under the release-tag ruleset.

```bash
git fetch upstream
git tag -a v1.0.0 <reviewed-main-revision> -m "dat0 v1.0.0"
git push upstream v1.0.0
```

Use the actual workspace version, not this example literally. The sequence is `validate-ref` → `gate` → `macos` and `linux` → `publish`. Only a canonical **tag push** with a `v*` ref can publish; manual branch/tag dispatches other than main are rejected. Tags always require signing and all preflight findings to be resolved, rather than silently shipping unsigned.

`publish` signs the update manifest and runs `cargo xtask verify-manifest` against the committed public key before uploading. Inspect all jobs and the release asset list; no skipped signing step is a signed-release success. Do not casually move a published tag to repair failures: diagnose first and follow maintainer tag policy.

## Signed artifact verification and manual acceptance

These are provisioned release checks, **not** prerequisites for every contribution or proof supplied by an unsigned dry run. Use clean machines and synthetic data; record OS/architecture, revision, artifact hashes, commands, observations and unperformed items in the release PR. Do not declare a release shipped until all applicable acceptance items have evidence.

- **Installation / Gatekeeper:** install on a clean macOS machine, assess the notarized/stapled app and DMG, and confirm Gatekeeper permits launch. Install and launch the Linux AppImage on clean supported hosts. Verify actual window rendering.
- **Identity / integrity:** compare CLI `--version`, About version and Git SHA with the tagged build; verify each artifact against `SHA256SUMS`, and verify the detached Linux signature with the published trusted public key (`gpg --verify "dat0-$V-x86_64.AppImage.sig" "dat0-$V-x86_64.AppImage"`). Use `shasum -a 256` on macOS or `sha256sum -c SHA256SUMS` on Linux. Check signature fingerprint against the independently distributed key, not just “Good signature.” Run `cargo xtask verify --macos` / `--linux` on corresponding provisioned build artifacts.
- **Packages / routing:** verify `.dat0` file association opens read-only Inspect, not a mutable workspace. Test restart persistence and second-launch forwarding to the existing process with identical profile variables; confirm intended package/workspace opens and normal shutdown.
- **Updater round trip:** on disposable test installations, verify signed-manifest download, verification, artifact download and apply, followed by relaunch/version confirmation. Verify launch-check opt-out and explicit manual checks; exercise non-writable-install fallback. Reject tampered manifests, manifests signed by the wrong key, and artifacts with a wrong hash. Do not substitute test-key fixture success for the production-key path.
- **Synthetic telemetry:** verify default-off consent, no submission before opting in, receipt after a deliberate synthetic submission, and crash-specific redaction of sensitive path patterns. Inspect the received event, not only the client exit code. Use synthetic notes/data, never private datasets or real credentials. Local logs/user notes are not universally sanitized; see [privacy](privacy.md).

## Signing details and troubleshooting

macOS hardened-runtime signing currently uses no entitlements; if a dependency requires them, change [signing implementation](../xtask/src/sign.rs) through review and document the reason.

[Linux signing](../xtask/src/linux.rs) reads `DAT0_GPG_PASSPHRASE`. Empty/unset means a passphraseless dedicated CI subkey and plain batch detached signing. A set passphrase uses loopback pinentry and stdin, never argv. The import step enables `allow-loopback-pinentry`; Bundle + sign has its own passphrase environment because step-local values do not propagate. Verify `.sig` existence and `cargo xtask verify --linux` only when GPG signing was actually enabled.

| Failure | Action |
|---|---|
| Ref/ancestry rejection | Dispatch main or tag a reviewed main-history revision in the canonical repo |
| Preflight finding | Complete the corresponding prerequisite; never downgrade a tag finding |
| Certificate import / codesign identity | Renew/re-export certificate and match `DAT0_SIGN_IDENTITY` exactly |
| Notary rejection | Inspect `xcrun notarytool log <submission-id>`; do not waive Gatekeeper |
| GPG prompt/failure | Match passphrase wiring to the selected subkey |
| Manifest signature rejection | Match secret key to the committed production public key; nothing should publish |
| NOTICE mismatch | Regenerate with the canonical Linux script / correction artifact |
| Publish skipped | Manual run: expected. Tag run: inspect failed dependencies/event guard |

## Crash-report wiring and rotation

The release DSN is compiled into the binary via `DAT0_GLITCHTIP_DSN_PUBLIC`; development/ordinary CI uses `.cargo/config.toml`'s stub. This concerns crash reporting, not all application networking.

The main-only canonical [crash-e2e workflow](../.github/workflows/crash-e2e.yml) separately consumes `GLITCHTIP_DSN_PUBLIC`, `GLITCHTIP_API_TOKEN` (read access to project issues) and `GLITCHTIP_PROJECT_SLUG`. Missing secrets cause an explicit skip. Build/run failures remain hard failures; remote receipt assertion is advisory. It is not a required PR verdict.

For a provisioned manual synthetic check, build with the selected DSN and run `target/release/dat0 __telemetry-test`; confirm the `dat0 telemetry e2e` event arrives. This directly submits a synthetic message without a real panic or crash sentinel. Spot-check `filepath`, `abs_path` and `module` redaction against [redactor source](../crates/dat0-core/src/telemetry/redaction.rs).

For DSN rotation, create the replacement in the service, update the workflow-scoped `GLITCHTIP_DSN_PUBLIC`, and cut a new reviewed build. Existing binaries keep their compiled DSN until users update. Plan old-key revocation with the service owner; a compromised credential may require immediate revocation rather than waiting for adoption. Follow [security operations](security-runbook.md) and [private reporting](../SECURITY.md).
