# dat0 Release Runbook

> Operational reference for cutting a signed, notarized dat0 release.
> Covers the full secret inventory, cut-a-release steps, artifact verification,
> and pointers to clean-VM UAT.
>
> Pipeline file: `.github/workflows/release.yml` (commit `f2dae56`).
> Security ops (cert renewal, key rotation): see `docs/security-runbook.md`.
>
> **Before the first release:** `docs/release-prerequisites.md` (RL1) lists the
> five human-only prerequisites — minisign key, GPG subkey, Apple enrolment,
> the `nyc_taxi.parquet` asset, GlitchTip credentials. None of them are done
> yet, and none of the steps below can succeed until they are.

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

## Linux GPG signing — passphrase wiring

> **Resolved (RL4).** The wiring defect described here for P10a is fixed; what
> follows documents the behaviour that shipped, not an open decision.

The original defect: `GPG_PASSPHRASE` was declared only in the **"Import GPG
key"** step's `env:`, and GitHub Actions step-scoped `env:` does not propagate.
The actual signing command runs in the separate **"Bundle + sign"** step, so a
passphrase-protected key would hang or fail non-interactively.

**What `release.yml` does now.** The **"Bundle + sign"** step declares
`DAT0_GPG_PASSPHRASE: ${{ secrets.GPG_PASSPHRASE }}`, and the **"Import GPG
key"** step writes `allow-loopback-pinentry` into `~/.gnupg/gpg-agent.conf`.

**What `xtask` does with it.** `xtask/src/linux.rs::gpg_sign` reads
`DAT0_GPG_PASSPHRASE` and branches:

- **Empty or unset** (the recommended posture — a dedicated passphraseless CI
  signing subkey, `docs/security-runbook.md:120-123`) → plain
  `gpg --batch --yes --detach-sign`. The loopback flags are **omitted**, not
  passed empty: `--passphrase-fd 0` with an empty passphrase makes gpg fail on
  a passphraseless key rather than skip the prompt.
- **Set** → `gpg --batch --yes --pinentry-mode loopback --passphrase-fd 0
  --detach-sign`, with the passphrase written to the child's **stdin**. Never
  on argv, which `ps` exposes on a shared runner.

So both key postures work, and the recommended one needs no secret at all.

**Provisioning either key:** `docs/release-prerequisites.md` step 2.

**Dry-run check:** confirm `dat0.AppImage.sig` exists and that
`cargo xtask verify --linux` (which runs `gpg --verify`) passes.

---

## Artifact layout (produced by `release.yml`)

