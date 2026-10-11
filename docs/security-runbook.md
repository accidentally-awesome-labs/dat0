# dat0 Security Runbook

Operational reference for release key storage, renewal, rotation, and private vulnerability response. See the [documentation index](README.md), [public security policy](../SECURITY.md), [release prerequisites](release-prerequisites.md), and [release runbook](release-runbook.md).

## Trust and provisioning boundary

Store signing, notarization, minisign, GPG, and ephemeral build-keychain material in the protected GitHub Actions **`release` environment**, with offline backups in an encrypted password manager. Never commit private keys or put their values in logs, issues, or documentation. Public verification keys belong in version control and public distribution channels.

Release validation accepts only canonical-repository `v*` tag pushes or manual dispatch on `main`; tag commits must be ancestors of `main`. Manual main runs never publish. The release environment permits the `main` branch and `v*` tags. Ordinary PR checks run on hosted runners without release secrets. Release preparation changes go through a DCO-signed PR, not a direct push to main.

Telemetry credentials are separately scoped to the workflows and steps that consume them; they are not release signing material. Ordinary contributor checks use the development telemetry configuration, not production credentials. See [CI](ci.md) for required checks and main-only live-service traffic.

A manual run uses:

```bash
gh workflow run release.yml --repo accidentally-awesome-labs/dat0 --ref main
```

When signing material is absent, the existing unsigned dry-run path can exercise packaging and platform smoke checks. It does **not** validate production manifest signing or prove release provisioning complete. When credentials are installed, a manual main run can exercise signing without publication. Follow the release runbook's provisioned acceptance checks before publishing a tag.

## Apple Developer ID Certificate

`APPLE_DEV_ID_CERT_P12` and `CERT_PASSWORD` hold the exported certificate and private key used to codesign the DMG and enclosed binary. `DAT0_SIGN_IDENTITY` identifies the signing identity. A signed distribution also needs successful notarization and clean-machine Gatekeeper verification.

Developer ID certificates are valid for five years from issuance. Set an operator reminder 60 days before expiry; do not rely solely on Apple's reminders. On the Mac holding the certificate, inspect its dates:

```bash
security find-certificate -c "Developer ID Application" -p | \
  openssl x509 -noout -dates
```

To renew:

1. Request a new Developer ID Application certificate through Xcode Accounts or the Apple Developer portal and install it in the local Keychain.
2. Export the certificate and private key as a password-protected `.p12` to an operator-selected protected path.
3. Base64-encode it and update `APPLE_DEV_ID_CERT_P12` in the `release` environment. Update `CERT_PASSWORD` and, if changed, `DAT0_SIGN_IDENTITY`.
4. Run the manual main workflow and complete signing, notarization, and clean-machine verification before the old certificate expires.
5. Remove temporary exported material securely according to the operator's storage policy; retain the encrypted backup.

Keep the old certificate available during the verified transition while both certificates remain valid.

## App Store Connect Notary API Key

`AC_API_KEY_P8`, `AC_KEY_ID`, and `AC_ISSUER_ID` authenticate `xcrun notarytool`. Apple allows the `.p8` download only once. Keep it in encrypted storage; the base64-encoded value belongs only in the `release` environment secret, never in Git.

If compromised or lost, revoke the key in App Store Connect → Users and Access → Integrations, create a replacement Developer key, and download it immediately. Update `AC_API_KEY_P8`, `AC_KEY_ID`, and any changed issuer ID. Verify notarization with a manual main run. API keys do not expire automatically.

## GPG Signing Key (Linux AppImage)

`GPG_PRIVATE_KEY` produces the detached AppImage signature. Publish the public key and fingerprint through the release documentation and project website so users can import the key and independently verify:

```bash
gpg --verify dat0.AppImage.sig dat0.AppImage
```

### Passphrase wiring

A dedicated passphraseless CI signing subkey is the current recommended posture; leave `GPG_PASSPHRASE` unset. A passphrase-protected key is also supported. The workflow forwards `GPG_PASSPHRASE` as `DAT0_GPG_PASSPHRASE` to the signing step. `xtask/src/linux.rs::gpg_sign` adds loopback pinentry and `--passphrase-fd 0` only when that variable is non-empty, sending the passphrase over stdin, not argv. See [release prerequisites](release-prerequisites.md) and the release runbook for provisioning.

### Storage and rotation

Keep the private key and any passphrase in encrypted offline storage and the `release` environment. Commit and publish only the public key; do not link to an unprovisioned public-key file as if it already existed.

Rotate on suspected compromise or on the operator's renewal schedule:

1. Generate a new key or signing subkey, for example:
   ```bash
   gpg --quick-gen-key "dat0 CI Signing <security@dat0.app>" ed25519 sign 2y
   ```
