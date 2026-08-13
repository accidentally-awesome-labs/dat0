# dat0 Security Runbook

> Operational security reference for P10a and P10a-2's signing and notarization secrets.
> Covers renewal cadence, key storage, rotation procedures, and the
> SECURITY.md inbox process.
>
> Scope: **P10a secrets** (Apple Developer cert, GPG signing key, notary
> API key) **+ P10a-2 minisign update-signing key** (`latest.json` signature).

---

## Apple Developer ID Certificate

### What it is

The `Developer ID Application` certificate (stored in GitHub Actions as
`APPLE_DEV_ID_CERT_P12` + `CERT_PASSWORD`) is issued by Apple and used to
codesign `dat0.dmg` and its enclosed binary. Without a valid cert, Gatekeeper
blocks the app on any macOS user's machine.

### Renewal cadence

Developer ID certificates are valid for **5 years** from issuance. Apple sends
email reminders at 30 days and again at 7 days before expiry, but do not rely
solely on those emails.

**60-day-before-expiry reminder (spec R12):**
Set a recurring calendar reminder (or a GitHub Actions scheduled workflow alert)
to fire 60 days before the certificate's `Not After` date. Check the current
expiry with:

```bash
# On the Mac that holds the private key:
security find-certificate -c "Developer ID Application" -p | \
    openssl x509 -noout -dates
```

### Renewal procedure

1. In Xcode → Settings → Accounts, or via the Apple Developer portal
   (developer.apple.com → Certificates), request a new Developer ID Application
   certificate.
2. Download and install it into your local Keychain.
3. Export the new cert + private key as a `.p12` (Keychain Access → right-click
   → Export; set a strong export password).
4. Base64-encode and update the `APPLE_DEV_ID_CERT_P12` GitHub secret:
   ```bash
   base64 -i ~/Desktop/dat0-dev-id-new.p12 | pbcopy
   ```
5. Update `CERT_PASSWORD` to the new export password.
6. Update `DAT0_SIGN_IDENTITY` if the certificate's Common Name or Team ID
   changed (it usually does not).
7. Run a `workflow_dispatch` dry run (`gh workflow run release.yml`) to confirm
   the new cert works before the old one expires.
8. Delete the `.p12` file from your Desktop after the secret is updated.

Keep the old cert installed locally until confirmed the new one signs and
notarizes cleanly — Apple's transition window allows both to be valid
simultaneously.

---

## App Store Connect (Notary) API Key

### What it is

The `AC_API_KEY_P8`, `AC_KEY_ID`, and `AC_ISSUER_ID` secrets are the
credentials `xcrun notarytool` uses to submit the signed DMG to Apple for
notarization. The `.p8` key is downloaded **once** from App Store Connect;
Apple does not let you re-download it.

### Storage

- The `.p8` file must be kept in a secure location (encrypted keychain or
  password manager). If lost, revoke the API key in App Store Connect and
  create a new one.
- Do not store the raw `.p8` file in any git repository. Only the base64-encoded
  value belongs in a GitHub Secret.

### Rotation

API keys do not expire but should be rotated if the key is believed compromised:

1. App Store Connect → Users and Access → Integrations → App Store Connect API
   → Revoke the current key.
2. Create a new key (type: Developer).
3. Download the new `.p8` immediately (shown once).
4. Base64-encode and update `AC_API_KEY_P8`.
5. Update `AC_KEY_ID` and `AC_ISSUER_ID` if they changed (the issuer UUID does
   not change with key rotation; the key ID does).
6. Run a `workflow_dispatch` dry run to confirm notarization works.

---

## GPG Signing Key (Linux AppImage)

### What it is

The GPG private key (`GPG_PRIVATE_KEY`) is used to produce the detached
`.sig` file alongside `dat0.AppImage`. Users verify with:
```bash
gpg --verify dat0.AppImage.sig dat0.AppImage
```

The public key should be published at a well-known URL (e.g. in the README and
on the project website) so users can import it before verifying.

### Passphrase decision — wired for BOTH postures (RL4)

> Full details in `docs/release-runbook.md` under
> **"Linux GPG signing — passphrase wiring"**.