| Artifact | Job | Contents |
|---|---|---|
| `dat0-<version>-universal.dmg` | `macos` | Signed + notarized + stapled universal DMG (arm64 + x86_64 lipo'd binary) |
| `dat0-<version>-x86_64.AppImage` | `linux` | Signed Linux AppImage (x86_64) |
| `dat0-<version>-x86_64.AppImage.sig` | `linux` | Detached binary GPG signature (`.sig`, NOT `.asc`) |
| `dat0.app.tar.gz` | `macos` | macOS auto-update payload: the **signed, notarized and stapled** bundle, re-tarred by `sign::sign_and_notarize` after `stapler staple` (`macos::tar_app`). **Unversioned by design** — `xtask/src/manifest.rs` points `latest.json`'s macOS URL at this exact name. |
| `latest.json` + `.minisig` | `publish` | minisign-signed update manifest |
| `SHA256SUMS` | `publish` | SHA-256 checksums for the DMG, AppImage, `.sig`, and the tarball |

In-`target/` and in-artifact filenames stay **unversioned** (`dat0.dmg`,
`dat0.AppImage`) because `xtask::sign::verify` and the `publish` job address
them by fixed path. The version is applied by the publish job's
**"Stage release assets under their versioned names"** step, because
`gh release` names each asset after its basename and has no rename flag. These
are the names `README.md:36-38` documents to users.

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

### 5. Run the perf gate on the release host

```bash
cargo xtask perf --check
```

All six scenarios must pass. This is the only place the 60 fps / 1 s cold-launch
/ 200 MB idle-RSS numbers the marketing page prints are actually checked against
a running window — `ci.yml`'s copy is advisory (`continue-on-error`), because a
virtualized GPU cannot defend a frame-rate claim.

Attach the emitted JSON lines to the release PR. Two arms can fail:

- **`FAIL … over budget`** — the absolute number in
  `docs/internal/perf-baselines.json`. Do not ship; the claim would be false.
- **`FAIL … worse than the recorded`** — a >20% regression against this host's
  own committed baseline. Investigate before shipping; if the regression is
  understood and accepted, re-record with `--update-baseline` **in its own
  commit**, so the loosening is a reviewed act rather than a side effect.

`RECORD` (not `PASS`) on every scenario means this host has no committed
baseline yet. That is expected on a new machine; run `--update-baseline` once
and commit the result.

This step may be run earlier, on the release branch, since its output belongs on
the release PR. It is placed here so that a release cut without it is visibly
skipping a numbered step.

### 6. Watch the post-merge main run (project CI lesson)

The push to `main` (step 3) also triggers `ci.yml`. The macOS grid-scroll
bench is push-to-main-only and is the first indicator of a disk-reclaim
regression. Confirm it goes green:

```bash
gh run list --workflow=ci.yml --limit 3
gh run watch <main-run-id>
```

Do **not** declare a release green until both `release.yml` and the post-merge
`ci.yml` main run are confirmed green.

### 7. Verify the release artifacts

After `publish` completes, confirm the GitHub Release page has (with
`<version>` = the tag without its leading `v`):
- `dat0-<version>-universal.dmg`
- `dat0-<version>-x86_64.AppImage`
- `dat0-<version>-x86_64.AppImage.sig`
- `dat0.app.tar.gz`
- `latest.json` and `latest.json.minisig`
- `SHA256SUMS`

Download and verify locally:

```bash
V=1.0.0   # the release version

# macOS
shasum -a 256 "dat0-$V-universal.dmg" | grep -F "$(grep "dat0-$V-universal.dmg" SHA256SUMS | awk '{print $1}')"

# Linux AppImage + signature
sha256sum "dat0-$V-x86_64.AppImage" | grep -F "$(grep "dat0-$V-x86_64.AppImage " SHA256SUMS | awk '{print $1}')"
gpg --verify "dat0-$V-x86_64.AppImage.sig" "dat0-$V-x86_64.AppImage"   # must print "Good signature"
```

### 8. Run the clean-VM UAT

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

### Last verified dry run

**NOT YET RUN — requires RL1 secrets** (`docs/release-prerequisites.md`).

```bash
gh workflow run release.yml && gh run watch
```

Record the run URL here once it is green:

| Date | Run URL | Result |
|---|---|---|
| — | — | not yet run |

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `security import` fails (cert not trusted) | `APPLE_DEV_ID_CERT_P12` is stale or the cert expired | Re-export the `.p12` from a Mac with the valid cert installed; update the secret. |
| `codesign` exits non-zero (`The specified item could not be found`) | `DAT0_SIGN_IDENTITY` string does not exactly match the cert CN | Run `security find-identity -v -p codesigning` on the signing Mac; copy verbatim. |
| `notarytool submit` returns `Invalid` status | Entitlement or binary issue | Run `xcrun notarytool log <submission-id>` to see the notarization report. |
| `gpg --detach-sign` exits non-zero / prompts | `DAT0_GPG_PASSPHRASE` set for a passphraseless key, or unset for a protected one | See **Linux GPG signing — passphrase wiring** above; the variable must match the key. |
| `publish` job skipped on a tag push | Preceding `macos` or `linux` job failed | Fix the failing job first; re-push the tag after fixing the source. |
| `notice` CI gate warns after a dep change | NOTICE.md regenerated on macOS, not Linux | Regenerate on Linux; commit the result (see step 2 above). |

---

## See also

- `docs/security-runbook.md` — cert renewal cadence, key rotation, SECURITY.md process
- `docs/release-prerequisites.md` — the five human-only prerequisites (RL1)
- `docs/plans/2026-06-17-dat0-p10a-uat.md` — clean-VM UAT checklist
- `docs/ci-mac-vm-runner.md` — tart-based self-hosted macOS runner (D-013)
- `.github/workflows/release.yml` — the pipeline itself
- `xtask/src/` — the `bundle-macos`, `sign-macos`, `bundle-linux`, `verify` subcommands

---

## GlitchTip / crash reporting

> **Added P10c (2026-06-25).** Covers the self-hosted GlitchTip instance that
> receives opt-in crash reports and Report-a-Bug events from dat0.

### Instance and DSN provenance

dat0 uses a **self-hosted GlitchTip instance** (confirmed by the user: only the
GlitchTip path exists; managed/Sentry is a fallback option noted in spec R11 if
the self-hosted instance becomes unmaintainable). The public DSN is baked into
the release binary at compile time via `env!("DAT0_GLITCHTIP_DSN_PUBLIC")` in
`crates/dat0-core/src/telemetry/`. Development and CI builds use the stub value
`https://stub@glitchtip.invalid/1` (set in `.cargo/config.toml`) so no reports
are sent during development. Only a release build compiled with the real DSN emits events.

### CI secrets

Add these three secrets to the GitHub repository
(**Settings → Secrets and variables → Actions**) before the `crash-e2e` soft
gate runs live:

| Secret name | What it is | Where to find it |
|---|---|---|
| `GLITCHTIP_DSN_PUBLIC` | Public DSN for the dat0 GlitchTip project (the full `https://…@host/N` URL). Used as `DAT0_GLITCHTIP_DSN_PUBLIC` at build time. | GlitchTip → project → Settings → Client Keys. |
| `GLITCHTIP_API_TOKEN` | Personal or bot API token with read access to the dat0 project's issues. | GlitchTip → account settings → API Tokens. |
| `GLITCHTIP_PROJECT_SLUG` | The URL slug for the dat0 project (e.g. `dat0`). | Visible in the GlitchTip project URL: `…/organizations/<org>/projects/<slug>/`. |

The `crash-e2e.yml` workflow has `continue-on-error: true` (design D4 — a live
external dependency must never redden `main`). The job is a clean skip when any
of the three secrets are absent.

### DSN rotation

If the DSN needs to be rotated (key compromise, project re-key):

1. In GlitchTip: generate a new DSN key for the dat0 project.
2. Update `GLITCHTIP_DSN_PUBLIC` in GitHub Secrets.
3. Cut a new release build — the DSN is baked in at compile time, so the old
   binary continues to use the old key until users update.
4. Revoke the old key in GlitchTip only after the majority of users are on a
   build that carries the new DSN (check crash-volume drop on the old key).

### Postgres backups and uptime monitoring

GlitchTip persists data in Postgres. Recommended hygiene:

- **Daily backups:** `pg_dump` or managed-snapshot at the hosting layer.
  Retain at least 7 days of snapshots.
- **Uptime check:** add the GlitchTip web URL to an uptime monitor (e.g.
  UptimeRobot free tier). Alert to a team channel. If the instance goes dark,
  dat0 continues operating normally (the SDK drops events silently when the
  DSN is unreachable); no user data is lost, but crash signals stop arriving.
- **Spec R11 fallback:** if the self-hosted instance becomes unmaintainable,
  migrate to Sentry's managed free tier. The DSN format is compatible;
  rotation procedure above applies.

### Reading incoming crash issues

1. Open GlitchTip → dat0 project → **Issues**.
2. Each crash shows: exception type, top-of-stack, OS/arch/version, and the
   free-form note the user appended before submitting.
3. Filter by `environment:production` to exclude test events.
4. Look for the `dat0 telemetry e2e` tag/title to identify CI smoke events;
   these can be muted or filtered as noise once the gate is stable.
5. **Redaction check (periodic):** spot-check that `filepath`, `abs_path`, and
   `module` fields do not contain real home-directory paths. The redactor
   (`telemetry/redact.rs`) strips `/Users/<name>/` and `/home/<name>/`; confirm
   it is still active after any telemetry refactor.

### Manual smoke test

To verify the live round-trip outside of CI, build with the real DSN and run:

```bash
DAT0_GLITCHTIP_DSN_PUBLIC="<real-dsn>" cargo build -p dat0-ui --release --bin dat0
./target/release/dat0 __telemetry-test
# Wait ~15 s, then check GlitchTip Issues for "dat0 telemetry e2e"
```

The `__telemetry-test` subcommand (implemented in T10) emits a synthetic
`message`-type event with the title `"dat0 telemetry e2e"` and exits 0. It
exercises the full submission path (panic hook bypassed; direct `sentry::capture_message`)
without triggering a real panic or leaving a crash sentinel on disk.

See also `docs/plans/2026-06-24-dat0-p10c-uat.md` §4 for the full manual UAT
sequence.
