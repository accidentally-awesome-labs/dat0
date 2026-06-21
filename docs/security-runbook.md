# dat0 Security Runbook

> Operational security reference for P10a's signing and notarization secrets.
> Covers renewal cadence, key storage, rotation procedures, and the
> SECURITY.md inbox process.
>
> Scope: **P10a secrets only** (Apple Developer cert, GPG signing key, notary
> API key). EdDSA appcast-signing-key rotation is documented when the in-app
> updater lands in P10a-2; the shipped P10a code does not yet consume that key.

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

### Passphrase decision (verify at first dry run)

> See the full details in `docs/release-runbook.md` under
> **"Linux GPG signing — passphrase wiring"**.

The current `release.yml` sets `GPG_PASSPHRASE` only in the "Import GPG key"
step's `env:` — it does **not** propagate to the "Bundle + sign" step where
`gpg --batch --detach-sign` actually runs. Before the first release:

- **(Recommended)** Provision a **passphraseless** dedicated CI signing subkey.
  This is the simplest posture: the CI key is disposable (rotate on compromise),
  so a passphrase adds minimal security over the GitHub Secret encryption.
- **(Alternative)** Wire `GPG_PASSPHRASE` into the sign step and use
  `--pinentry-mode loopback`. See the release runbook for exact flags.

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

## EdDSA appcast signing key (P10a-2)

> **Not yet active in P10a.** The shipped P10a code delivers a minimal update
> *nudge* only (About box → "Download latest" link to the GitHub Releases page).
> The in-app Sparkle bridge (D-003) and AppImageUpdate subprocess (D-004) move
> to the P10a-2 updater slice.

When P10a-2 lands, this section will document: the `ed25519` appcast signing
key pair, rotation procedure, the Sparkle `SUPublicEDKey` bundle string, and
the AppImage update-info URL. Until then, no appcast key rotation is needed.

---

## See also

- `docs/release-runbook.md` — full secret inventory, cut-a-release steps
- `docs/plans/2026-06-17-dat0-p10a-uat.md` — clean-VM verification checklist
- `SECURITY.md` — public-facing disclosure policy
- `docs/deferrals.md` D-003 / D-004 — Sparkle bridge / AppImageUpdate (P10a-2)