The P10a defect — `GPG_PASSPHRASE` set only in the "Import GPG key" step's
`env:`, which does not propagate to the "Bundle + sign" step — is fixed.
`release.yml` now passes `DAT0_GPG_PASSPHRASE` to the signing step, and
`xtask/src/linux.rs::gpg_sign` adds `--pinentry-mode loopback
--passphrase-fd 0` **only when that variable is non-empty**, feeding the
passphrase over stdin rather than argv.

- **(Recommended, and the default posture)** a **passphraseless** dedicated CI
  signing subkey. Leave `GPG_PASSPHRASE` unset; `gpg_sign` omits the loopback
  flags entirely, which is required — passing `--passphrase-fd 0` with an empty
  passphrase makes gpg fail on such a key rather than skip the prompt.
- **(Alternative)** a passphrase-protected key. Set `GPG_PASSPHRASE`; the
  loopback path engages automatically. No workflow edit needed.

Provisioning either key: `docs/release-prerequisites.md` step 2.

### Key storage

- Store the private key (and passphrase, if set) in a password manager.
- The GitHub Secret holds the exported ASCII-armored key; the corresponding
  public key should be committed to the repository (`docs/dat0-signing-key.asc`
  or similar) and published so users can verify artifacts independently.

### Rotation

Rotate the signing key if the private key is believed compromised, or on a
regular schedule (e.g. yearly for the CI subkey):

1. Generate a new key (or subkey):
   ```bash
   gpg --quick-gen-key "dat0 CI Signing <security@yourorg.example>" ed25519 sign 2y
   ```
2. Export and update `GPG_PRIVATE_KEY` in GitHub Secrets.
3. If passphraseless (recommended), `GPG_PASSPHRASE` remains empty or vestigial.
4. Publish the new public key alongside the old one (do not revoke the old key
   immediately — existing users may still hold it for verifying past releases).
5. Annotate the release notes for the first release signed with the new key.
6. Run a `workflow_dispatch` dry run to confirm signing works.

---

## minisign Update-Signing Key (`latest.json`)

### What it is

The minisign secret key is used by the `release.yml` CI workflow to sign the
`latest.json` update manifest. The in-app updater (`dat0-core`) fetches
`latest.json` and verifies its minisign signature before trusting any version
or download URL. The app embeds the public key at build time via:

```rust
const MINISIGN_PUBLIC_KEY: &str = include_str!("../assets/minisign-public-key.txt");
```

(`crates/dat0-core/assets/minisign-public-key.txt`). The app **verifies only**
— it never holds the secret key.

### Passphrase decision — **passwordless CI key** (deliberate)

The CI key is generated without a passphrase (`-W` flag):

```bash
# Generate a new passwordless minisign key pair:
minisign -G -W -p minisign-public-key.txt -s minisign-secret-key.key
# Or with rsign (Rust re-implementation):
rsign generate -W -p minisign-public-key.txt -s minisign-secret-key.key
```