2. Update `GPG_PRIVATE_KEY` and, when needed, `GPG_PASSPHRASE` in the `release` environment. Remove an obsolete passphrase when moving to a passphraseless key.
3. Publish the new public key and fingerprint, and identify the first release signed by it. For routine rotation, retain old public keys for historical artifact verification.
4. For compromise, revoke the compromised key and publish the revocation and incident guidance; do not delay revocation merely to preserve historical trust.
5. Exercise signing and independent signature verification in a manual main run before publication.

## minisign Update-Signing Key (`latest.json`)

The updater verifies the signed update manifest before trusting its version or download URL. Its public key is embedded from `crates/dat0-core/assets/minisign-public-key.txt`; the app never holds the secret key.

The CI key is passwordless for non-interactive signing:

```bash
minisign -G -W -p minisign-public-key.txt -s dat0-minisign.key
# Alternative Rust implementation:
rsign generate -W -p minisign-public-key.txt -s dat0-minisign.key
```

Generate keys only on an appropriate protected operator system. Store secret contents as `MINISIGN_SECRET_KEY` in the `release` environment and in an encrypted offline backup. Protect temporary files and never commit them. `rsign sign -W` avoids a terminal password prompt on CI; environment protection and secret access control remain essential.

The publication path signs `latest.json` and then runs `cargo xtask verify-manifest` against the committed public key before uploading. A successful unsigned packaging run does not exercise this path.

### Rotation and client migration

1. Generate a replacement key pair and update `MINISIGN_SECRET_KEY` in the `release` environment.
2. Replace the committed embedded public key through a reviewed, DCO-signed PR.
3. Coordinate the client migration: clients with the old embedded key cannot verify manifests signed by the new key. There is no cross-signing or key-transition window. For routine rotation, arrange and communicate a client update before switching keys.
4. On compromise, rotate immediately and publish an advisory and manual-update instructions. Old clients can see signature failures until updated; do not weaken verification to bypass them.
5. Identify the first release using the replacement key and verify signing, wrong-key rejection, tamper rejection, and artifact hashes using the release runbook.

### Production provisioning remains required

The production update key is not yet provisioned; the committed public key is the development test key. Before any production release, provision `MINISIGN_SECRET_KEY`, replace the embedded key, and complete the provisioned release checks. The ignored production-key test in `crates/dat0-core/tests/update_key_is_production.rs` rejects the committed test key. See [release prerequisites](release-prerequisites.md) for commands and [release acceptance](release-runbook.md) for clean-machine and update verification. Do not represent this prerequisite as complete based on an unsigned dry run.

## Build Keychain and Hardened Runtime

`KEYCHAIN_PASSWORD` creates and unlocks the ephemeral macOS build keychain. Store it in the `release` environment and rotate it when operator access changes or compromise is suspected; it is not an application credential.

The release uses an empty hardened-runtime entitlements set. If a dependency requires an additional entitlement, review its necessity, change the xtask signing configuration through a PR, document the added privilege, and notarize the resulting artifact again. Entitlements are not retroactive.

## Private vulnerability response

The [public policy](../SECURITY.md) defines private advisory reporting and **security@dat0.app**, with best-effort acknowledgment within 7 days, assessment within 14 days, and a fix or mitigation plan within 30 days for high-severity reports. An internal aim to acknowledge within 48 hours is not a separate public promise or SLA.

On receipt:

1. Acknowledge privately, reproduce safely, and assess impact and severity. Do not request production credentials or private datasets when synthetic input suffices.
2. Keep undisclosed details out of public issues, PRs, and workflow logs. Prepare fixes in a **temporary private GitHub security advisory fork** or a **separate private repository**. A branch of this public repository is not private.
3. Coordinate disclosure and reporter credit or anonymity. Route upstream-only defects upstream while assessing their effect on dat0.
4. Land the fix on `main` at coordinated disclosure and publish a fixed revision/advisory; publish a fixed release when releases exist. There is not yet a released-version/backport support matrix.
5. For exposed credentials, have the service owner revoke them, provision replacements through the proper protected channel, and review affected artifacts and access. Do not expose secret values in the advisory.
6. Update the public policy if reporting routes or supported-version commitments change.

## See also

- [Release runbook](release-runbook.md): packaging, signing, manual acceptance, and publication.
- [Release prerequisites](release-prerequisites.md): provisioning checklist and production-key verification.
- [Security policy](../SECURITY.md): public disclosure policy.
- [Privacy guide](privacy.md): application network and data boundaries.
- [Support](../SUPPORT.md): non-sensitive questions and conduct reporting.