**Passwordless is deliberate.** The P10a GPG signing key ran into a
passphrase-propagation pitfall in `release.yml` — `GPG_PASSPHRASE` was set in
the "Import" step's `env:` but did not reach the "Bundle + sign" step, so the
detach-sign hung waiting for a PIN. See the
[GPG "Passphrase decision" note](#passphrase-decision--wired-for-both-postures-rl4)
above. A passwordless minisign key avoids that class of bug entirely: the key
is disposable (rotate on compromise), and the GitHub Secret encryption is the
sole access control — a passphrase would add no meaningful protection over the
secret itself. The CI signs `latest.json` non-interactively with no PIN prompt.

### Storage

- **Secret key contents** — stored only as the `MINISIGN_SECRET_KEY` GitHub
  Actions repository secret (the full ASCII content of `minisign-secret-key.key`)
  plus an offline backup in a password manager. The raw file must **never be
  committed** to the repository.
- **Public key** — committed to the repository at
  `crates/dat0-core/assets/minisign-public-key.txt` and embedded by the app
  via `include_str!`. This is the only key material that belongs in version
  control.

To load the secret in CI (`release.yml`):

```yaml
- name: Sign latest.json
  env:
    MINISIGN_SECRET_KEY: ${{ secrets.MINISIGN_SECRET_KEY }}
  run: |
    echo "$MINISIGN_SECRET_KEY" > /tmp/minisign.key
    rsign sign -s /tmp/minisign.key latest.json
    rm /tmp/minisign.key
```

### Rotation

Rotate the minisign key if the secret is believed compromised, or on a regular
schedule (e.g. yearly):

1. Generate a new passwordless key pair (see "Passphrase decision" above).
2. Update `MINISIGN_SECRET_KEY` in GitHub Secrets with the new secret key contents.
3. Replace `crates/dat0-core/assets/minisign-public-key.txt` with the new public
   key and commit it.
4. Cut a new release that ships the updated embedded public key. Clients running
   the **old** embedded key can still verify **old** releases — they cannot verify
   releases signed with the new key until they update. This is the migration
   caveat: there is no cross-signing or key-transition window; prompt users to
   update before rotating if the old key is not compromised. In a compromise
   scenario, rotate immediately and accept that clients still on the old embedded
   key will see signature-verification failures (and fall back to the manual
   nudge) until they update; communicate this urgently via the release notes and
   a SECURITY advisory.
5. Annotate the release notes for the first release signed with the new key.
6. Run a `workflow_dispatch` dry run to confirm the new key signs and verifies
   cleanly.

### Current state — dry-run blocker

> **The production key is NOT yet provisioned.** `crates/dat0-core/assets/minisign-public-key.txt`
> currently holds the **T1 test key** (generated during development). Before any
> real release, a human MUST:
>
> 1. Generate a production passwordless key pair (see above).
> 2. Set `MINISIGN_SECRET_KEY` to the new secret key in GitHub Secrets.
> 3. Replace the embedded public key in `crates/dat0-core/assets/minisign-public-key.txt`
>    and commit it.
>
> Until these steps are done, either the test key ships to real users (invalid for
> production releases) or the CI sign step fails (no production secret set). The
> full end-to-end validation is a `workflow_dispatch` dry-run + the clean-VM UAT
> in `docs/plans/2026-06-22-dat0-p10a-2-uat.md`.
>
> **This is now enforced, not just documented.**
> `crates/dat0-core/tests/update_key_is_production.rs` compares the embedded key
> against the committed test fixture and **fails while they are identical** —
> which is the state of the tree today. The exact commands are
> `docs/release-prerequisites.md` step 1.

---

## Keychain Bootstrap Password

The `KEYCHAIN_PASSWORD` secret is used only to create and unlock the ephemeral
`build.keychain` within the macOS CI runner. It never leaves the runner and is
not cryptographically significant. Rotate it anytime (e.g. on team membership
changes) by generating a new random value and updating the secret.

---

## macOS Entitlements (hardened runtime)

**The P10a release uses an empty entitlements set.** The dat0 binary, DuckDB,
and the Metal rendering layer all operate correctly under the hardened runtime
with no special entitlements. This is the most restricted, most Gatekeeper-
friendly posture.

If a future dependency requires an entitlement (e.g. JIT compilation for a
scripting engine, or `com.apple.security.network.client` for a new network
surface), add it to the xtask `sign` step's entitlements file, document it
here, and re-notarize. Entitlement additions require a fresh notarization
submission; they are not retroactive.

---

## SECURITY.md inbox process

The `SECURITY.md` file at the repository root describes the responsible
disclosure policy and the intake email address for security reports.

On receipt of a security report:

1. Acknowledge within **48 hours** (even if just "received, investigating").
2. Reproduce and assess severity (CVSS score or equivalent).
3. For confirmed vulnerabilities:
   - Do NOT discuss on public issues or PRs until patched.
   - Prepare a fix on a private branch.
   - Coordinate a disclosure date with the reporter (90-day max).
   - Cut a patch release using this runbook.
   - Publish a GitHub Security Advisory after the patch ships.
4. Update `SECURITY.md` if the contact address or process changes.

---

## See also

- `docs/release-runbook.md` — full secret inventory, cut-a-release steps
- `docs/plans/2026-06-17-dat0-p10a-uat.md` — P10a clean-VM verification checklist
- `docs/plans/2026-06-22-dat0-p10a-2-uat.md` — P10a-2 auto-update clean-VM UAT checklist
- `SECURITY.md` — public-facing disclosure policy
- `docs/deferrals.md` D-003 / D-004 — closed by P10a-2 (unified Rust updater superseded Sparkle/AppImageUpdate)
- `docs/deferrals.md` D-028 — privileged `/Applications` auto-update (SMJobBless helper, v1.x)
